//! Iroh-native management protocol for cluster administration
//!
//! This module provides P2P management operations over QUIC streams, following
//! Iroh's native patterns for efficient, secure cluster management without
//! requiring HTTP servers or external dependencies.
//!
//! Features include:
//! - P2P cluster management using QUIC streams with custom ALPN protocols
//! - Zero-copy message handling with Cow<[u8]> and Bytes
//! - Built-in service discovery and cluster formation
//! - Real-time event streaming over P2P connections
//! - Role-based access control using Iroh's authentication
//! - Automatic failover and connection pooling
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use iroh_raft::management::IrohRaftManagementClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to cluster using discovery
//!     let client = IrohRaftManagementClient::connect_to_cluster("my-cluster").await?;
//!     
//!     // Get cluster status
//!     let status = client.get_cluster_status().await?;
//!     println!("Cluster leader: {:?}", status.leader);
//!     
//!     // Add a new member
//!     let result = client.add_member(
//!         4,
//!         "new-node.example.com:8080".to_string(),
//!         MemberRole::Voter,
//!     ).await?;
//!     
//!     // Subscribe to real-time events
//!     let mut events = client.subscribe_to_events().await?;
//!     while let Some(event) = events.next().await {
//!         println!("Event: {:?}", event);
//!     }
//!     
//!     Ok(())
//! }
//! ```

pub mod protocol;
pub mod client;
pub mod discovery;
pub mod events;
pub mod auth;

// Legacy HTTP compatibility (optional)
#[cfg(feature = "http-management-compat")]
pub mod http_compat;

pub use protocol::*;
pub use client::*;
pub use discovery::*;
pub use events::*;
pub use auth::*;

use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::error::Result;

/// ALPN protocol identifier for iroh-raft management
pub const MANAGEMENT_ALPN: &[u8] = b"iroh-raft-mgmt/0";

/// ALPN protocol identifier for iroh-raft discovery  
pub const DISCOVERY_ALPN: &[u8] = b"iroh-raft-discovery/0";

/// ALPN protocol identifier for iroh-raft metrics
pub const METRICS_ALPN: &[u8] = b"iroh-raft-metrics/0";

/// Management message types following Raft protocol patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManagementMessageType {
    /// Management RPC request
    Request,
    /// Management RPC response
    Response,
    /// Event notification (fire-and-forget)
    Event,
    /// Health check ping
    HealthCheck,
    /// Error response
    Error,
}

/// Member roles in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberRole {
    /// Voting member that participates in consensus
    Voter,
    /// Learner that receives log entries but doesn't vote
    Learner,
    /// Observer that only receives health and status updates
    Observer,
}

/// Cluster member information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    pub node_id: u64,
    pub address: String,
    pub role: MemberRole,
    pub health: HealthStatus,
    pub last_seen: std::time::SystemTime,
    pub version: String,
}

/// Health status of a cluster member
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Raft state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftState {
    pub term: u64,
    pub vote: Option<u64>,
    pub leader: Option<u64>,
    pub commit_index: u64,
    pub last_applied: u64,
    pub log_length: u64,
}

/// Overall cluster health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHealthStatus {
    pub status: HealthStatus,
    pub healthy_members: u32,
    pub total_members: u32,
    pub quorum_available: bool,
    pub leader_available: bool,
}

/// Cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub cluster_id: String,
    pub heartbeat_interval: Duration,
    pub election_timeout: Duration,
    pub max_log_entries: u64,
    pub snapshot_threshold: u64,
    pub compaction_threshold: u64,
}

/// Metric categories for monitoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricCategory {
    Raft,
    Transport,
    Storage,
    Performance,
    System,
}

/// Backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub backup_id: String,
    pub description: String,
    pub created_at: std::time::SystemTime,
    pub cluster_state: RaftState,
}

/// Management operations that can be performed on the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementOperation {
    // Cluster Management
    GetClusterStatus,
    AddMember { 
        node_id: u64, 
        address: String, 
        role: MemberRole 
    },
    RemoveMember { 
        node_id: u64 
    },
    UpdateMemberRole { 
        node_id: u64, 
        role: MemberRole 
    },
    
    // Leadership Operations
    TransferLeadership { 
        target_node: Option<u64> 
    },
    StepDown,
    
    // Configuration Management
    GetConfiguration,
    UpdateConfiguration { 
        config: ClusterConfig 
    },
    
    // Metrics & Monitoring
    GetMetrics { 
        categories: Vec<MetricCategory> 
    },
    GetHealthStatus,
    
    // Log Management
    GetLogInfo,
    CompactLog { 
        index: u64 
    },
    GetLogEntries { 
        start: u64, 
        end: u64 
    },
    
    // Snapshot Operations
    TriggerSnapshot,
    GetSnapshotInfo,
    
    // Backup & Restore
    CreateBackup { 
        metadata: BackupMetadata 
    },
    RestoreBackup { 
        backup_id: String 
    },
    ListBackups,
}

/// Response types for management operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementResponse {
    ClusterStatus(ClusterStatusResponse),
    MemberOperation(MemberOperationResponse),
    Leadership(LeadershipResponse),
    Configuration(ConfigurationResponse),
    Metrics(MetricsResponse),
    Health(HealthResponse),
    LogInfo(LogInfoResponse),
    LogEntries(LogEntriesResponse),
    SnapshotInfo(SnapshotInfoResponse),
    BackupOperation(BackupOperationResponse),
    BackupList(BackupListResponse),
    Success,
    Error { message: String },
}

/// Cluster status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatusResponse {
    pub cluster_id: String,
    pub leader: Option<u64>,
    pub term: u64,
    pub members: Vec<ClusterMember>,
    pub health: ClusterHealthStatus,
    pub raft_state: RaftState,
}

/// Member operation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberOperationResponse {
    pub success: bool,
    pub member_id: Option<u64>,
    pub error: Option<String>,
}

/// Leadership operation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeadershipResponse {
    pub success: bool,
    pub new_leader: Option<u64>,
    pub transfer_duration: Option<Duration>,
    pub error: Option<String>,
}

/// Configuration response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationResponse {
    pub config: ClusterConfig,
    pub version: u64,
}

/// Metrics response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub metrics: std::collections::HashMap<String, f64>,
    pub timestamp: std::time::SystemTime,
}

/// Health status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub overall_health: HealthStatus,
    pub cluster_health: ClusterHealthStatus,
    pub node_health: std::collections::HashMap<u64, HealthStatus>,
}

/// Log information response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogInfoResponse {
    pub first_index: u64,
    pub last_index: u64,
    pub committed_index: u64,
    pub total_entries: u64,
    pub total_size_bytes: u64,
}

/// Log entries response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntriesResponse {
    pub entries: Vec<LogEntry>,
    pub start_index: u64,
    pub end_index: u64,
}

/// Log entry information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub index: u64,
    pub term: u64,
    pub entry_type: String,
    pub data_size: usize,
    pub timestamp: std::time::SystemTime,
}

/// Snapshot information response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfoResponse {
    pub last_included_index: u64,
    pub last_included_term: u64,
    pub size_bytes: u64,
    pub created_at: std::time::SystemTime,
}

/// Backup operation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupOperationResponse {
    pub success: bool,
    pub backup_id: Option<String>,
    pub size_bytes: Option<u64>,
    pub error: Option<String>,
}

/// Backup list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupListResponse {
    pub backups: Vec<BackupInfo>,
}

/// Backup information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub backup_id: String,
    pub description: String,
    pub created_at: std::time::SystemTime,
    pub size_bytes: u64,
    pub cluster_state: RaftState,
}

/// Management request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementRequest {
    pub operation: ManagementOperation,
    pub timeout: Option<Duration>,
    pub authorization: Option<String>,
}

/// Management role for access control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ManagementRole {
    /// Full cluster administration
    Admin,
    /// Read-only monitoring access
    Observer,
    /// Member management only
    MemberManager,
    /// Metrics and health monitoring
    Monitor,
}

/// Permissions for management operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementPermissions {
    pub node_id: u64,
    pub role: ManagementRole,
    pub allowed_operations: Vec<ManagementOperation>,
}

impl ManagementOperation {
    /// Check if this operation requires admin privileges
    pub fn requires_admin(&self) -> bool {
        matches!(self, 
            ManagementOperation::AddMember { .. } |
            ManagementOperation::RemoveMember { .. } |
            ManagementOperation::UpdateMemberRole { .. } |
            ManagementOperation::TransferLeadership { .. } |
            ManagementOperation::StepDown |
            ManagementOperation::UpdateConfiguration { .. } |
            ManagementOperation::CompactLog { .. } |
            ManagementOperation::TriggerSnapshot |
            ManagementOperation::CreateBackup { .. } |
            ManagementOperation::RestoreBackup { .. }
        )
    }
    
    /// Check if this operation is read-only
    pub fn is_read_only(&self) -> bool {
        matches!(self,
            ManagementOperation::GetClusterStatus |
            ManagementOperation::GetConfiguration |
            ManagementOperation::GetMetrics { .. } |
            ManagementOperation::GetHealthStatus |
            ManagementOperation::GetLogInfo |
            ManagementOperation::GetLogEntries { .. } |
            ManagementOperation::GetSnapshotInfo |
            ManagementOperation::ListBackups
        )
    }
}

impl Default for MemberRole {
    fn default() -> Self {
        MemberRole::Voter
    }
}

impl Default for HealthStatus {
    fn default() -> Self {
        HealthStatus::Unknown
    }
}

impl Default for ManagementRole {
    fn default() -> Self {
        ManagementRole::Observer
    }
}