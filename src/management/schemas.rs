//! OpenAPI schemas for the management REST API
//!
//! This module defines the request and response types used by the management API,
//! with OpenAPI/JSON Schema generation via utoipa derives.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use utoipa::ToSchema;

/// Cluster status information
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterStatus {
    /// Current leader node ID
    #[schema(example = 1)]
    pub leader_id: Option<u64>,
    
    /// Current term number
    #[schema(example = 42)]
    pub term: u64,
    
    /// Total number of nodes in the cluster
    #[schema(example = 3)]
    pub cluster_size: usize,
    
    /// List of cluster members
    pub members: Vec<ClusterMember>,
    
    /// Cluster state (Leader, Follower, Candidate)
    pub state: NodeState,
    
    /// Last log index
    #[schema(example = 1000)]
    pub last_log_index: u64,
    
    /// Last log term
    #[schema(example = 42)]
    pub last_log_term: u64,
    
    /// Commit index
    #[schema(example = 1000)]
    pub commit_index: u64,
    
    /// Applied index
    #[schema(example = 1000)]
    pub applied_index: u64,
    
    /// Cluster health status
    pub health: ClusterHealth,
    
    /// Timestamp of this status
    pub timestamp: SystemTime,
}

/// Information about a cluster member
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterMember {
    /// Node ID
    #[schema(example = 1)]
    pub id: u64,
    
    /// Node address
    #[schema(example = "127.0.0.1:8080")]
    pub address: String,
    
    /// Whether the node is currently online
    pub online: bool,
    
    /// Last seen timestamp
    pub last_seen: Option<SystemTime>,
    
    /// Match index (for leaders)
    #[schema(example = 950)]
    pub match_index: Option<u64>,
    
    /// Next index (for leaders)
    #[schema(example = 1001)]
    pub next_index: Option<u64>,
    
    /// Node role in the cluster
    pub role: NodeRole,
    
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Node state in the Raft consensus algorithm
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Node is a follower
    Follower,
    /// Node is a candidate (during election)
    Candidate,
    /// Node is the leader
    Leader,
    /// Node is a learner (non-voting member)
    Learner,
    /// Node is offline or disconnected
    Offline,
}

/// Node role in the cluster
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Voting member of the cluster
    Voter,
    /// Non-voting learner
    Learner,
}

/// Overall cluster health status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClusterHealth {
    /// Cluster is healthy and fully operational
    Healthy,
    /// Cluster is degraded but functional
    Degraded,
    /// Cluster is unhealthy and may not be functional
    Unhealthy,
    /// Cluster status is unknown
    Unknown,
}

/// Request to add a new member to the cluster
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AddMemberRequest {
    /// Node ID for the new member
    #[schema(example = 4)]
    pub node_id: u64,
    
    /// Address of the new member
    #[schema(example = "127.0.0.1:8083")]
    pub address: String,
    
    /// Whether the new member should be a voter or learner
    #[serde(default = "default_voter_role")]
    pub role: NodeRole,
    
    /// Optional metadata for the new member
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

fn default_voter_role() -> NodeRole {
    NodeRole::Voter
}

/// Request to remove a member from the cluster
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RemoveMemberRequest {
    /// Node ID to remove
    #[schema(example = 3)]
    pub node_id: u64,
    
    /// Whether to force removal even if the node is online
    #[serde(default)]
    pub force: bool,
}

/// Request to transfer leadership to another node
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransferLeadershipRequest {
    /// Target node ID to transfer leadership to
    #[schema(example = 2)]
    pub target_node_id: u64,
    
    /// Timeout for the leadership transfer
    #[serde(with = "duration_serde", default = "default_leadership_timeout")]
    pub timeout: Duration,
}

fn default_leadership_timeout() -> Duration {
    Duration::from_secs(10)
}

/// Response for member operations
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemberOperationResponse {
    /// Whether the operation was successful
    pub success: bool,
    
    /// Human-readable message about the operation
    pub message: String,
    
    /// Updated cluster configuration after the operation
    pub cluster_config: Option<ClusterConfiguration>,
}

/// Cluster configuration information
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterConfiguration {
    /// Configuration change index
    #[schema(example = 100)]
    pub config_index: u64,
    
    /// List of all members in the configuration
    pub members: Vec<ClusterMember>,
    
    /// Whether the configuration is committed
    pub committed: bool,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Overall health status
    pub status: HealthStatus,
    
    /// Current timestamp
    pub timestamp: SystemTime,
    
    /// Detailed health checks
    pub checks: Vec<HealthCheck>,
    
    /// Service uptime
    #[serde(with = "duration_serde")]
    pub uptime: Duration,
}

/// Health status enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Service is healthy
    Healthy,
    /// Service is degraded
    Degraded,
    /// Service is unhealthy
    Unhealthy,
}

/// Individual health check result
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthCheck {
    /// Name of the health check
    #[schema(example = "storage")]
    pub name: String,
    
    /// Status of this check
    pub status: HealthStatus,
    
    /// Optional message with details
    pub message: Option<String>,
    
    /// Check execution duration
    #[serde(with = "duration_serde")]
    pub duration: Duration,
}

/// Readiness check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadinessResponse {
    /// Whether the service is ready
    pub ready: bool,
    
    /// Current timestamp
    pub timestamp: SystemTime,
    
    /// List of readiness checks
    pub checks: Vec<ReadinessCheck>,
}

/// Individual readiness check result
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadinessCheck {
    /// Name of the readiness check
    #[schema(example = "raft_initialized")]
    pub name: String,
    
    /// Whether this check passes
    pub ready: bool,
    
    /// Optional message with details
    pub message: Option<String>,
}

/// Metrics response in Prometheus format
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MetricsResponse {
    /// Metrics in Prometheus text format
    #[schema(example = "# HELP raft_term Current Raft term\n# TYPE raft_term gauge\nraft_term 42\n")]
    pub metrics: String,
    
    /// Content type (should be "text/plain; version=0.0.4")
    pub content_type: String,
    
    /// Timestamp when metrics were collected
    pub timestamp: SystemTime,
}

/// Generic error response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Error type/category
    #[schema(example = "validation_error")]
    pub error: String,
    
    /// Human-readable error message
    #[schema(example = "Invalid node ID: must be greater than 0")]
    pub message: String,
    
    /// Optional error details
    pub details: Option<serde_json::Value>,
    
    /// Request ID for tracing
    pub request_id: Option<String>,
    
    /// Timestamp of the error
    pub timestamp: SystemTime,
}

/// Event types for WebSocket streaming
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum ClusterEvent {
    /// Leadership change event
    #[serde(rename = "leadership_change")]
    LeadershipChange {
        /// Previous leader ID
        old_leader: Option<u64>,
        /// New leader ID
        new_leader: Option<u64>,
        /// Term when the change occurred
        term: u64,
        /// Timestamp of the change
        timestamp: SystemTime,
    },
    
    /// Member added to cluster
    #[serde(rename = "member_added")]
    MemberAdded {
        /// Details of the added member
        member: ClusterMember,
        /// Timestamp of the addition
        timestamp: SystemTime,
    },
    
    /// Member removed from cluster
    #[serde(rename = "member_removed")]
    MemberRemoved {
        /// ID of the removed member
        node_id: u64,
        /// Timestamp of the removal
        timestamp: SystemTime,
    },
    
    /// Cluster health status change
    #[serde(rename = "health_change")]
    HealthChange {
        /// Previous health status
        old_status: ClusterHealth,
        /// New health status
        new_status: ClusterHealth,
        /// Timestamp of the change
        timestamp: SystemTime,
    },
    
    /// Log entry committed
    #[serde(rename = "log_committed")]
    LogCommitted {
        /// Index of the committed entry
        index: u64,
        /// Term of the committed entry
        term: u64,
        /// Timestamp of the commit
        timestamp: SystemTime,
    },
}

/// WebSocket subscription request
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubscriptionRequest {
    /// Event types to subscribe to
    pub event_types: Vec<String>,
    
    /// Optional filter parameters
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

// Custom serialization for Duration as seconds
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs_f64().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

// Helper functions for creating responses
impl ClusterStatus {
    /// Create a new cluster status from basic information
    pub fn new(
        leader_id: Option<u64>,
        term: u64,
        members: Vec<ClusterMember>,
        state: NodeState,
    ) -> Self {
        Self {
            leader_id,
            term,
            cluster_size: members.len(),
            members,
            state,
            last_log_index: 0,
            last_log_term: 0,
            commit_index: 0,
            applied_index: 0,
            health: ClusterHealth::Unknown,
            timestamp: SystemTime::now(),
        }
    }
}

impl ClusterMember {
    /// Create a new cluster member
    pub fn new(id: u64, address: String, role: NodeRole) -> Self {
        Self {
            id,
            address,
            online: false,
            last_seen: None,
            match_index: None,
            next_index: None,
            role,
            metadata: HashMap::new(),
        }
    }
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(error: &str, message: &str) -> Self {
        Self {
            error: error.to_string(),
            message: message.to_string(),
            details: None,
            request_id: None,
            timestamp: SystemTime::now(),
        }
    }
    
    /// Create an error response with details
    pub fn with_details(error: &str, message: &str, details: serde_json::Value) -> Self {
        Self {
            error: error.to_string(),
            message: message.to_string(),
            details: Some(details),
            request_id: None,
            timestamp: SystemTime::now(),
        }
    }
}

impl HealthResponse {
    /// Create a healthy response
    pub fn healthy(uptime: Duration) -> Self {
        Self {
            status: HealthStatus::Healthy,
            timestamp: SystemTime::now(),
            checks: vec![],
            uptime,
        }
    }
    
    /// Add a health check to the response
    pub fn with_check(mut self, check: HealthCheck) -> Self {
        self.checks.push(check);
        // Update overall status based on checks
        if self.checks.iter().any(|c| matches!(c.status, HealthStatus::Unhealthy)) {
            self.status = HealthStatus::Unhealthy;
        } else if self.checks.iter().any(|c| matches!(c.status, HealthStatus::Degraded)) {
            self.status = HealthStatus::Degraded;
        }
        self
    }
}

impl ReadinessResponse {
    /// Create a new readiness response
    pub fn new(checks: Vec<ReadinessCheck>) -> Self {
        let ready = checks.iter().all(|c| c.ready);
        Self {
            ready,
            timestamp: SystemTime::now(),
            checks,
        }
    }
}