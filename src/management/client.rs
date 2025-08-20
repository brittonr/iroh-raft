//! High-level Iroh-native management client
//!
//! This module provides a convenient client interface for managing iroh-raft
//! clusters using P2P protocols instead of HTTP APIs.

use bytes::Bytes;
use iroh::endpoint::{Endpoint, Connection};
use iroh::{NodeAddr, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;
use futures_util::StreamExt;

use crate::error::{RaftError, Result};
use crate::transport::protocol::{
    generate_request_id, read_message_zero_copy, write_message_zero_copy,
    ZeroCopyMessage, MessageType,
};

use super::{
    ManagementOperation, ManagementResponse, ManagementRequest,
    ClusterStatusResponse, MemberOperationResponse, LeadershipResponse,
    ConfigurationResponse, MetricsResponse, HealthResponse,
    LogInfoResponse, LogEntriesResponse, SnapshotInfoResponse,
    BackupOperationResponse, BackupListResponse,
    MemberRole, MetricCategory, BackupMetadata, ClusterConfig,
    MANAGEMENT_ALPN
};

use super::protocol::{ZeroCopyManagementMessage, mgmt_utils};
use super::ManagementMessageType;
use super::discovery::ClusterDiscovery;
use super::events::EventStream;

/// Connection pool for managing connections to cluster members
#[derive(Debug)]
pub struct ConnectionPool {
    connections: Arc<RwLock<HashMap<u64, Connection>>>,
    endpoints: Arc<RwLock<HashMap<u64, NodeAddr>>>,
    iroh_endpoint: Endpoint,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(iroh_endpoint: Endpoint) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            iroh_endpoint,
        }
    }

    /// Add a cluster member endpoint
    pub async fn add_member(&self, node_id: u64, node_addr: NodeAddr) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.insert(node_id, node_addr);
    }

    /// Get or create a connection to a cluster member
    pub async fn get_connection(&self, node_id: u64) -> Result<Connection> {
        // Check if we already have a connection
        {
            let connections = self.connections.read().await;
            if let Some(connection) = connections.get(&node_id) {
                // TODO: Check if connection is still alive
                return Ok(connection.clone());
            }
        }

        // Get the endpoint for this node
        let node_addr = {
            let endpoints = self.endpoints.read().await;
            endpoints.get(&node_id).cloned()
                .ok_or_else(|| RaftError::Internal {
                    message: format!("No endpoint found for node {}", node_id),
                    backtrace: snafu::Backtrace::new(),
                })?
        };

        // Create new connection
        let connection = self.iroh_endpoint
            .connect(node_addr, MANAGEMENT_ALPN)
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to connect to node {}: {}", node_id, e),
                backtrace: snafu::Backtrace::new(),
            })?;

        // Store the connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(node_id, connection.clone());
        }

        Ok(connection)
    }

    /// Remove a connection (on error or disconnect)
    pub async fn remove_connection(&self, node_id: u64) {
        let mut connections = self.connections.write().await;
        connections.remove(&node_id);
    }

    /// Get a list of available nodes
    pub async fn get_available_nodes(&self) -> Vec<u64> {
        let endpoints = self.endpoints.read().await;
        endpoints.keys().copied().collect()
    }
}

/// High-level client for iroh-raft management operations
#[derive(Debug)]
pub struct IrohRaftManagementClient {
    connection_pool: ConnectionPool,
    cluster_discovery: Option<ClusterDiscovery>,
    default_timeout: Duration,
    current_leader: Arc<RwLock<Option<u64>>>,
}

impl IrohRaftManagementClient {
    /// Create a new management client with an existing Iroh endpoint
    pub fn new(iroh_endpoint: Endpoint) -> Self {
        Self {
            connection_pool: ConnectionPool::new(iroh_endpoint),
            cluster_discovery: None,
            default_timeout: Duration::from_secs(30),
            current_leader: Arc::new(RwLock::new(None)),
        }
    }

    /// Connect to a cluster using service discovery
    pub async fn connect_to_cluster(cluster_id: &str) -> Result<Self> {
        let discovery = ClusterDiscovery::new().await?;
        let cluster_info = discovery.discover_cluster(cluster_id).await?;
        
        let iroh_endpoint = Endpoint::builder()
            .bind()
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to create Iroh endpoint: {}", e),
                backtrace: snafu::Backtrace::new(),
            })?;

        let mut client = Self::new(iroh_endpoint);
        client.cluster_discovery = Some(discovery);

        // Add cluster members to connection pool
        for member in &cluster_info.members {
            // Parse the address to create NodeAddr
            // This is simplified - in a real implementation, you'd have proper address resolution
            if let Ok(socket_addr) = member.address.parse::<SocketAddr>() {
                // Create a 32-byte array from the node_id (u64)
                let mut node_id_bytes = [0u8; 32];
                node_id_bytes[..8].copy_from_slice(&member.node_id.to_be_bytes());
                let node_id = NodeId::from_bytes(&node_id_bytes)
                    .map_err(|e| RaftError::ConnectionFailed {
                        peer_id: member.node_id.to_string(),
                        reason: format!("Invalid node ID: {}", e),
                        backtrace: snafu::Backtrace::new(),
                    })?;
                let node_addr = NodeAddr::new(node_id).with_direct_addresses([socket_addr]);
                client.connection_pool.add_member(member.node_id, node_addr).await;
            }
        }

        // Set the current leader if known
        if let Some(leader_id) = cluster_info.leader {
            *client.current_leader.write().await = Some(leader_id);
        }

        Ok(client)
    }

    /// Connect to specific cluster members
    pub async fn connect_to_members(members: Vec<(u64, NodeAddr)>) -> Result<Self> {
        let iroh_endpoint = Endpoint::builder()
            .bind()
            .await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to create Iroh endpoint: {}", e),
                backtrace: snafu::Backtrace::new(),
            })?;

        let client = Self::new(iroh_endpoint);

        // Add members to connection pool
        for (node_id, node_addr) in members {
            client.connection_pool.add_member(node_id, node_addr).await;
        }

        Ok(client)
    }

    /// Set the default timeout for operations
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Send a management request to any available cluster member
    async fn send_request(
        &self,
        operation: ManagementOperation,
        timeout: Option<Duration>,
    ) -> Result<ManagementResponse> {
        let timeout = timeout.unwrap_or(self.default_timeout);
        
        // Try to send to the current leader first
        if let Some(leader_id) = *self.current_leader.read().await {
            match self.send_request_to_node(leader_id, operation.clone(), timeout).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    tracing::warn!("Failed to send request to leader {}: {}", leader_id, e);
                    // Clear the cached leader and try other nodes
                    *self.current_leader.write().await = None;
                }
            }
        }

        // Try other available nodes
        let available_nodes = self.connection_pool.get_available_nodes().await;
        for node_id in available_nodes {
            match self.send_request_to_node(node_id, operation.clone(), timeout).await {
                Ok(response) => {
                    // If this was a successful leadership operation, update our leader cache
                    if let ManagementResponse::ClusterStatus(ref status) = response {
                        if let Some(leader_id) = status.leader {
                            *self.current_leader.write().await = Some(leader_id);
                        }
                    }
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!("Failed to send request to node {}: {}", node_id, e);
                    self.connection_pool.remove_connection(node_id).await;
                }
            }
        }

        Err(RaftError::Internal {
            message: "No available cluster members".to_string(),
            backtrace: snafu::Backtrace::new(),
        })
    }

    /// Send a management request to a specific node
    async fn send_request_to_node(
        &self,
        node_id: u64,
        operation: ManagementOperation,
        timeout: Duration,
    ) -> Result<ManagementResponse> {
        let connection = self.connection_pool.get_connection(node_id).await?;
        
        // Open a bidirectional stream for the request
        let (mut send, mut recv) = connection.open_bi().await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to open stream to node {}: {}", node_id, e),
                backtrace: snafu::Backtrace::new(),
            })?;

        // Create the management request message
        let request_msg = mgmt_utils::create_management_request(operation, None)?;

        // Send the request with timeout
        let result = tokio::time::timeout(timeout, async {
            // Convert to base protocol message format
            let base_msg = ZeroCopyMessage::new_borrowed(
                MessageType::Request,
                request_msg.request_id,
                request_msg.payload_bytes(),
            );

            // Send the request
            write_message_zero_copy(&mut send, &base_msg).await?;
            send.finish().map_err(|e| RaftError::ConnectionFailed {
                peer_id: "remote".to_string(),
                reason: e.to_string(),
                backtrace: snafu::Backtrace::new(),
            })?;

            // Read the response
            let (header, payload) = read_message_zero_copy(&mut recv).await?;
            
            // Deserialize the response
            let response: ManagementResponse = crate::transport::protocol::deserialize_payload(&payload)?;
            
            Ok::<ManagementResponse, RaftError>(response)
        }).await;

        match result {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(RaftError::Internal {
                message: format!("Request to node {} timed out", node_id),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    // === Public API Methods ===

    /// Get cluster status from any available member
    pub async fn get_cluster_status(&self) -> Result<ClusterStatusResponse> {
        let response = self.send_request(ManagementOperation::GetClusterStatus, None).await?;
        match response {
            ManagementResponse::ClusterStatus(status) => Ok(status),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Add a new member to the cluster
    pub async fn add_member(
        &self,
        node_id: u64,
        address: String,
        role: MemberRole,
    ) -> Result<MemberOperationResponse> {
        let operation = ManagementOperation::AddMember { node_id, address, role };
        let response = self.send_request(operation, None).await?;
        
        match response {
            ManagementResponse::MemberOperation(result) => Ok(result),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Remove a member from the cluster
    pub async fn remove_member(&self, node_id: u64) -> Result<MemberOperationResponse> {
        let operation = ManagementOperation::RemoveMember { node_id };
        let response = self.send_request(operation, None).await?;
        
        match response {
            ManagementResponse::MemberOperation(result) => Ok(result),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Transfer leadership to a specific node or let Raft choose
    pub async fn transfer_leadership(
        &self,
        target_node: Option<u64>,
    ) -> Result<LeadershipResponse> {
        let operation = ManagementOperation::TransferLeadership { target_node };
        let response = self.send_request(operation, Some(Duration::from_secs(60))).await?;
        
        match response {
            ManagementResponse::Leadership(result) => Ok(result),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Get cluster configuration
    pub async fn get_configuration(&self) -> Result<ConfigurationResponse> {
        let response = self.send_request(ManagementOperation::GetConfiguration, None).await?;
        
        match response {
            ManagementResponse::Configuration(config) => Ok(config),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Update cluster configuration
    pub async fn update_configuration(&self, config: ClusterConfig) -> Result<()> {
        let operation = ManagementOperation::UpdateConfiguration { config };
        let response = self.send_request(operation, None).await?;
        
        match response {
            ManagementResponse::Success => Ok(()),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Get metrics for specified categories
    pub async fn get_metrics(&self, categories: Vec<MetricCategory>) -> Result<MetricsResponse> {
        let operation = ManagementOperation::GetMetrics { categories };
        let response = self.send_request(operation, None).await?;
        
        match response {
            ManagementResponse::Metrics(metrics) => Ok(metrics),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Get health status of the cluster
    pub async fn get_health_status(&self) -> Result<HealthResponse> {
        let response = self.send_request(ManagementOperation::GetHealthStatus, None).await?;
        
        match response {
            ManagementResponse::Health(health) => Ok(health),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Trigger a snapshot
    pub async fn trigger_snapshot(&self) -> Result<()> {
        let response = self.send_request(ManagementOperation::TriggerSnapshot, None).await?;
        
        match response {
            ManagementResponse::Success => Ok(()),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Create a backup
    pub async fn create_backup(&self, metadata: BackupMetadata) -> Result<BackupOperationResponse> {
        let operation = ManagementOperation::CreateBackup { metadata };
        let response = self.send_request(operation, Some(Duration::from_secs(300))).await?;
        
        match response {
            ManagementResponse::BackupOperation(result) => Ok(result),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// List available backups
    pub async fn list_backups(&self) -> Result<BackupListResponse> {
        let response = self.send_request(ManagementOperation::ListBackups, None).await?;
        
        match response {
            ManagementResponse::BackupList(list) => Ok(list),
            ManagementResponse::Error { message } => Err(RaftError::Internal {
                message,
                backtrace: snafu::Backtrace::new(),
            }),
            _ => Err(RaftError::Internal {
                message: "Unexpected response type".to_string(),
                backtrace: snafu::Backtrace::new(),
            }),
        }
    }

    /// Subscribe to cluster events
    pub async fn subscribe_to_events(&self) -> Result<EventStream> {
        // For event streaming, we'll use a different approach with long-lived connections
        // This is a simplified implementation - real implementation would manage
        // persistent event streams
        let (tx, rx) = mpsc::unbounded_channel();
        let subscription_id = generate_request_id();
        
        // TODO: Set up actual event subscription with cluster
        tracing::info!("Event subscription created: {:?}", subscription_id);
        
        Ok(EventStream::new(rx, subscription_id))
    }

    /// Check connectivity to a specific node
    pub async fn ping_node(&self, node_id: u64) -> Result<Duration> {
        let start = std::time::Instant::now();
        
        // Send a health check message
        let connection = self.connection_pool.get_connection(node_id).await?;
        let (mut send, mut recv) = connection.open_bi().await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to open ping stream: {}", e),
                backtrace: snafu::Backtrace::new(),
            })?;

        let ping_msg = mgmt_utils::create_health_check_message();
        let base_msg = ZeroCopyMessage::new_borrowed(
            MessageType::Request,
            ping_msg.request_id,
            ping_msg.payload_bytes(),
        );

        write_message_zero_copy(&mut send, &base_msg).await?;
        send.finish().map_err(|e| RaftError::ConnectionFailed {
                peer_id: "remote".to_string(),
            reason: e.to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        let (_header, _payload) = read_message_zero_copy(&mut recv).await?;
        
        Ok(start.elapsed())
    }
}