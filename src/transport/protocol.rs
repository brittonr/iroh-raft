//! Iroh protocol implementation for Raft consensus
//!
//! This module provides the protocol handler for Raft messages over Iroh P2P,
//! following the Iroh protocol patterns for proper connection handling.

use bytes::Bytes;
use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};
use snafu::Backtrace;
use std::future::Future;
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::error::{RaftError, Result};

/// ALPN protocol identifier for iroh-raft
pub const RAFT_ALPN: &[u8] = b"iroh-raft/0";

/// Message types for the protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// RPC request message
    Request,
    /// RPC response message  
    Response,
    /// Direct Raft consensus message
    RaftMessage,
    /// Heartbeat message for connection keepalive
    Heartbeat,
    /// Error message indicating a protocol failure
    Error,
}

/// Protocol message header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHeader {
    /// Type of message being sent
    pub msg_type: MessageType,
    /// Unique request identifier for correlation (16 random bytes)
    pub request_id: Option<[u8; 16]>,
    /// Length of the message payload in bytes
    pub payload_len: u32,
}

/// RPC request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Service name being called (e.g., "raft")
    pub service: String,
    /// Method name being invoked (e.g., "raft.message")
    pub method: String,
    /// Serialized request payload
    pub payload: Vec<u8>,
}

/// RPC response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Whether the RPC call was successful
    pub success: bool,
    /// Serialized response payload if successful
    pub payload: Option<Vec<u8>>,
    /// Error message if unsuccessful
    pub error: Option<String>,
}

/// Generate a unique request ID
pub fn generate_request_id() -> [u8; 16] {
    use rand::RngCore;
    let mut id = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut id);
    id
}

/// Serialize a payload
pub fn serialize_payload<T: Serialize>(payload: &T) -> Result<Vec<u8>> {
    bincode::serialize(payload).map_err(|e| RaftError::Internal {
        message: format!("Failed to serialize payload: {}", e),
        backtrace: Backtrace::new(),
    })
}

/// Deserialize a payload
pub fn deserialize_payload<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T> {
    bincode::deserialize(data).map_err(|e| RaftError::Internal {
        message: format!("Failed to deserialize payload: {}", e),
        backtrace: Backtrace::new(),
    })
}

/// Write a message to a stream
pub async fn write_message<W>(
    stream: &mut W,
    msg_type: MessageType,
    request_id: Option<[u8; 16]>,
    payload: &[u8],
) -> Result<()> 
where
    W: AsyncWriteExt + Unpin,
{
    let header = MessageHeader {
        msg_type,
        request_id,
        payload_len: payload.len() as u32,
    };

    let header_bytes = serialize_payload(&header)?;
    let header_len = header_bytes.len() as u32;

    // Write header length (4 bytes)
    stream
        .write_all(&header_len.to_be_bytes())
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to write header length: {}", e),
            backtrace: Backtrace::new(),
        })?;

    // Write header
    stream
        .write_all(&header_bytes)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to write header: {}", e),
            backtrace: Backtrace::new(),
        })?;

    // Write payload
    stream
        .write_all(payload)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to write payload: {}", e),
            backtrace: Backtrace::new(),
        })?;

    stream.flush().await.map_err(|e| RaftError::Internal {
        message: format!("Failed to flush stream: {}", e),
        backtrace: Backtrace::new(),
    })?;

    Ok(())
}

/// Read a message from a stream
pub async fn read_message<R>(
    stream: &mut R,
) -> Result<(MessageHeader, Vec<u8>)> 
where
    R: AsyncReadExt + Unpin,
{
    // Read header length (4 bytes)
    let mut header_len_bytes = [0u8; 4];
    stream
        .read_exact(&mut header_len_bytes)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to read header length: {}", e),
            backtrace: Backtrace::new(),
        })?;

    let header_len = u32::from_be_bytes(header_len_bytes) as usize;

    // Read header
    let mut header_bytes = vec![0u8; header_len];
    stream
        .read_exact(&mut header_bytes)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to read header: {}", e),
            backtrace: Backtrace::new(),
        })?;

    let header: MessageHeader = deserialize_payload(&header_bytes)?;

    // Read payload
    let mut payload = vec![0u8; header.payload_len as usize];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to read payload: {}", e),
            backtrace: Backtrace::new(),
        })?;

    Ok((header, payload))
}

/// Raft protocol handler for Iroh
#[derive(Debug, Clone)]
pub struct RaftProtocolHandler {
    /// Channel to send incoming Raft messages to the Raft manager
    raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    /// Our node ID
    node_id: u64,
}

impl RaftProtocolHandler {
    /// Create a new Raft protocol handler
    pub fn new(
        node_id: u64,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Self {
        Self {
            raft_rx_tx,
            node_id,
        }
    }

    /// Handle an incoming connection
    async fn handle_connection(&self, connection: Connection) -> Result<()> {
        // Accept bidirectional streams for Raft messages
        loop {
            match connection.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    let raft_rx_tx = self.raft_rx_tx.clone();
                    let node_id = self.node_id;

                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::handle_stream(&mut send, &mut recv, raft_rx_tx, node_id).await
                        {
                            tracing::warn!("Failed to handle Raft stream: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::debug!("Connection closed: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle a bidirectional stream
    async fn handle_stream<S, R>(
        send: &mut S,
        recv: &mut R,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
        _node_id: u64,
    ) -> Result<()> 
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        // Read the message
        let (header, payload) = read_message(recv).await?;

        match header.msg_type {
            MessageType::Request => {
                // Handle RPC request
                let request: RpcRequest = deserialize_payload(&payload)?;

                if request.service == "raft" && request.method == "raft.message" {
                    // Deserialize Raft message
                    let message: raft::prelude::Message =
                        crate::raft::messages::deserialize_message(&request.payload)?;

                    // Get the sender's node ID from the message
                    let from = message.from;

                    // Send to Raft manager
                    if let Err(e) = raft_rx_tx.send((from, message)) {
                        tracing::error!("Failed to send Raft message to manager: {}", e);
                        
                        // Send error response
                        let response = RpcResponse {
                            success: false,
                            payload: None,
                            error: Some(format!("Failed to process message: {}", e)),
                        };
                        
                        let response_bytes = serialize_payload(&response)?;
                        write_message(
                            send,
                            MessageType::Response,
                            header.request_id,
                            &response_bytes,
                        )
                        .await?;
                    } else {
                        // Send success response
                        let response = RpcResponse {
                            success: true,
                            payload: None,
                            error: None,
                        };
                        
                        let response_bytes = serialize_payload(&response)?;
                        write_message(
                            send,
                            MessageType::Response,
                            header.request_id,
                            &response_bytes,
                        )
                        .await?;
                    }
                } else {
                    // Unknown service/method
                    let response = RpcResponse {
                        success: false,
                        payload: None,
                        error: Some(format!("Unknown service/method: {}/{}", request.service, request.method)),
                    };
                    
                    let response_bytes = serialize_payload(&response)?;
                    write_message(
                        send,
                        MessageType::Response,
                        header.request_id,
                        &response_bytes,
                    )
                    .await?;
                }
            }
            MessageType::RaftMessage => {
                // Direct Raft message (legacy path)
                let message = crate::raft::messages::deserialize_message(&payload)?;
                let from = message.from;
                
                if let Err(e) = raft_rx_tx.send((from, message)) {
                    tracing::error!("Failed to send Raft message to manager: {}", e);
                }
            }
            _ => {
                tracing::warn!("Unexpected message type: {:?}", header.msg_type);
            }
        }

        // Flush the send stream
        send.flush().await.map_err(|e| RaftError::Internal {
            message: format!("Failed to flush stream: {}", e),
            backtrace: Backtrace::new(),
        })?;

        Ok(())
    }
}

/// Implement the accept method for handling connections
impl RaftProtocolHandler {
    /// Accept and handle an incoming connection
    pub fn accept(
        &self,
        connection: Connection,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
        let handler = self.clone();
        Box::pin(async move {
            handler
                .handle_connection(connection)
                .await
                .map_err(|e| anyhow::anyhow!("Protocol handler error: {}", e))
        })
    }
}