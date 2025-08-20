//! Actor pattern implementations for lock-free concurrency
//!
//! This module provides actor-based replacements for shared state management
//! to eliminate lock contention in hot paths.

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use std::collections::HashMap;
use crate::types::{NodeStatus, NodeRole, NodeId};
use crate::error::{RaftError, Result};

/// Commands for the Node Status Actor
#[derive(Debug)]
pub enum NodeStatusCommand {
    GetStatus {
        response: oneshot::Sender<NodeStatus>,
    },
    UpdateStatus {
        status: NodeStatus,
        response: oneshot::Sender<()>,
    },
    UpdateRole {
        role: NodeRole,
        term: u64,
        leader_id: Option<NodeId>,
        response: oneshot::Sender<()>,
    },
    UpdateIndexes {
        last_log_index: u64,
        committed_index: u64,
        applied_index: u64,
        response: oneshot::Sender<()>,
    },
}

/// Lock-free Node Status Actor
pub struct NodeStatusActor {
    status: NodeStatus,
    receiver: mpsc::Receiver<NodeStatusCommand>,
}

impl NodeStatusActor {
    pub fn new(initial_status: NodeStatus) -> (Self, NodeStatusHandle) {
        let (sender, receiver) = mpsc::channel(1000); // Bounded for backpressure
        
        let actor = Self {
            status: initial_status,
            receiver,
        };
        
        let handle = NodeStatusHandle { sender };
        
        (actor, handle)
    }
    
    pub async fn run(mut self) {
        while let Some(cmd) = self.receiver.recv().await {
            match cmd {
                NodeStatusCommand::GetStatus { response } => {
                    let _ = response.send(self.status.clone());
                }
                NodeStatusCommand::UpdateStatus { status, response } => {
                    self.status = status;
                    let _ = response.send(());
                }
                NodeStatusCommand::UpdateRole { role, term, leader_id, response } => {
                    self.status.role = role;
                    self.status.term = term;
                    self.status.leader_id = leader_id;
                    let _ = response.send(());
                }
                NodeStatusCommand::UpdateIndexes { 
                    last_log_index, 
                    committed_index, 
                    applied_index, 
                    response 
                } => {
                    self.status.last_log_index = last_log_index;
                    self.status.committed_index = committed_index;
                    self.status.applied_index = applied_index;
                    let _ = response.send(());
                }
            }
        }
    }
}

/// Handle for communicating with NodeStatusActor
#[derive(Clone)]
pub struct NodeStatusHandle {
    sender: mpsc::Sender<NodeStatusCommand>,
}

impl NodeStatusHandle {
    /// Get current status with timeout
    pub async fn get_status(&self) -> Result<NodeStatus> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(NodeStatusCommand::GetStatus { 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Node status actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "get node status".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Node status response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Update status with timeout
    pub async fn update_status(&self, status: NodeStatus) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(NodeStatusCommand::UpdateStatus { 
            status, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Node status actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "update node status".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Node status response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Update role-specific fields with timeout
    pub async fn update_role(&self, role: NodeRole, term: u64, leader_id: Option<NodeId>) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(NodeStatusCommand::UpdateRole { 
            role, 
            term, 
            leader_id, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Node status actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "update node role".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Node role response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Update index fields with timeout
    pub async fn update_indexes(
        &self, 
        last_log_index: u64, 
        committed_index: u64, 
        applied_index: u64
    ) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(NodeStatusCommand::UpdateIndexes { 
            last_log_index, 
            committed_index, 
            applied_index, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Node status actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "update node indexes".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Node indexes response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
}

/// Commands for the Connection Pool Actor
#[derive(Debug)]
pub enum ConnectionPoolCommand {
    GetConnection {
        peer_id: u64,
        response: oneshot::Sender<Option<iroh::endpoint::Connection>>,
    },
    AddConnection {
        peer_id: u64,
        connection: iroh::endpoint::Connection,
        response: oneshot::Sender<()>,
    },
    RemoveConnection {
        peer_id: u64,
        response: oneshot::Sender<Option<iroh::endpoint::Connection>>,
    },
    HealthCheck {
        response: oneshot::Sender<Vec<u64>>, // Returns stale connection IDs
    },
    GetMetrics {
        response: oneshot::Sender<ConnectionPoolMetrics>,
    },
}

/// Connection pool metrics
#[derive(Debug, Clone)]
pub struct ConnectionPoolMetrics {
    pub active_connections: usize,
    pub total_created: u64,
    pub total_removed: u64,
    pub health_check_failures: u64,
}

/// Lock-free Connection Pool Actor
pub struct ConnectionPoolActor {
    connections: HashMap<u64, (iroh::endpoint::Connection, std::time::Instant)>,
    metrics: ConnectionPoolMetrics,
    receiver: mpsc::Receiver<ConnectionPoolCommand>,
    _health_check_timeout: Duration,
}

impl ConnectionPoolActor {
    pub fn new(health_check_timeout: Duration) -> (Self, ConnectionPoolHandle) {
        let (sender, receiver) = mpsc::channel(1000);
        
        let actor = Self {
            connections: HashMap::new(),
            metrics: ConnectionPoolMetrics {
                active_connections: 0,
                total_created: 0,
                total_removed: 0,
                health_check_failures: 0,
            },
            receiver,
            _health_check_timeout: health_check_timeout,
        };
        
        let handle = ConnectionPoolHandle { sender };
        
        (actor, handle)
    }
    
    pub async fn run(mut self) {
        let mut health_check_interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                Some(cmd) = self.receiver.recv() => {
                    self.handle_command(cmd).await;
                }
                _ = health_check_interval.tick() => {
                    self.perform_health_check().await;
                }
            }
        }
    }
    
    async fn handle_command(&mut self, cmd: ConnectionPoolCommand) {
        match cmd {
            ConnectionPoolCommand::GetConnection { peer_id, response } => {
                let connection = self.connections.get(&peer_id)
                    .map(|(conn, _)| conn.clone());
                let _ = response.send(connection);
            }
            ConnectionPoolCommand::AddConnection { peer_id, connection, response } => {
                self.connections.insert(peer_id, (connection, std::time::Instant::now()));
                self.metrics.active_connections = self.connections.len();
                self.metrics.total_created += 1;
                let _ = response.send(());
            }
            ConnectionPoolCommand::RemoveConnection { peer_id, response } => {
                let removed = self.connections.remove(&peer_id)
                    .map(|(conn, _)| conn);
                if removed.is_some() {
                    self.metrics.active_connections = self.connections.len();
                    self.metrics.total_removed += 1;
                }
                let _ = response.send(removed);
            }
            ConnectionPoolCommand::HealthCheck { response } => {
                let stale_ids = self.get_stale_connections();
                let _ = response.send(stale_ids);
            }
            ConnectionPoolCommand::GetMetrics { response } => {
                self.metrics.active_connections = self.connections.len();
                let _ = response.send(self.metrics.clone());
            }
        }
    }
    
    async fn perform_health_check(&mut self) {
        let now = std::time::Instant::now();
        let stale_timeout = Duration::from_secs(300); // 5 minutes
        
        let stale_connections: Vec<u64> = self.connections
            .iter()
            .filter(|(_, (_, last_activity))| now.duration_since(*last_activity) > stale_timeout)
            .map(|(peer_id, _)| *peer_id)
            .collect();
        
        for peer_id in stale_connections {
            self.connections.remove(&peer_id);
            self.metrics.health_check_failures += 1;
        }
        
        self.metrics.active_connections = self.connections.len();
    }
    
    fn get_stale_connections(&self) -> Vec<u64> {
        let now = std::time::Instant::now();
        let stale_timeout = Duration::from_secs(300);
        
        self.connections
            .iter()
            .filter(|(_, (_, last_activity))| now.duration_since(*last_activity) > stale_timeout)
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }
}

/// Handle for communicating with ConnectionPoolActor
#[derive(Clone)]
pub struct ConnectionPoolHandle {
    sender: mpsc::Sender<ConnectionPoolCommand>,
}

impl ConnectionPoolHandle {
    /// Get connection with timeout
    pub async fn get_connection(&self, peer_id: u64) -> Result<Option<iroh::endpoint::Connection>> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(ConnectionPoolCommand::GetConnection { 
            peer_id, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Connection pool actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "get connection".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Get connection response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Add connection with timeout
    pub async fn add_connection(&self, peer_id: u64, connection: iroh::endpoint::Connection) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(ConnectionPoolCommand::AddConnection { 
            peer_id, 
            connection, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Connection pool actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "add connection".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Add connection response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Remove connection with timeout
    pub async fn remove_connection(&self, peer_id: u64) -> Result<Option<iroh::endpoint::Connection>> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(ConnectionPoolCommand::RemoveConnection { 
            peer_id, 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Connection pool actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "remove connection".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Remove connection response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
    
    /// Get pool metrics with timeout
    pub async fn get_metrics(&self) -> Result<ConnectionPoolMetrics> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.sender.send(ConnectionPoolCommand::GetMetrics { 
            response: response_tx 
        }).await.map_err(|_| RaftError::Internal { 
            message: "Connection pool actor is closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        timeout(Duration::from_millis(100), response_rx)
            .await
            .map_err(|_| RaftError::Timeout { 
                operation: "get connection metrics".to_string(), 
                duration: Duration::from_millis(100),
                backtrace: snafu::Backtrace::new(),
            })?
            .map_err(|_| RaftError::Internal { 
                message: "Get metrics response channel closed".to_string(),
                backtrace: snafu::Backtrace::new(),
            })
    }
}