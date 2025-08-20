//! Management protocol implementation over Iroh P2P
//!
//! This module implements the zero-copy management protocol handler that follows
//! the same patterns as the existing Raft protocol, providing efficient P2P
//! cluster management operations.

use bytes::{BufMut, Bytes, BytesMut};
use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};
use snafu::Backtrace;
use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;
use dashmap::DashMap;

use crate::error::{RaftError, Result};
use crate::transport::protocol::{
    generate_request_id, serialize_payload, deserialize_payload,
    serialize_payload_zero_copy, serialize_header_zero_copy,
    read_message_zero_copy, write_message_zero_copy,
    MessageHeader, MessageType, ZeroCopyMessage,
    MAX_HEADER_SIZE, LARGE_MESSAGE_THRESHOLD
};

use super::{
    ManagementMessageType, ManagementOperation, ManagementResponse,
    ManagementRequest, ManagementPermissions, ManagementRole,
    ClusterEvent, MANAGEMENT_ALPN
};

/// Management message header extending the base protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementMessageHeader {
    /// Base message header
    pub base: MessageHeader,
    /// Management-specific operation type
    pub operation_type: Option<String>,
    /// Authorization token or node ID
    pub authorization: Option<String>,
}

/// Zero-copy management message for efficient processing
#[derive(Debug, Clone)]
pub struct ZeroCopyManagementMessage<'a> {
    pub msg_type: ManagementMessageType,
    pub request_id: Option<[u8; 16]>,
    pub operation: Option<ManagementOperation>,
    pub payload: Cow<'a, [u8]>,
    pub total_size: u32,
    pub authorization: Option<String>,
}

impl<'a> ZeroCopyManagementMessage<'a> {
    /// Create a new zero-copy management message with borrowed payload
    #[inline]
    pub fn new_borrowed(
        msg_type: ManagementMessageType,
        request_id: Option<[u8; 16]>,
        payload: &'a [u8],
        authorization: Option<String>,
    ) -> Self {
        let total_size = Self::calculate_size(payload.len());
        Self {
            msg_type,
            request_id,
            operation: None,
            payload: Cow::Borrowed(payload),
            total_size,
            authorization,
        }
    }

    /// Create a new zero-copy management message with owned payload
    #[inline]
    pub fn new_owned(
        msg_type: ManagementMessageType,
        request_id: Option<[u8; 16]>,
        payload: Bytes,
        authorization: Option<String>,
    ) -> Self {
        let total_size = Self::calculate_size(payload.len());
        Self {
            msg_type,
            request_id,
            operation: None,
            payload: Cow::Owned(payload.to_vec()),
            total_size,
            authorization,
        }
    }

    /// Create a management request message
    pub fn new_request(
        operation: ManagementOperation,
        request_id: Option<[u8; 16]>,
        authorization: Option<String>,
    ) -> Result<Self> {
        let request = ManagementRequest {
            operation: operation.clone(),
            timeout: Some(Duration::from_secs(30)),
            authorization: authorization.clone(),
        };
        
        let payload = serialize_payload(&request)?;
        let payload_len = payload.len();
        
        Ok(Self {
            msg_type: ManagementMessageType::Request,
            request_id,
            operation: Some(operation),
            payload: Cow::Owned(payload),
            total_size: Self::calculate_size(payload_len),
            authorization,
        })
    }

    /// Calculate the total message size including headers
    #[inline]
    const fn calculate_size(payload_len: usize) -> u32 {
        // Conservative estimate: 4 bytes length + header + payload
        4 + 64 + payload_len as u32
    }

    /// Convert to owned message for storage
    pub fn into_owned(self) -> ZeroCopyManagementMessage<'static> {
        ZeroCopyManagementMessage {
            msg_type: self.msg_type,
            request_id: self.request_id,
            operation: self.operation,
            payload: Cow::Owned(self.payload.into_owned()),
            total_size: self.total_size,
            authorization: self.authorization,
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

/// Management RPC request with zero-copy payload
#[derive(Debug, Clone)]
pub struct ZeroCopyManagementRequest<'a> {
    pub operation: ManagementOperation,
    pub payload: Cow<'a, [u8]>,
    pub timeout: Option<Duration>,
    pub authorization: Option<String>,
}

impl<'a> ZeroCopyManagementRequest<'a> {
    /// Create new zero-copy management request
    #[inline]
    pub fn new(
        operation: ManagementOperation,
        payload: &'a [u8],
        timeout: Option<Duration>,
        authorization: Option<String>,
    ) -> Self {
        Self {
            operation,
            payload: Cow::Borrowed(payload),
            timeout,
            authorization,
        }
    }

    /// Convert to the standard ManagementRequest for processing
    pub fn to_standard(&self) -> ManagementRequest {
        ManagementRequest {
            operation: self.operation.clone(),
            timeout: self.timeout,
            authorization: self.authorization.clone(),
        }
    }
}

/// Management protocol handler for Iroh P2P
#[derive(Debug)]
pub struct ManagementProtocolHandler {
    /// Channel to send management requests to the cluster manager
    management_tx: mpsc::UnboundedSender<(ManagementRequest, oneshot::Sender<ManagementResponse>)>,
    /// Our node ID
    node_id: u64,
    /// Event subscribers for real-time notifications
    event_subscribers: Arc<DashMap<[u8; 16], mpsc::UnboundedSender<super::ClusterEvent>>>,
    /// Access control permissions
    permissions: Arc<RwLock<DashMap<u64, ManagementPermissions>>>,
}

impl ManagementProtocolHandler {
    /// Create a new management protocol handler
    pub fn new(
        node_id: u64,
        management_tx: mpsc::UnboundedSender<(ManagementRequest, oneshot::Sender<ManagementResponse>)>,
    ) -> Self {
        Self {
            management_tx,
            node_id,
            event_subscribers: Arc::new(DashMap::new()),
            permissions: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    /// Add permissions for a node
    pub async fn add_permissions(&self, permissions: ManagementPermissions) {
        let perms = self.permissions.read().await;
        perms.insert(permissions.node_id, permissions);
    }

    /// Check if a node has permission for an operation
    async fn check_permission(&self, node_id: u64, operation: &ManagementOperation) -> bool {
        let perms = self.permissions.read().await;
        
        let result = if let Some(permissions) = perms.get(&node_id) {
            // Check role-based permissions
            match permissions.role {
                ManagementRole::Admin => true,
                ManagementRole::Observer => operation.is_read_only(),
                ManagementRole::MemberManager => {
                    operation.is_read_only() || matches!(operation,
                        ManagementOperation::AddMember { .. } |
                        ManagementOperation::RemoveMember { .. } |
                        ManagementOperation::UpdateMemberRole { .. }
                    )
                },
                ManagementRole::Monitor => matches!(operation,
                    ManagementOperation::GetClusterStatus |
                    ManagementOperation::GetHealthStatus |
                    ManagementOperation::GetMetrics { .. }
                ),
            }
        } else {
            // Default to observer permissions for unknown nodes
            operation.is_read_only()
        };
        
        result
    }

    /// Handle an incoming management connection
    pub async fn handle_connection(&self, connection: Connection) -> Result<()> {
        tracing::debug!("New management connection from: {:?}", connection.remote_node_id());
        
        loop {
            match connection.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    let tx = self.management_tx.clone();
                    let node_id = self.node_id;
                    let subscribers = self.event_subscribers.clone();
                    let permissions = self.permissions.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_management_stream(
                            &mut send, &mut recv, tx, node_id, subscribers, permissions
                        ).await {
                            tracing::warn!("Management stream error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::debug!("Management connection closed: {}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }

    /// Handle a bidirectional management stream
    async fn handle_management_stream<S, R>(
        send: &mut S,
        recv: &mut R,
        management_tx: mpsc::UnboundedSender<(ManagementRequest, oneshot::Sender<ManagementResponse>)>,
        _node_id: u64,
        event_subscribers: Arc<DashMap<[u8; 16], mpsc::UnboundedSender<super::ClusterEvent>>>,
        permissions: Arc<RwLock<DashMap<u64, ManagementPermissions>>>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        // Set timeout for management operations
        let result = timeout(Duration::from_secs(60), async {
            // Read the management message
            let (header, payload_bytes) = read_message_zero_copy(recv).await?;
            
            // Convert to management message
            let mgmt_msg = ZeroCopyManagementMessage {
                msg_type: match header.msg_type {
                    MessageType::Request => ManagementMessageType::Request,
                    MessageType::Response => ManagementMessageType::Response,
                    MessageType::Error => ManagementMessageType::Error,
                    MessageType::Heartbeat => ManagementMessageType::HealthCheck,
                    _ => ManagementMessageType::Error,
                },
                request_id: header.request_id,
                operation: None,
                payload: Cow::Owned(payload_bytes.to_vec()),
                total_size: header.payload_len + 4,
                authorization: None,
            };
            
            Self::process_management_message(
                send, mgmt_msg, management_tx, event_subscribers, permissions
            ).await
        }).await;
        
        match result {
            Ok(Ok(())) => {
                if let Err(e) = send.flush().await {
                    tracing::warn!("Failed to flush management stream: {}", e);
                }
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!("Error processing management message: {}", e);
                Err(e)
            }
            Err(_) => {
                tracing::warn!("Management stream processing timed out");
                Err(RaftError::Internal {
                    message: "Management stream timeout".to_string(),
                    backtrace: Backtrace::new(),
                })
            }
        }
    }

    /// Process a management message and send response
    async fn process_management_message<S>(
        send: &mut S,
        message: ZeroCopyManagementMessage<'_>,
        management_tx: mpsc::UnboundedSender<(ManagementRequest, oneshot::Sender<ManagementResponse>)>,
        event_subscribers: Arc<DashMap<[u8; 16], mpsc::UnboundedSender<super::ClusterEvent>>>,
        permissions: Arc<RwLock<DashMap<u64, ManagementPermissions>>>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        let result = match message.msg_type {
            ManagementMessageType::Request => {
                Self::handle_management_request(send, &message, management_tx, permissions).await
            }
            ManagementMessageType::Event => {
                Self::handle_event_subscription(&message, event_subscribers).await
            }
            ManagementMessageType::HealthCheck => {
                Self::handle_health_check(send, &message).await
            }
            _ => {
                tracing::warn!("Unexpected management message type: {:?}", message.msg_type);
                Ok(())
            }
        };
        
        result
    }

    /// Handle a management RPC request
    async fn handle_management_request<S>(
        send: &mut S,
        message: &ZeroCopyManagementMessage<'_>,
        management_tx: mpsc::UnboundedSender<(ManagementRequest, oneshot::Sender<ManagementResponse>)>,
        _permissions: Arc<RwLock<DashMap<u64, ManagementPermissions>>>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        // Deserialize the management request
        let request: ManagementRequest = deserialize_payload(message.payload_bytes())?;
        
        // Create a response channel
        let (response_tx, response_rx) = oneshot::channel();
        
        // Send the request to the cluster manager
        if let Err(_) = management_tx.send((request, response_tx)) {
            let error_response = ManagementResponse::Error {
                message: "Cluster manager unavailable".to_string(),
            };
            return Self::send_management_response(send, &error_response, message.request_id).await;
        }
        
        // Wait for the response
        match response_rx.await {
            Ok(response) => {
                Self::send_management_response(send, &response, message.request_id).await
            }
            Err(_) => {
                let error_response = ManagementResponse::Error {
                    message: "Response channel closed".to_string(),
                };
                Self::send_management_response(send, &error_response, message.request_id).await
            }
        }
    }

    /// Send a management response using zero-copy optimizations
    async fn send_management_response<S>(
        send: &mut S,
        response: &ManagementResponse,
        request_id: Option<[u8; 16]>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        // Serialize the response
        let mut response_buf = BytesMut::with_capacity(1024);
        serialize_payload_zero_copy(response, &mut response_buf)?;
        
        // Create the response message
        let response_msg = ZeroCopyMessage::new_borrowed(
            crate::transport::protocol::MessageType::Response,
            request_id,
            &response_buf,
        );
        
        // Send the response
        write_message_zero_copy(send, &response_msg).await
    }

    /// Handle event subscription
    async fn handle_event_subscription(
        message: &ZeroCopyManagementMessage<'_>,
        event_subscribers: Arc<DashMap<[u8; 16], mpsc::UnboundedSender<super::ClusterEvent>>>,
    ) -> Result<()> {
        if let Some(subscription_id) = message.request_id {
            // In a real implementation, we'd set up the event channel here
            // For now, just acknowledge the subscription
            tracing::info!("Event subscription registered: {:?}", subscription_id);
            
            // Create a placeholder channel (real implementation would connect to cluster events)
            let (_tx, _rx) = mpsc::unbounded_channel::<ClusterEvent>();
            // event_subscribers.insert(subscription_id, tx);
        }
        
        Ok(())
    }

    /// Handle health check ping
    async fn handle_health_check<S>(
        send: &mut S,
        message: &ZeroCopyManagementMessage<'_>,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
    {
        // Simple health check response
        let health_response = ManagementResponse::Success;
        Self::send_management_response(send, &health_response, message.request_id).await
    }

    /// Subscribe to cluster events (for external use)
    pub fn subscribe_to_events(&self, subscription_id: [u8; 16]) -> mpsc::UnboundedReceiver<super::ClusterEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_subscribers.insert(subscription_id, tx);
        rx
    }

    /// Publish an event to all subscribers
    pub async fn publish_event(&self, event: super::ClusterEvent) {
        let mut closed_subscriptions = Vec::new();
        
        for entry in self.event_subscribers.iter() {
            let subscription_id = *entry.key();
            let sender = entry.value();
            
            if let Err(_) = sender.send(event.clone()) {
                // Subscriber channel is closed, mark for removal
                closed_subscriptions.push(subscription_id);
            }
        }
        
        // Clean up closed subscriptions
        for subscription_id in closed_subscriptions {
            self.event_subscribers.remove(&subscription_id);
        }
    }
}

/// Implement the accept method for handling connections (matching Raft protocol pattern)
impl ManagementProtocolHandler {
    /// Accept and handle an incoming management connection
    pub fn accept(
        &self,
        connection: Connection,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
        let handler = self.clone();
        Box::pin(async move {
            handler
                .handle_connection(connection)
                .await
                .map_err(|e| anyhow::anyhow!("Management protocol handler error: {}", e))
        })
    }
}

impl Clone for ManagementProtocolHandler {
    fn clone(&self) -> Self {
        Self {
            management_tx: self.management_tx.clone(),
            node_id: self.node_id,
            event_subscribers: self.event_subscribers.clone(),
            permissions: self.permissions.clone(),
        }
    }
}

/// Utility functions for management message handling
pub mod mgmt_utils {
    use super::*;

    /// Create a management request message with zero-copy payload
    pub fn create_management_request(
        operation: ManagementOperation,
        authorization: Option<String>,
    ) -> Result<ZeroCopyManagementMessage<'static>> {
        let request_id = Some(generate_request_id());
        ZeroCopyManagementMessage::new_request(operation, request_id, authorization)
    }

    /// Create a health check message
    #[inline]
    pub fn create_health_check_message() -> ZeroCopyManagementMessage<'static> {
        ZeroCopyManagementMessage::new_owned(
            ManagementMessageType::HealthCheck,
            Some(generate_request_id()),
            Bytes::new(),
            None,
        )
    }

    /// Check if a message should use streaming (for large responses)
    #[inline]
    pub fn should_stream_message(message: &ZeroCopyManagementMessage<'_>) -> bool {
        message.is_large_message()
    }
}

// Conversion helpers for message types
impl From<ManagementMessageType> for u8 {
    fn from(msg_type: ManagementMessageType) -> u8 {
        match msg_type {
            ManagementMessageType::Request => 0,
            ManagementMessageType::Response => 1,
            ManagementMessageType::Event => 2,
            ManagementMessageType::HealthCheck => 3,
            ManagementMessageType::Error => 4,
        }
    }
}

impl From<u8> for ManagementMessageType {
    fn from(value: u8) -> Self {
        match value {
            0 => ManagementMessageType::Request,
            1 => ManagementMessageType::Response,
            2 => ManagementMessageType::Event,
            3 => ManagementMessageType::HealthCheck,
            _ => ManagementMessageType::Error,
        }
    }
}