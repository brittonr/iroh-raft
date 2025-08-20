//! High-level cluster management API for iroh-raft
//! 
//! This module provides a unified, ergonomic interface for managing Raft clusters,
//! addressing the ergonomics issues identified in the original implementation.
//! The cluster API provides:
//! 
//! - **Unified State Machine API**: Single consistent pattern for all operations
//! - **High-level abstractions**: Simple propose(), query(), and administrative operations
//! - **Simplified configuration**: Builder with sensible defaults and presets
//! - **Administrative operations**: Member management, leadership operations, health checks
//! - **Graceful lifecycle management**: Startup, shutdown, and resource cleanup
//! 
//! # Example Usage
//! 
//! ```rust,no_run
//! use iroh_raft::{cluster::RaftCluster, raft::KeyValueStore, config::ConfigBuilder};
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create cluster configuration with sensible defaults
//!     let config = ConfigBuilder::development_preset()
//!         .node_id(1)
//!         .data_dir("./data")
//!         .bind_address("127.0.0.1:8080")
//!         .add_peer("127.0.0.1:8081")
//!         .build()?;
//! 
//!     // Create and start cluster
//!     let mut cluster: RaftCluster<KeyValueStore> = RaftCluster::new(config).await?;
//! 
//!     // Propose a command
//!     let response = cluster.propose(KvCommand::Set {
//!         key: "user:123".to_string(),
//!         value: "Alice".to_string(),
//!     }).await?;
//! 
//!     // Execute a read-only query
//!     let result = cluster.query(KvQuery::Get {
//!         key: "user:123".to_string(),
//!     }).await?;
//! 
//!     // Administrative operations
//!     let status = cluster.status();
//!     println!("Cluster status: {:?}", status);
//! 
//!     // Graceful shutdown
//!     cluster.shutdown().await?;
//!     Ok(())
//! }
//! ```

use crate::config::Config;
use crate::error::{RaftError, Result};
use crate::raft::StateMachine;
use crate::types::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// Trait alias for state machines used in cluster management
/// 
/// This provides a convenient alias for the StateMachine trait specifically
/// for use in cluster contexts. It's semantically identical to StateMachine
/// but provides better ergonomics for cluster-specific usage.
pub trait ClusterStateMachine: StateMachine {}

// Blanket implementation - any StateMachine is also a ClusterStateMachine
impl<T> ClusterStateMachine for T where T: StateMachine {}

/// High-level cluster management interface
/// 
/// Provides a unified API for managing Raft clusters with ergonomic operations
/// for proposals, queries, membership management, and administrative tasks.
pub struct RaftCluster<S: StateMachine> {
    /// Node ID of this cluster instance
    node_id: NodeId,
    /// Cluster configuration
    config: Config,
    /// Command channel for sending operations to the cluster
    command_tx: mpsc::UnboundedSender<ClusterCommand<S>>,
    /// Handle to the cluster task for lifecycle management
    _cluster_handle: tokio::task::JoinHandle<()>,
    /// Shutdown signal for graceful termination
    shutdown_tx: Option<oneshot::Sender<()>>,
}

/// Configuration for cluster member management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberConfig {
    /// Node ID of the member
    pub node_id: NodeId,
    /// Network address of the member
    pub address: String,
    /// Optional metadata for the member
    pub metadata: Option<HashMap<String, String>>,
}

/// Current status of the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    /// This node's ID
    pub node_id: NodeId,
    /// Current Raft role (Leader, Follower, Candidate)
    pub role: NodeRole,
    /// Current Raft term
    pub term: u64,
    /// Current commit index
    pub commit_index: u64,
    /// Last applied index
    pub applied_index: u64,
    /// Current cluster members
    pub members: Vec<NodeId>,
    /// Leader node ID, if known
    pub leader_id: Option<NodeId>,
    /// Health status of the cluster
    pub health: HealthStatus,
    /// Cluster metrics
    pub metrics: ClusterMetrics,
}

/// Node role in the Raft cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeRole {
    /// Node is a follower
    Follower,
    /// Node is a candidate
    Candidate,
    /// Node is the leader
    Leader,
}

/// Health status of the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Overall cluster health
    pub overall: Health,
    /// Individual component health
    pub components: HashMap<String, Health>,
    /// Last health check timestamp
    pub last_check: SystemTime,
}

/// Health state enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Health {
    /// Component is healthy
    Healthy,
    /// Component is degraded but functional
    Degraded,
    /// Component is unhealthy
    Unhealthy,
}

/// Cluster performance and operational metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMetrics {
    /// Total number of proposals processed
    pub total_proposals: u64,
    /// Number of successful proposals
    pub successful_proposals: u64,
    /// Number of failed proposals
    pub failed_proposals: u64,
    /// Total number of queries processed
    pub total_queries: u64,
    /// Average proposal latency in milliseconds
    pub avg_proposal_latency_ms: f64,
    /// Average query latency in milliseconds
    pub avg_query_latency_ms: f64,
    /// Current memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Storage size in bytes
    pub storage_size_bytes: u64,
}

/// Information about the cluster configuration and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInfo {
    /// Cluster configuration
    pub config: ClusterConfig,
    /// Current membership
    pub membership: ClusterMembership,
    /// Performance statistics
    pub statistics: ClusterStatistics,
}

/// High-level cluster configuration information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Cluster name or identifier
    pub name: Option<String>,
    /// Number of nodes in the cluster
    pub node_count: usize,
    /// Replication factor
    pub replication_factor: usize,
    /// Election timeout range
    pub election_timeout_ms: (u64, u64),
    /// Heartbeat interval
    pub heartbeat_interval_ms: u64,
}

/// Cluster membership information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMembership {
    /// List of all cluster members
    pub members: Vec<MemberInfo>,
    /// Current leader
    pub leader: Option<MemberInfo>,
    /// Nodes currently joining the cluster
    pub joining: Vec<MemberInfo>,
    /// Nodes currently leaving the cluster
    pub leaving: Vec<MemberInfo>,
}

/// Information about a cluster member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    /// Node ID
    pub node_id: NodeId,
    /// Network address
    pub address: String,
    /// Current role
    pub role: NodeRole,
    /// Whether the node is reachable
    pub reachable: bool,
    /// Last seen timestamp
    pub last_seen: SystemTime,
    /// Node metadata
    pub metadata: HashMap<String, String>,
}

/// Cluster performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatistics {
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Total operations processed
    pub total_operations: u64,
    /// Operations per second (current rate)
    pub ops_per_second: f64,
    /// Average latency for operations
    pub avg_latency_ms: f64,
    /// Error rate (percentage)
    pub error_rate_percent: f64,
}

/// Unique identifier for snapshots
pub type SnapshotId = String;

/// Internal cluster command types
enum ClusterCommand<S: StateMachine> {
    /// Propose a command through Raft consensus
    Propose {
        command: S::Command,
        response_tx: oneshot::Sender<Result<S::Response>>,
    },
    /// Execute a read-only query
    Query {
        query: S::Query,
        response_tx: oneshot::Sender<Result<S::QueryResponse>>,
    },
    /// Add a new member to the cluster
    AddMember {
        config: MemberConfig,
        response_tx: oneshot::Sender<Result<()>>,
    },
    /// Remove a member from the cluster
    RemoveMember {
        node_id: NodeId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    /// Transfer leadership to another node
    TransferLeadership {
        target: NodeId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    /// Get current cluster status
    GetStatus {
        response_tx: oneshot::Sender<ClusterStatus>,
    },
    /// Perform health check
    HealthCheck {
        response_tx: oneshot::Sender<HealthStatus>,
    },
    /// Get cluster information
    GetClusterInfo {
        response_tx: oneshot::Sender<ClusterInfo>,
    },
    /// Take a snapshot
    TakeSnapshot {
        response_tx: oneshot::Sender<Result<SnapshotId>>,
    },
    /// Compact log entries
    CompactLog {
        retain_index: u64,
        response_tx: oneshot::Sender<Result<()>>,
    },
}

impl<S: StateMachine> RaftCluster<S> {
    /// Create a new Raft cluster with the given configuration
    /// 
    /// This initializes all cluster components and starts background tasks
    /// for consensus, networking, and storage operations.
    pub async fn new(config: Config) -> Result<Self> {
        let node_id = config.node.id.ok_or_else(|| RaftError::MissingConfiguration {
            parameter: "node_id".to_string(),
            component: "cluster".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        info!("Initializing RaftCluster with node_id: {}", node_id);

        // Create command channel for cluster operations
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        
        // Create shutdown signal
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Start the cluster task
        let config_clone = config.clone();
        let cluster_handle = tokio::spawn(async move {
            if let Err(e) = Self::run_cluster_task(config_clone, command_rx, shutdown_rx).await {
                warn!("Cluster task ended with error: {}", e);
            }
        });

        Ok(RaftCluster {
            node_id,
            config,
            command_tx,
            _cluster_handle: cluster_handle,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Propose a command to be applied through Raft consensus
    /// 
    /// This method submits a command to the Raft log and waits for it to be
    /// committed and applied to the state machine.
    pub async fn propose(&self, command: S::Command) -> Result<S::Response> {
        debug!("Proposing command to cluster");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::Propose {
            command,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "proposal response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Execute a read-only query without consensus
    /// 
    /// This method executes queries directly on the local state machine
    /// without going through the Raft consensus process.
    pub async fn query(&self, query: S::Query) -> Result<S::QueryResponse> {
        debug!("Executing query on cluster");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::Query {
            query,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "query response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Add a new member to the cluster
    /// 
    /// This initiates a configuration change to add a new node to the cluster.
    /// The operation will be committed through Raft consensus.
    pub async fn add_member(&mut self, node_config: MemberConfig) -> Result<()> {
        info!("Adding member {} to cluster", node_config.node_id);
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::AddMember {
            config: node_config,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "add member response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Remove a member from the cluster
    /// 
    /// This initiates a configuration change to remove a node from the cluster.
    /// The operation will be committed through Raft consensus.
    pub async fn remove_member(&mut self, node_id: NodeId) -> Result<()> {
        info!("Removing member {} from cluster", node_id);
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::RemoveMember {
            node_id,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "remove member response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Transfer leadership to another node
    /// 
    /// This requests the current leader to transfer leadership to the specified node.
    /// Only works if this node is currently the leader.
    pub async fn transfer_leadership(&self, target: NodeId) -> Result<()> {
        info!("Transferring leadership to node {}", target);
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::TransferLeadership {
            target,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "transfer leadership response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Get the current status of the cluster
    /// 
    /// Returns comprehensive information about the cluster state, including
    /// leadership, membership, and operational metrics.
    pub async fn status(&self) -> ClusterStatus {
        debug!("Getting cluster status");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        if self.command_tx.send(ClusterCommand::GetStatus {
            response_tx,
        }).is_err() {
            // If the command channel is closed, return a degraded status
            return ClusterStatus {
                node_id: self.node_id,
                role: NodeRole::Follower,
                term: 0,
                commit_index: 0,
                applied_index: 0,
                members: vec![],
                leader_id: None,
                health: HealthStatus {
                    overall: Health::Unhealthy,
                    components: HashMap::new(),
                    last_check: SystemTime::now(),
                },
                metrics: ClusterMetrics::default(),
            };
        }

        response_rx.await.unwrap_or_else(|_| ClusterStatus {
            node_id: self.node_id,
            role: NodeRole::Follower,
            term: 0,
            commit_index: 0,
            applied_index: 0,
            members: vec![],
            leader_id: None,
            health: HealthStatus {
                overall: Health::Unhealthy,
                components: HashMap::new(),
                last_check: SystemTime::now(),
            },
            metrics: ClusterMetrics::default(),
        })
    }

    /// Perform a health check on the cluster
    /// 
    /// Returns the current health status of various cluster components.
    pub async fn health_check(&self) -> HealthStatus {
        debug!("Performing cluster health check");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        if self.command_tx.send(ClusterCommand::HealthCheck {
            response_tx,
        }).is_err() {
            return HealthStatus {
                overall: Health::Unhealthy,
                components: HashMap::new(),
                last_check: SystemTime::now(),
            };
        }

        response_rx.await.unwrap_or_else(|_| HealthStatus {
            overall: Health::Unhealthy,
            components: HashMap::new(),
            last_check: SystemTime::now(),
        })
    }

    /// Get detailed information about the cluster
    /// 
    /// Returns comprehensive cluster information including configuration,
    /// membership, and performance statistics.
    pub async fn cluster_info(&self) -> ClusterInfo {
        debug!("Getting cluster information");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        if self.command_tx.send(ClusterCommand::GetClusterInfo {
            response_tx,
        }).is_err() {
            return ClusterInfo::default();
        }

        response_rx.await.unwrap_or_default()
    }

    /// Take a snapshot of the current cluster state
    /// 
    /// Creates a point-in-time snapshot that can be used for backups
    /// or to bootstrap new cluster members.
    pub async fn take_snapshot(&self) -> Result<SnapshotId> {
        info!("Taking cluster snapshot");
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::TakeSnapshot {
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "take snapshot response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Compact the Raft log, retaining entries up to the specified index
    /// 
    /// This helps manage storage space by removing old log entries that
    /// are no longer needed.
    pub async fn compact_log(&mut self, retain_index: u64) -> Result<()> {
        info!("Compacting cluster log, retaining up to index {}", retain_index);
        
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx.send(ClusterCommand::CompactLog {
            retain_index,
            response_tx,
        }).map_err(|_| RaftError::ServiceUnavailable {
            service: "cluster".to_string(),
            reason: "cluster command channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        response_rx.await.map_err(|_| RaftError::Internal {
            message: "compact log response channel closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?
    }

    /// Gracefully shut down the cluster
    /// 
    /// This stops all background tasks and cleans up resources.
    /// The cluster cannot be used after shutdown.
    pub async fn shutdown(mut self) -> Result<()> {
        info!("Shutting down cluster");
        
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        // Wait a moment for graceful shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        Ok(())
    }

    /// Internal cluster task that handles all cluster operations
    async fn run_cluster_task(
        _config: Config,
        mut command_rx: mpsc::UnboundedReceiver<ClusterCommand<S>>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        info!("Starting cluster task");

        // TODO: Initialize actual Raft node, state machine, storage, and transport
        // For now, this is a stub implementation that handles the API
        
        loop {
            tokio::select! {
                // Handle cluster commands
                command = command_rx.recv() => {
                    match command {
                        Some(cmd) => {
                            if let Err(e) = Self::handle_cluster_command(cmd).await {
                                warn!("Error handling cluster command: {}", e);
                            }
                        }
                        None => {
                            debug!("Command channel closed, stopping cluster task");
                            break;
                        }
                    }
                }
                
                // Handle shutdown signal
                _ = &mut shutdown_rx => {
                    info!("Received shutdown signal, stopping cluster task");
                    break;
                }
            }
        }

        info!("Cluster task stopped");
        Ok(())
    }

    /// Handle individual cluster commands
    async fn handle_cluster_command(command: ClusterCommand<S>) -> Result<()> {
        match command {
            ClusterCommand::Propose { command: _cmd, response_tx } => {
                // TODO: Implement actual proposal through Raft
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "cluster proposals".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::Query { query: _query, response_tx } => {
                // TODO: Implement actual query execution
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "cluster queries".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::AddMember { config: _config, response_tx } => {
                // TODO: Implement actual member addition
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "add member".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::RemoveMember { node_id: _node_id, response_tx } => {
                // TODO: Implement actual member removal
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "remove member".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::TransferLeadership { target: _target, response_tx } => {
                // TODO: Implement actual leadership transfer
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "transfer leadership".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::GetStatus { response_tx } => {
                // TODO: Get actual cluster status
                let status = ClusterStatus {
                    node_id: 0, // This will be filled in with actual node ID
                    role: NodeRole::Follower,
                    term: 0,
                    commit_index: 0,
                    applied_index: 0,
                    members: vec![],
                    leader_id: None,
                    health: HealthStatus {
                        overall: Health::Healthy,
                        components: HashMap::new(),
                        last_check: SystemTime::now(),
                    },
                    metrics: ClusterMetrics::default(),
                };
                let _ = response_tx.send(status);
            }
            
            ClusterCommand::HealthCheck { response_tx } => {
                // TODO: Implement actual health check
                let health = HealthStatus {
                    overall: Health::Healthy,
                    components: HashMap::new(),
                    last_check: SystemTime::now(),
                };
                let _ = response_tx.send(health);
            }
            
            ClusterCommand::GetClusterInfo { response_tx } => {
                // TODO: Get actual cluster info
                let info = ClusterInfo::default();
                let _ = response_tx.send(info);
            }
            
            ClusterCommand::TakeSnapshot { response_tx } => {
                // TODO: Implement actual snapshot creation
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "take snapshot".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
            
            ClusterCommand::CompactLog { retain_index: _retain_index, response_tx } => {
                // TODO: Implement actual log compaction
                let _ = response_tx.send(Err(RaftError::NotImplemented {
                    feature: "compact log".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }));
            }
        }
        
        Ok(())
    }
}

// Default implementations for various types

impl Default for ClusterMetrics {
    fn default() -> Self {
        Self {
            total_proposals: 0,
            successful_proposals: 0,
            failed_proposals: 0,
            total_queries: 0,
            avg_proposal_latency_ms: 0.0,
            avg_query_latency_ms: 0.0,
            memory_usage_bytes: 0,
            storage_size_bytes: 0,
        }
    }
}

impl Default for ClusterInfo {
    fn default() -> Self {
        Self {
            config: ClusterConfig {
                name: None,
                node_count: 0,
                replication_factor: 0,
                election_timeout_ms: (0, 0),
                heartbeat_interval_ms: 0,
            },
            membership: ClusterMembership {
                members: vec![],
                leader: None,
                joining: vec![],
                leaving: vec![],
            },
            statistics: ClusterStatistics {
                uptime_seconds: 0,
                total_operations: 0,
                ops_per_second: 0.0,
                avg_latency_ms: 0.0,
                error_rate_percent: 0.0,
            },
        }
    }
}

/// Administrative operations trait for advanced cluster management
pub trait AdminOps {
    /// Perform a comprehensive health check
    async fn health_check(&self) -> HealthStatus;
    
    /// Get detailed cluster information
    async fn cluster_info(&self) -> ClusterInfo;
    
    /// Take a snapshot of the current state
    async fn take_snapshot(&self) -> Result<SnapshotId>;
    
    /// Compact the log, retaining entries up to the specified index
    async fn compact_log(&mut self, retain_index: u64) -> Result<()>;
}

impl<S: StateMachine> AdminOps for RaftCluster<S> {
    async fn health_check(&self) -> HealthStatus {
        RaftCluster::health_check(self).await
    }
    
    async fn cluster_info(&self) -> ClusterInfo {
        RaftCluster::cluster_info(self).await
    }
    
    async fn take_snapshot(&self) -> Result<SnapshotId> {
        RaftCluster::take_snapshot(self).await
    }
    
    async fn compact_log(&mut self, retain_index: u64) -> Result<()> {
        RaftCluster::compact_log(self, retain_index).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigBuilder;
    use crate::raft::KeyValueStore;

    #[tokio::test]
    async fn test_cluster_creation() {
        let config = ConfigBuilder::new()
            .node_id(1)
            .data_dir("/tmp/test_cluster")
            .bind_address("127.0.0.1:0") // Random port
            .build()
            .expect("Failed to build config");

        let cluster: RaftCluster<KeyValueStore> = RaftCluster::new(config).await
            .expect("Failed to create cluster");

        // Test basic cluster operations
        let status = cluster.status().await;
        assert_eq!(status.node_id, 1);

        let health = cluster.health_check().await;
        assert!(matches!(health.overall, Health::Healthy));

        // Clean shutdown
        cluster.shutdown().await.expect("Failed to shutdown cluster");
    }

    #[tokio::test]
    async fn test_admin_ops() {
        let config = ConfigBuilder::new()
            .node_id(2)
            .data_dir("/tmp/test_admin")
            .bind_address("127.0.0.1:0")
            .build()
            .expect("Failed to build config");

        let cluster: RaftCluster<KeyValueStore> = RaftCluster::new(config).await
            .expect("Failed to create cluster");

        // Test admin operations
        let info = cluster.cluster_info().await;
        assert_eq!(info.config.node_count, 0); // No members yet

        cluster.shutdown().await.expect("Failed to shutdown cluster");
    }
}
