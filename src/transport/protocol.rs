//! Iroh protocol implementation for Raft consensus
//!
//! This module provides the protocol handler for Raft messages over Iroh P2P,
//! following the Iroh protocol patterns for proper connection handling with
//! zero-copy optimizations for high-performance message processing.

use bytes::{BufMut, Bytes, BytesMut};
use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};
use snafu::Backtrace;
use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::error::{RaftError, Result};

/// ALPN protocol identifier for iroh-raft
pub const RAFT_ALPN: &[u8] = b"iroh-raft/0";

/// Threshold for considering a message "large" and using streaming (64KB)
pub const LARGE_MESSAGE_THRESHOLD: usize = 64 * 1024;

/// Maximum header size for pre-allocation optimization
pub const MAX_HEADER_SIZE: usize = 256;

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

/// Zero-copy message structure for high-performance message handling
///
/// This structure uses borrowing and Bytes for efficient memory management,
/// avoiding unnecessary allocations in hot paths.
#[derive(Debug, Clone)]
pub struct ZeroCopyMessage<'a> {
    /// Message type for protocol dispatch
    pub msg_type: MessageType,
    /// Optional request ID for correlation
    pub request_id: Option<[u8; 16]>,
    /// Zero-copy payload data using either borrowed slice or Bytes
    pub payload: Cow<'a, [u8]>,
    /// Pre-computed message size for efficient serialization
    pub total_size: u32,
}

impl<'a> ZeroCopyMessage<'a> {
    /// Create a new zero-copy message with borrowed payload
    #[inline]
    pub fn new_borrowed(
        msg_type: MessageType,
        request_id: Option<[u8; 16]>,
        payload: &'a [u8],
    ) -> Self {
        let total_size = Self::calculate_size(payload.len());
        Self {
            msg_type,
            request_id,
            payload: Cow::Borrowed(payload),
            total_size,
        }
    }

    /// Create a new zero-copy message with owned Bytes payload
    #[inline]
    pub fn new_owned(
        msg_type: MessageType,
        request_id: Option<[u8; 16]>,
        payload: Bytes,
    ) -> Self {
        let total_size = Self::calculate_size(payload.len());
        Self {
            msg_type,
            request_id,
            payload: Cow::Owned(payload.to_vec()),
            total_size,
        }
    }

    /// Calculate the total message size including headers
    #[inline]
    const fn calculate_size(payload_len: usize) -> u32 {
        // 4 bytes for header length + estimated header size + payload length
        4 + 32 + payload_len as u32 // Conservative header size estimate
    }

    /// Convert to owned message for storage or further processing
    pub fn into_owned(self) -> ZeroCopyMessage<'static> {
        ZeroCopyMessage {
            msg_type: self.msg_type,
            request_id: self.request_id,
            payload: Cow::Owned(self.payload.into_owned()),
            total_size: self.total_size,
        }
    }

    /// Get payload as bytes slice
    #[inline]
    pub fn payload_bytes(&self) -> &[u8] {
        &self.payload
    }

    /// Get payload length
    #[inline]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Check if this is a large message that should use streaming
    #[inline]
    pub fn is_large_message(&self) -> bool {
        self.payload.len() > LARGE_MESSAGE_THRESHOLD
    }
}

/// Zero-copy RPC request with efficient payload handling
#[derive(Debug, Clone)]
pub struct ZeroCopyRpcRequest<'a> {
    /// Service name (typically short, so owned is fine)
    pub service: &'static str,
    /// Method name (typically short, so owned is fine)  
    pub method: &'static str,
    /// Zero-copy payload data
    pub payload: Cow<'a, [u8]>,
}

impl<'a> ZeroCopyRpcRequest<'a> {
    /// Create new zero-copy RPC request with borrowed payload
    #[inline]
    pub fn new_borrowed(
        service: &'static str,
        method: &'static str,
        payload: &'a [u8],
    ) -> Self {
        Self {
            service,
            method,
            payload: Cow::Borrowed(payload),
        }
    }

    /// Create new zero-copy RPC request with owned payload
    #[inline]
    pub fn new_owned(
        service: &'static str,
        method: &'static str,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            service,
            method,
            payload: Cow::Owned(payload),
        }
    }

    /// Convert to the standard RpcRequest for backward compatibility
    pub fn to_standard(&self) -> RpcRequest {
        RpcRequest {
            service: self.service.to_string(),
            method: self.method.to_string(),
            payload: self.payload.to_vec(),
        }
    }
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

/// Zero-copy serialization using pre-allocated buffer
///
/// This function attempts to serialize directly into a BytesMut buffer
/// to avoid intermediate allocations for better performance.
pub fn serialize_payload_zero_copy<T: Serialize>(payload: &T, buf: &mut BytesMut) -> Result<()> {
    // Pre-allocate space if buffer is empty
    if buf.is_empty() {
        buf.reserve(1024); // Reasonable default for most messages
    }

    let writer = buf.writer();
    bincode::serialize_into(writer, payload).map_err(|e| RaftError::Internal {
        message: format!("Failed to serialize payload: {}", e),
        backtrace: Backtrace::new(),
    })
}

/// Optimized header serialization for zero-copy messages
///
/// Serializes directly into a pre-allocated buffer to minimize allocations.
pub fn serialize_header_zero_copy(header: &MessageHeader, buf: &mut BytesMut) -> Result<()> {
    // Reserve space for header (conservative estimate)
    buf.reserve(MAX_HEADER_SIZE);
    
    let writer = buf.writer();
    bincode::serialize_into(writer, header).map_err(|e| RaftError::Internal {
        message: format!("Failed to serialize header: {}", e),
        backtrace: Backtrace::new(),
    })
}

/// Zero-copy message deserialization
///
/// Deserializes from a Bytes buffer without copying the underlying data.
pub fn deserialize_zero_copy<T: for<'de> Deserialize<'de>>(bytes: &Bytes) -> Result<T> {
    bincode::deserialize(bytes).map_err(|e| RaftError::Internal {
        message: format!("Failed to deserialize from Bytes: {}", e),
        backtrace: Backtrace::new(),
    })
}

/// Fast path for common Raft message types
///
/// Uses postcard for smaller serialized size and faster (de)serialization
/// for frequently used message types like heartbeats.
pub fn serialize_raft_message_fast(msg: &raft::prelude::Message) -> Result<Bytes> {
    use prost::Message as ProstMessage;
    
    // Use prost directly for maximum efficiency
    let mut buf = BytesMut::with_capacity(msg.encoded_len());
    msg.encode(&mut buf).map_err(|e| RaftError::Serialization {
        message_type: "RaftMessage".to_string(),
        source: Box::new(e),
        backtrace: Backtrace::new(),
    })?;
    
    Ok(buf.freeze())
}

/// Fast deserialization for Raft messages using zero-copy when possible
pub fn deserialize_raft_message_fast(data: &[u8]) -> Result<raft::prelude::Message> {
    use prost::Message as ProstMessage;
    
    raft::prelude::Message::decode(data).map_err(|e| RaftError::Serialization {
        message_type: "RaftMessage".to_string(),
        source: Box::new(e),
        backtrace: Backtrace::new(),
    })
}

/// Write a message to a stream (legacy interface for compatibility)
pub async fn write_message<W>(
    stream: &mut W,
    msg_type: MessageType,
    request_id: Option<[u8; 16]>,
    payload: &[u8],
) -> Result<()> 
where
    W: AsyncWriteExt + Unpin,
{
    let msg = ZeroCopyMessage::new_borrowed(msg_type, request_id, payload);
    write_message_zero_copy(stream, &msg).await
}

/// Optimized zero-copy message write using pre-allocated buffers
///
/// This function minimizes allocations by reusing buffers and using
/// vectored writes when possible for better performance.
pub async fn write_message_zero_copy<W>(
    stream: &mut W,
    message: &ZeroCopyMessage<'_>,
) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    // Use a single buffer for the entire message for small messages
    if !message.is_large_message() {
        let mut buf = BytesMut::with_capacity(message.total_size as usize);
        
        // Serialize header
        let header = MessageHeader {
            msg_type: message.msg_type,
            request_id: message.request_id,
            payload_len: message.payload_len() as u32,
        };
        
        serialize_header_zero_copy(&header, &mut buf)?;
        let header_len = buf.len() as u32;
        
        // Prepare final buffer with header length prefix
        let mut final_buf = BytesMut::with_capacity(4 + buf.len() + message.payload_len());
        final_buf.put_u32(header_len);
        final_buf.extend_from_slice(&buf);
        final_buf.extend_from_slice(message.payload_bytes());
        
        // Single write call for efficiency
        stream
            .write_all(&final_buf)
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to write message: {}", e),
                backtrace: Backtrace::new(),
            })?;
    } else {
        // For large messages, use streaming approach
        write_large_message_streaming(stream, message).await?;
    }

    stream.flush().await.map_err(|e| RaftError::Internal {
        message: format!("Failed to flush stream: {}", e),
        backtrace: Backtrace::new(),
    })?;

    Ok(())
}

/// Stream large messages efficiently to avoid large buffer allocations
async fn write_large_message_streaming<W>(
    stream: &mut W,
    message: &ZeroCopyMessage<'_>,
) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    // Serialize header first
    let mut header_buf = BytesMut::with_capacity(MAX_HEADER_SIZE);
    let header = MessageHeader {
        msg_type: message.msg_type,
        request_id: message.request_id,
        payload_len: message.payload_len() as u32,
    };
    
    serialize_header_zero_copy(&header, &mut header_buf)?;
    let header_len = header_buf.len() as u32;

    // Write header length
    stream
        .write_all(&header_len.to_be_bytes())
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to write header length: {}", e),
            backtrace: Backtrace::new(),
        })?;

    // Write header
    stream
        .write_all(&header_buf)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to write header: {}", e),
            backtrace: Backtrace::new(),
        })?;

    // Stream payload in chunks for large messages
    const CHUNK_SIZE: usize = 32 * 1024; // 32KB chunks
    let payload = message.payload_bytes();
    
    for chunk in payload.chunks(CHUNK_SIZE) {
        stream
            .write_all(chunk)
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to write payload chunk: {}", e),
                backtrace: Backtrace::new(),
            })?;
    }

    Ok(())
}

/// Read a message from a stream (legacy interface for compatibility)
pub async fn read_message<R>(
    stream: &mut R,
) -> Result<(MessageHeader, Vec<u8>)> 
where
    R: AsyncReadExt + Unpin,
{
    let (header, bytes) = read_message_zero_copy(stream).await?;
    Ok((header, bytes.to_vec()))
}

/// Zero-copy message reading that returns Bytes instead of Vec<u8>
///
/// This function minimizes allocations by using Bytes for the payload,
/// allowing for efficient sharing of the underlying buffer.
pub async fn read_message_zero_copy<R>(
    stream: &mut R,
) -> Result<(MessageHeader, Bytes)>
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
    
    // Validate header length to prevent DoS attacks
    if header_len > MAX_HEADER_SIZE {
        return Err(RaftError::Internal {
            message: format!("Header too large: {} bytes (max {})", header_len, MAX_HEADER_SIZE),
            backtrace: Backtrace::new(),
        });
    }

    // Read header into BytesMut for efficient processing
    let mut header_buf = BytesMut::with_capacity(header_len);
    header_buf.resize(header_len, 0);
    stream
        .read_exact(&mut header_buf)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to read header: {}", e),
            backtrace: Backtrace::new(),
        })?;

    let header: MessageHeader = deserialize_payload(&header_buf)?;
    
    // Validate payload length
    if header.payload_len > (100 * 1024 * 1024) as u32 {  // 100MB max
        return Err(RaftError::Internal {
            message: format!("Payload too large: {} bytes", header.payload_len),
            backtrace: Backtrace::new(),
        });
    }

    // Choose reading strategy based on message size
    let payload = if header.payload_len as usize > LARGE_MESSAGE_THRESHOLD {
        read_large_payload_streaming(stream, header.payload_len as usize).await?
    } else {
        read_small_payload_direct(stream, header.payload_len as usize).await?
    };

    Ok((header, payload))
}

/// Read small payloads directly into a single buffer
async fn read_small_payload_direct<R>(
    stream: &mut R,
    payload_len: usize,
) -> Result<Bytes>
where
    R: AsyncReadExt + Unpin,
{
    let mut payload_buf = BytesMut::with_capacity(payload_len);
    payload_buf.resize(payload_len, 0);
    
    stream
        .read_exact(&mut payload_buf)
        .await
        .map_err(|e| RaftError::Internal {
            message: format!("Failed to read payload: {}", e),
            backtrace: Backtrace::new(),
        })?;

    Ok(payload_buf.freeze())
}

/// Read large payloads using streaming to avoid large buffer allocations
async fn read_large_payload_streaming<R>(
    stream: &mut R,
    payload_len: usize,
) -> Result<Bytes>
where
    R: AsyncReadExt + Unpin,
{
    let mut payload_buf = BytesMut::with_capacity(payload_len);
    let mut remaining = payload_len;
    
    const READ_CHUNK_SIZE: usize = 32 * 1024; // 32KB chunks
    
    while remaining > 0 {
        let chunk_size = remaining.min(READ_CHUNK_SIZE);
        let start_len = payload_buf.len();
        payload_buf.resize(start_len + chunk_size, 0);
        
        stream
            .read_exact(&mut payload_buf[start_len..start_len + chunk_size])
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to read payload chunk: {}", e),
                backtrace: Backtrace::new(),
            })?;
            
        remaining -= chunk_size;
    }

    Ok(payload_buf.freeze())
}

/// Read a zero-copy message directly into ZeroCopyMessage struct
pub async fn read_zero_copy_message<R>(
    stream: &mut R,
) -> Result<ZeroCopyMessage<'static>>
where
    R: AsyncReadExt + Unpin,
{
    let (header, payload_bytes) = read_message_zero_copy(stream).await?;
    
    let message = ZeroCopyMessage {
        msg_type: header.msg_type,
        request_id: header.request_id,
        payload: Cow::Owned(payload_bytes.to_vec()),
        total_size: 4 + payload_bytes.len() as u32, // Approximate
    };
    
    Ok(message)
}

/// High-performance utilities for common Raft operations
pub mod raft_utils {
    use super::*;

    /// Create an optimized Raft RPC request with zero-copy payload
    pub fn create_raft_rpc_request<'a>(
        raft_message: &raft::prelude::Message,
        request_id: Option<[u8; 16]>,
    ) -> Result<ZeroCopyMessage<'static>> {
        // Serialize Raft message using fast path
        let raft_payload = serialize_raft_message_fast(raft_message)?;
        
        // Create RPC request wrapper
        let rpc_request = RpcRequest {
            service: "raft".to_string(),
            method: "raft.message".to_string(),
            payload: raft_payload.to_vec(),
        };
        
        // Serialize the RPC request
        let mut request_buf = BytesMut::with_capacity(raft_payload.len() + 64);
        serialize_payload_zero_copy(&rpc_request, &mut request_buf)?;
        
        Ok(ZeroCopyMessage::new_owned(
            MessageType::Request,
            request_id,
            request_buf.freeze(),
        ))
    }

    /// Create a heartbeat message with minimal allocations
    #[inline]
    pub fn create_heartbeat_message() -> ZeroCopyMessage<'static> {
        ZeroCopyMessage::new_owned(
            MessageType::Heartbeat,
            None,
            Bytes::new(), // Empty payload for heartbeat
        )
    }

    /// Check if a message is a heartbeat (for fast-path processing)
    #[inline]
    pub fn is_heartbeat_message(message: &ZeroCopyMessage<'_>) -> bool {
        message.msg_type == MessageType::Heartbeat
    }

    /// Check if a message is likely to be large (for streaming decisions)
    #[inline]
    pub fn should_stream_message(message: &ZeroCopyMessage<'_>) -> bool {
        message.is_large_message()
    }
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
        // Wrap in timeout to prevent hanging on malformed streams
        let result = timeout(Duration::from_secs(30), async {
            // Use zero-copy message reading for better performance
            let message = read_zero_copy_message(recv).await?;
            
            Self::process_message_zero_copy(send, message, raft_rx_tx).await
        }).await;
        
        match result {
            Ok(Ok(())) => {
                // Successfully processed message, flush and finish send stream
                if let Err(e) = send.flush().await {
                    tracing::warn!("Failed to flush stream: {}", e);
                }
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!("Error processing message: {}", e);
                Err(e)
            }
            Err(_) => {
                tracing::warn!("Stream processing timed out");
                Err(RaftError::Internal {
                    message: "Stream processing timeout".to_string(),
                    backtrace: Backtrace::new(),
                })
            }
        }
    }
    
    /// Process a single message and send response (legacy - kept for compatibility)
    async fn process_message<S>(
        send: &mut S,
        header: MessageHeader,
        payload: Vec<u8>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        let message = ZeroCopyMessage {
            msg_type: header.msg_type,
            request_id: header.request_id,
            payload: Cow::Owned(payload),
            total_size: 0, // Not used in this path
        };
        
        Self::process_message_zero_copy(send, message, raft_rx_tx).await
    }

    /// Process a zero-copy message and send response with optimized performance
    async fn process_message_zero_copy<S>(
        send: &mut S,
        message: ZeroCopyMessage<'_>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        let result = match message.msg_type {
            MessageType::Request => {
                Self::handle_rpc_request_zero_copy(send, &message, raft_rx_tx).await
            }
            MessageType::RaftMessage => {
                Self::handle_direct_raft_message_zero_copy(&message, raft_rx_tx).await
            }
            _ => {
                tracing::warn!("Unexpected message type: {:?}", message.msg_type);
                Ok(())
            }
        };
        
        result
    }
    
    /// Handle RPC request and send response
    async fn handle_rpc_request<S>(
        send: &mut S,
        header: MessageHeader,
        payload: Vec<u8>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        let request: RpcRequest = deserialize_payload(&payload)?;
        
        let response = if request.service == "raft" && request.method == "raft.message" {
            // Deserialize Raft message
            let message: raft::prelude::Message =
                crate::raft::messages::deserialize_message(&request.payload)?;

            // Get the sender's node ID from the message
            let from = message.from;

            // Send to Raft manager
            if let Err(e) = raft_rx_tx.send((from, message)) {
                tracing::error!("Failed to send Raft message to manager: {}", e);
                RpcResponse {
                    success: false,
                    payload: None,
                    error: Some(format!("Failed to process message: {}", e)),
                }
            } else {
                RpcResponse {
                    success: true,
                    payload: None,
                    error: None,
                }
            }
        } else {
            // Unknown service/method
            RpcResponse {
                success: false,
                payload: None,
                error: Some(format!("Unknown service/method: {}/{}", request.service, request.method)),
            }
        };
        
        let response_bytes = serialize_payload(&response)?;
        write_message(
            send,
            MessageType::Response,
            header.request_id,
            &response_bytes,
        ).await?;
        
        Ok(())
    }
    
    /// Handle direct Raft message (legacy path)
    async fn handle_direct_raft_message(
        payload: Vec<u8>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()> {
        let message = crate::raft::messages::deserialize_message(&payload)?;
        let from = message.from;
        
        if let Err(e) = raft_rx_tx.send((from, message)) {
            tracing::error!("Failed to send Raft message to manager: {}", e);
        }
        
        Ok(())
    }

    /// Handle RPC request with zero-copy optimizations
    async fn handle_rpc_request_zero_copy<S>(
        send: &mut S,
        message: &ZeroCopyMessage<'_>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        let request: RpcRequest = deserialize_payload(message.payload_bytes())?;
        
        let response = if request.service == "raft" && request.method == "raft.message" {
            // Use fast Raft message deserialization
            let raft_message = deserialize_raft_message_fast(&request.payload)?;
            let from = raft_message.from;

            // Send to Raft manager
            if let Err(e) = raft_rx_tx.send((from, raft_message)) {
                tracing::error!("Failed to send Raft message to manager: {}", e);
                RpcResponse {
                    success: false,
                    payload: None,
                    error: Some(format!("Failed to process message: {}", e)),
                }
            } else {
                RpcResponse {
                    success: true,
                    payload: None,
                    error: None,
                }
            }
        } else {
            // Unknown service/method
            RpcResponse {
                success: false,
                payload: None,
                error: Some(format!("Unknown service/method: {}/{}", request.service, request.method)),
            }
        };
        
        // Use zero-copy response serialization
        let mut response_buf = BytesMut::with_capacity(256);
        serialize_payload_zero_copy(&response, &mut response_buf)?;
        
        let response_msg = ZeroCopyMessage::new_borrowed(
            MessageType::Response,
            message.request_id,
            &response_buf,
        );
        
        write_message_zero_copy(send, &response_msg).await
    }

    /// Handle direct Raft message with zero-copy optimizations
    async fn handle_direct_raft_message_zero_copy(
        message: &ZeroCopyMessage<'_>,
        raft_rx_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Result<()> {
        let raft_message = deserialize_raft_message_fast(message.payload_bytes())?;
        let from = raft_message.from;
        
        if let Err(e) = raft_rx_tx.send((from, raft_message)) {
            tracing::error!("Failed to send Raft message to manager: {}", e);
        }
        
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