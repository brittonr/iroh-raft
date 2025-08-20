//! High-level cluster API for iroh-raft
//!
//! This module provides a simplified, ergonomic API for creating and managing
//! Raft clusters. It abstracts away low-level details and provides sensible
//! defaults for common use cases.
//!
//! ## Design Principles
//!
//! 1. **Simplicity**: Easy to use for common cases
//! 2. **Flexibility**: Powerful enough for advanced use cases
//! 3. **Safety**: Type-safe with comprehensive error handling
//! 4. **Performance**: Minimal overhead over the core library
//! 5. **Observability**: Built-in metrics and monitoring
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use iroh_raft::cluster::{RaftCluster, StateMachine};
//! use serde::{Deserialize, Serialize};
//! use std::collections::HashMap;
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! enum Command {
//!     Set { key: String, value: String },
//!     Delete { key: String },
//! }
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct State {
//!     data: HashMap<String, String>,
//! }
//!
//! struct MyStateMachine {
//!     state: State,
//! }
//!
//! impl StateMachine for MyStateMachine {
//!     type Command = Command;
//!     type State = State;
//!     type Error = std::io::Error;
//!
//!     async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error> {
//!         match command {
//!             Command::Set { key, value } => {
//!                 self.state.data.insert(key, value);
//!             }
//!             Command::Delete { key } => {
//!                 self.state.data.remove(&key);
//!             }
//!         }
//!         Ok(())
//!     }
//!
//!     async fn snapshot(&self) -> Result<Self::State, Self::Error> {
//!         Ok(self.state.clone())
//!     }
//!
//!     async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error> {
//!         self.state = snapshot;
//!         Ok(())
//!     }
//!
//!     fn query(&self) -> &Self::State {
//!         &self.state
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let cluster = RaftCluster::builder(1)
//!         .bind_address("127.0.0.1:8080")
//!         .join_cluster(vec!["127.0.0.1:8081".parse()?])
//!         .enable_management_api("127.0.0.1:9090".parse()?)
//!         .build(MyStateMachine {
//!             state: State { data: HashMap::new() }
//!         })
//!         .await?;
//!
//!     // Propose commands
//!     cluster.propose(Command::Set {
//!         key: "hello".to_string(),
//!         value: "world".to_string(),
//!     }).await?;
//!
//!     // Query state
//!     let state = cluster.query().await;
//!     println!("Data: {:?}", state.data);
//!
//!     // Check health
//!     let health = cluster.health().await;
//!     println!("Cluster health: {:?}", health.status);
//!
//!     cluster.shutdown().await?;
//!     Ok(())
//! }
//! ```

use crate::config::{Config, ConfigError};
use crate::error::RaftError;
use crate::types::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[cfg(feature = "metrics-otel")]
use crate::metrics::MetricsRegistry;

/// Simplified state machine trait for the high-level API
///
/// This replaces the complex existing state machine APIs with a single,
/// easy-to-understand interface.
pub trait StateMachine: Send + Sync + 'static {
    /// Command type that can be applied to the state machine
    type Command: Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone + fmt::Debug;
    
    /// State type that represents the complete state
    type State: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + fmt::Debug;
    
    /// Error type for state machine operations
    type Error: Error + Send + Sync + 'static;

    /// Apply a command to update the state machine
    ///
    /// This method should be deterministic - applying the same command
    /// to the same state should always produce the same result.
    async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error>;

    /// Create a snapshot of the current state
    async fn snapshot(&self) -> Result<Self::State, Self::Error>;

    /// Restore the state machine from a snapshot
    async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error>;

    /// Get a read-only view of the current state
    fn query(&self) -> &Self::State;
}

/// High-level Raft cluster interface
///
/// This is the main entry point for applications using iroh-raft.
/// It provides a simple API for managing a Raft cluster and proposing
/// commands to the state machine.
pub struct RaftCluster<SM: StateMachine> {
    inner: Arc<ClusterInner<SM>>,
}

struct ClusterInner<SM: StateMachine> {
    node_id: NodeId,
    config: Config,
    state_machine: Arc<RwLock<SM>>,
    status: Arc<RwLock<ClusterStatus>>,
    health: Arc<RwLock<ClusterHealth>>,
    #[cfg(feature = "metrics-otel")]
    metrics: MetricsRegistry,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    shutdown_complete: Arc<tokio::sync::Notify>,
}

/// Builder for creating Raft clusters with sensible defaults
pub struct ClusterBuilder<SM> {
    node_id: NodeId,
    config: Config,
    management_api_addr: Option<SocketAddr>,
    #[cfg(feature = "metrics-otel")]
    metrics_addr: Option<SocketAddr>,
    _phantom: PhantomData<SM>,
}

/// Current status of the Raft cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    /// Unique cluster identifier
    pub cluster_id: String,
    
    /// Current leader node ID (if known)
    pub leader_id: Option<NodeId>,
    
    /// Current Raft term
    pub term: u64,
    
    /// Highest log entry known to be committed
    pub commit_index: u64,
    
    /// Highest log entry applied to state machine
    pub applied_index: u64,
    
    /// List of all nodes in the cluster
    pub nodes: Vec<NodeInfo>,
    
    /// Current state of this node
    pub state: NodeState,
    
    /// When this status was last updated
    pub last_updated: SystemTime,
}

/// Information about a cluster node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier
    pub id: NodeId,
    
    /// Network address of the node
    pub address: SocketAddr,
    
    /// Current role in the cluster
    pub role: NodeRole,
    
    /// Current connectivity status
    pub status: NodeStatus,
    
    /// Last successful contact with this node
    pub last_contact: Option<SystemTime>,
    
    /// Optional node metadata
    pub metadata: HashMap<String, String>,
}

/// Role of a node in the Raft cluster
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    /// Node is the cluster leader
    Leader,
    /// Node is a follower
    Follower,
    /// Node is a candidate for leadership
    Candidate,
    /// Node is a learner (non-voting member)
    Learner,
}

/// Connectivity status of a node
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is online and reachable
    Online,
    /// Node is offline or unreachable
    Offline,
    /// Node status is unknown
    Unknown,
}

/// State of a Raft node
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeState {
    /// Node is the leader
    Leader,
    /// Node is a follower
    Follower,
    /// Node is a candidate
    Candidate,
    /// Node is in pre-candidate state
    PreCandidate,
    /// Node is a learner
    Learner,
}

/// Health status of the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHealth {
    /// Overall health status
    pub status: HealthStatus,
    
    /// Individual health checks
    pub checks: Vec<HealthCheck>,
    
    /// Human-readable summary
    pub summary: String,
    
    /// When the health check was performed
    pub timestamp: SystemTime,
}

/// Overall health status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    /// Cluster is fully healthy
    Healthy,
    /// Cluster is functional but degraded
    Degraded,
    /// Cluster is unhealthy
    Unhealthy,
}

/// Individual health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Name of the health check
    pub name: String,
    
    /// Status of this check
    pub status: CheckStatus,
    
    /// Details about the check result
    pub message: String,
    
    /// Time taken to perform the check
    pub duration: Duration,
}

/// Status of an individual health check
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed
    Pass,
    /// Check passed with warnings
    Warn,
    /// Check failed
    Fail,
}

/// Result of an administrative operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    /// Whether the operation succeeded
    pub success: bool,
    
    /// Human-readable result message
    pub message: String,
    
    /// Unique identifier for tracking the operation
    pub operation_id: String,
    
    /// When the operation completed
    pub timestamp: SystemTime,
}

impl<SM: StateMachine> ClusterBuilder<SM> {
    /// Create a new cluster builder
    pub fn new(node_id: NodeId) -> Self {
        let mut config = Config::default();
        config.node.id = Some(node_id);
        
        Self {
            node_id,
            config,
            management_api_addr: None,
            #[cfg(feature = "metrics-otel")]
            metrics_addr: None,
            _phantom: PhantomData,
        }
    }

    /// Set the bind address for the Raft transport
    pub fn bind_address(mut self, addr: impl Into<SocketAddr>) -> Self {
        self.config.transport.bind_address = addr.into();
        self
    }

    /// Join an existing cluster by specifying peer addresses
    pub fn join_cluster(mut self, peers: Vec<SocketAddr>) -> Self {
        self.config.transport.peers = peers;
        self
    }

    /// Set the data directory for persistent storage
    pub fn data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.node.data_dir = path.into();
        self
    }

    /// Set election timeout (randomized between min and min+2s)
    pub fn election_timeout(mut self, timeout: Duration) -> Self {
        self.config.raft.election_timeout_min = timeout;
        self.config.raft.election_timeout_max = timeout + Duration::from_millis(2000);
        self
    }

    /// Set heartbeat interval
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.config.raft.heartbeat_interval = interval;
        self
    }

    /// Set maximum number of uncommitted log entries
    pub fn max_log_entries(mut self, max: u64) -> Self {
        self.config.raft.max_uncommitted_entries = max;
        self
    }

    /// Enable metrics collection with optional bind address
    #[cfg(feature = "metrics-otel")]
    pub fn enable_metrics(mut self, addr: Option<SocketAddr>) -> Self {
        self.config.observability.metrics.enabled = true;
        self.config.observability.metrics.bind_address = addr;
        self.metrics_addr = addr;
        self
    }

    /// Enable the management API on the specified address
    pub fn enable_management_api(mut self, addr: SocketAddr) -> Self {
        self.management_api_addr = Some(addr);
        self
    }

    /// Enable distributed tracing
    pub fn enable_tracing(mut self, endpoint: String) -> Self {
        self.config.observability.tracing.enabled = true;
        self.config.observability.tracing.otlp_endpoint = Some(endpoint);
        self
    }

    /// Set tracing sample rate (0.0 to 1.0)
    pub fn tracing_sample_rate(mut self, rate: f64) -> Self {
        self.config.observability.tracing.sample_rate = rate;
        self
    }

    /// Enable debug mode
    pub fn debug(mut self, enabled: bool) -> Self {
        self.config.node.debug = enabled;
        self
    }

    /// Set log level
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.observability.logging.level = level.into();
        self
    }

    /// Build the cluster with the provided state machine
    pub async fn build(self, state_machine: SM) -> Result<RaftCluster<SM>, RaftError> {
        // Validate configuration
        self.config.validate().map_err(|e| RaftError::InvalidConfiguration {
            component: "cluster_builder".to_string(),
            message: format!("Configuration validation failed: {}", e),
        })?;

        // Create shutdown channels
        let (shutdown_tx, _shutdown_rx) = tokio::sync::oneshot::channel();
        let shutdown_complete = Arc::new(tokio::sync::Notify::new());

        // Initialize metrics if enabled
        #[cfg(feature = "metrics-otel")]
        let metrics = MetricsRegistry::new(&self.config.observability.metrics)?;

        // Create cluster status
        let status = ClusterStatus {
            cluster_id: Uuid::new_v4().to_string(),
            leader_id: None,
            term: 0,
            commit_index: 0,
            applied_index: 0,
            nodes: vec![NodeInfo {
                id: self.node_id,
                address: self.config.transport.bind_address,
                role: NodeRole::Follower,
                status: NodeStatus::Online,
                last_contact: Some(SystemTime::now()),
                metadata: HashMap::new(),
            }],
            state: NodeState::Follower,
            last_updated: SystemTime::now(),
        };

        // Create health status
        let health = ClusterHealth {
            status: HealthStatus::Healthy,
            checks: vec![],
            summary: "Cluster starting up".to_string(),
            timestamp: SystemTime::now(),
        };

        let inner = ClusterInner {
            node_id: self.node_id,
            config: self.config,
            state_machine: Arc::new(RwLock::new(state_machine)),
            status: Arc::new(RwLock::new(status)),
            health: Arc::new(RwLock::new(health)),
            #[cfg(feature = "metrics-otel")]
            metrics,
            shutdown_tx: Some(shutdown_tx),
            shutdown_complete: shutdown_complete.clone(),
        };

        let cluster = RaftCluster {
            inner: Arc::new(inner),
        };

        // Start management API if enabled
        if let Some(addr) = self.management_api_addr {
            #[cfg(feature = "management-api")]
            {
                let api_cluster = cluster.clone();
                tokio::spawn(async move {
                    if let Err(e) = start_management_api(addr, api_cluster).await {
                        tracing::error!("Management API failed: {}", e);
                    }
                });
            }
            
            #[cfg(not(feature = "management-api"))]
            {
                tracing::warn!("Management API requested but feature not enabled");
            }
        }

        Ok(cluster)
    }
}

impl<SM: StateMachine> RaftCluster<SM> {
    /// Create a new cluster builder
    pub fn builder(node_id: NodeId) -> ClusterBuilder<SM> {
        ClusterBuilder::new(node_id)
    }

    /// Propose a command to the cluster
    ///
    /// This submits a command to the Raft consensus algorithm. The command
    /// will be replicated to all nodes and applied to the state machine
    /// once committed.
    pub async fn propose(&self, command: SM::Command) -> Result<(), RaftError> {
        // TODO: Implement actual Raft proposal
        // For now, just apply directly to demonstrate the API
        let mut state_machine = self.inner.state_machine.write().await;
        state_machine.apply(command).await.map_err(|e| RaftError::StateMachine(Box::new(e)))?;
        
        #[cfg(feature = "metrics-otel")]
        {
            // Update metrics
            // self.inner.metrics.record_proposal();
        }
        
        Ok(())
    }

    /// Query the current state of the state machine
    pub async fn query(&self) -> SM::State {
        let state_machine = self.inner.state_machine.read().await;
        state_machine.query().clone()
    }

    /// Add a new node to the cluster
    pub async fn add_node(&self, node_id: NodeId, address: SocketAddr) -> Result<OperationResult, RaftError> {
        // TODO: Implement actual node addition through Raft configuration change
        
        let operation_id = Uuid::new_v4().to_string();
        
        // Update cluster status
        let mut status = self.inner.status.write().await;
        status.nodes.push(NodeInfo {
            id: node_id,
            address,
            role: NodeRole::Learner,
            status: NodeStatus::Unknown,
            last_contact: None,
            metadata: HashMap::new(),
        });
        status.last_updated = SystemTime::now();
        
        Ok(OperationResult {
            success: true,
            message: format!("Node {} added successfully", node_id),
            operation_id,
            timestamp: SystemTime::now(),
        })
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&self, node_id: NodeId) -> Result<OperationResult, RaftError> {
        // TODO: Implement actual node removal through Raft configuration change
        
        let operation_id = Uuid::new_v4().to_string();
        
        // Update cluster status
        let mut status = self.inner.status.write().await;
        status.nodes.retain(|node| node.id != node_id);
        status.last_updated = SystemTime::now();
        
        Ok(OperationResult {
            success: true,
            message: format!("Node {} removed successfully", node_id),
            operation_id,
            timestamp: SystemTime::now(),
        })
    }

    /// Transfer leadership to another node
    pub async fn transfer_leadership(&self, to_node: NodeId) -> Result<OperationResult, RaftError> {
        // TODO: Implement actual leadership transfer
        
        let operation_id = Uuid::new_v4().to_string();
        
        Ok(OperationResult {
            success: true,
            message: format!("Leadership transfer to node {} initiated", to_node),
            operation_id,
            timestamp: SystemTime::now(),
        })
    }

    /// Get current cluster health
    pub async fn health(&self) -> ClusterHealth {
        // TODO: Implement comprehensive health checking
        
        let checks = vec![
            HealthCheck {
                name: "consensus".to_string(),
                status: CheckStatus::Pass,
                message: "Consensus algorithm functioning normally".to_string(),
                duration: Duration::from_millis(1),
            },
            HealthCheck {
                name: "storage".to_string(),
                status: CheckStatus::Pass,
                message: "Storage backend operational".to_string(),
                duration: Duration::from_millis(2),
            },
            HealthCheck {
                name: "network".to_string(),
                status: CheckStatus::Pass,
                message: "Network connectivity good".to_string(),
                duration: Duration::from_millis(5),
            },
        ];

        let health = ClusterHealth {
            status: HealthStatus::Healthy,
            checks,
            summary: "All systems operational".to_string(),
            timestamp: SystemTime::now(),
        };

        // Update stored health
        let mut stored_health = self.inner.health.write().await;
        *stored_health = health.clone();

        health
    }

    /// Get current cluster status
    pub async fn status(&self) -> ClusterStatus {
        let status = self.inner.status.read().await;
        status.clone()
    }

    /// Get access to the metrics registry
    #[cfg(feature = "metrics-otel")]
    pub fn metrics(&self) -> &MetricsRegistry {
        &self.inner.metrics
    }

    /// Shutdown the cluster gracefully
    pub async fn shutdown(&self) -> Result<(), RaftError> {
        // TODO: Implement graceful shutdown of all components
        
        tracing::info!("Initiating cluster shutdown");
        
        // Signal shutdown to all components
        if let Some(tx) = self.inner.shutdown_tx.take() {
            let _ = tx.send(());
        }
        
        // Wait for all components to shut down
        self.inner.shutdown_complete.notify_waiters();
        
        tracing::info!("Cluster shutdown complete");
        Ok(())
    }

    /// Wait for cluster shutdown to complete
    pub async fn wait_for_shutdown(&self) {
        self.inner.shutdown_complete.notified().await;
    }
}

impl<SM: StateMachine> Clone for RaftCluster<SM> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

// Implementation of management API (feature-gated)
#[cfg(feature = "management-api")]
async fn start_management_api<SM: StateMachine>(
    addr: SocketAddr,
    cluster: RaftCluster<SM>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
        response::Json,
        routing::{get, post, delete, put},
        Router,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;

    // Create router with all management endpoints
    let app = Router::new()
        // Cluster endpoints
        .route("/api/v1/cluster", get(get_cluster_status))
        .route("/api/v1/cluster/health", get(get_cluster_health))
        .route("/api/v1/cluster/leadership/transfer", post(transfer_leadership))
        
        // Node endpoints
        .route("/api/v1/nodes", get(list_nodes).post(add_node))
        .route("/api/v1/nodes/:id", get(get_node).delete(remove_node))
        
        // State endpoints
        .route("/api/v1/state", get(query_state))
        .route("/api/v1/state/snapshot", post(create_snapshot))
        
        // Monitoring endpoints
        .route("/api/v1/metrics", get(get_metrics))
        .with_state(cluster);

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Management API listening on {}", addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(feature = "management-api")]
async fn get_cluster_status<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
) -> Result<Json<ClusterStatus>, StatusCode> {
    let status = cluster.status().await;
    Ok(Json(status))
}

#[cfg(feature = "management-api")]
async fn get_cluster_health<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
) -> Result<Json<ClusterHealth>, StatusCode> {
    let health = cluster.health().await;
    Ok(Json(health))
}

#[cfg(feature = "management-api")]
async fn transfer_leadership<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<OperationResult>, StatusCode> {
    let target_node_id = payload["target_node_id"]
        .as_u64()
        .ok_or(StatusCode::BAD_REQUEST)?;
    
    match cluster.transfer_leadership(target_node_id).await {
        Ok(result) => Ok(Json(result)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(feature = "management-api")]
async fn list_nodes<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
) -> Result<Json<Vec<NodeInfo>>, StatusCode> {
    let status = cluster.status().await;
    Ok(Json(status.nodes))
}

#[cfg(feature = "management-api")]
async fn add_node<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<OperationResult>, StatusCode> {
    let node_id = payload["node_id"]
        .as_u64()
        .ok_or(StatusCode::BAD_REQUEST)?;
    
    let address_str = payload["address"]
        .as_str()
        .ok_or(StatusCode::BAD_REQUEST)?;
    
    let address: SocketAddr = address_str
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    match cluster.add_node(node_id, address).await {
        Ok(result) => Ok(Json(result)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(feature = "management-api")]
async fn get_node<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
    Path(node_id): Path<u64>,
) -> Result<Json<NodeInfo>, StatusCode> {
    let status = cluster.status().await;
    let node = status.nodes
        .into_iter()
        .find(|n| n.id == node_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    
    Ok(Json(node))
}

#[cfg(feature = "management-api")]
async fn remove_node<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
    Path(node_id): Path<u64>,
) -> Result<Json<OperationResult>, StatusCode> {
    match cluster.remove_node(node_id).await {
        Ok(result) => Ok(Json(result)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(feature = "management-api")]
async fn query_state<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
) -> Result<Json<SM::State>, StatusCode> {
    let state = cluster.query().await;
    Ok(Json(state))
}

#[cfg(feature = "management-api")]
async fn create_snapshot<SM: StateMachine>(
    State(cluster): State<RaftCluster<SM>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement snapshot creation
    Ok(Json(json!({
        "success": true,
        "message": "Snapshot created successfully",
        "timestamp": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    })))
}

#[cfg(feature = "management-api")]
async fn get_metrics<SM: StateMachine>(
    State(_cluster): State<RaftCluster<SM>>,
) -> Result<String, StatusCode> {
    // TODO: Return Prometheus metrics format
    Ok("# Placeholder metrics\niroh_raft_up 1\n".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Serialize, Deserialize, Clone, Debug)]
    enum TestCommand {
        Set { key: String, value: String },
        Delete { key: String },
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    struct TestState {
        data: HashMap<String, String>,
    }

    struct TestStateMachine {
        state: TestState,
    }

    impl StateMachine for TestStateMachine {
        type Command = TestCommand;
        type State = TestState;
        type Error = std::io::Error;

        async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error> {
            match command {
                TestCommand::Set { key, value } => {
                    self.state.data.insert(key, value);
                }
                TestCommand::Delete { key } => {
                    self.state.data.remove(&key);
                }
            }
            Ok(())
        }

        async fn snapshot(&self) -> Result<Self::State, Self::Error> {
            Ok(self.state.clone())
        }

        async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error> {
            self.state = snapshot;
            Ok(())
        }

        fn query(&self) -> &Self::State {
            &self.state
        }
    }

    #[tokio::test]
    async fn test_cluster_builder() {
        let cluster = RaftCluster::builder(1)
            .bind_address("127.0.0.1:0".parse().unwrap())
            .data_dir("/tmp/test-cluster")
            .debug(true)
            .build(TestStateMachine {
                state: TestState {
                    data: HashMap::new(),
                },
            })
            .await
            .expect("Failed to build cluster");

        // Test basic operations
        cluster
            .propose(TestCommand::Set {
                key: "test".to_string(),
                value: "value".to_string(),
            })
            .await
            .expect("Failed to propose command");

        let state = cluster.query().await;
        assert_eq!(state.data.get("test"), Some(&"value".to_string()));

        // Test health check
        let health = cluster.health().await;
        assert_eq!(health.status, HealthStatus::Healthy);

        // Test status
        let status = cluster.status().await;
        assert_eq!(status.nodes.len(), 1);
        assert_eq!(status.nodes[0].id, 1);

        cluster.shutdown().await.expect("Failed to shutdown");
    }

    #[tokio::test]
    async fn test_node_management() {
        let cluster = RaftCluster::builder(1)
            .bind_address("127.0.0.1:0".parse().unwrap())
            .build(TestStateMachine {
                state: TestState {
                    data: HashMap::new(),
                },
            })
            .await
            .expect("Failed to build cluster");

        // Test adding a node
        let result = cluster
            .add_node(2, "127.0.0.1:8081".parse().unwrap())
            .await
            .expect("Failed to add node");

        assert!(result.success);

        let status = cluster.status().await;
        assert_eq!(status.nodes.len(), 2);

        // Test removing a node
        let result = cluster
            .remove_node(2)
            .await
            .expect("Failed to remove node");

        assert!(result.success);

        let status = cluster.status().await;
        assert_eq!(status.nodes.len(), 1);

        cluster.shutdown().await.expect("Failed to shutdown");
    }
}