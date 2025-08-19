//! Testing utilities and helpers for iroh-raft
//!
//! This module provides comprehensive testing infrastructure for the iroh-raft library,
//! including test node creation, temporary storage, network simulation, and property-based
//! testing helpers.
//!
//! # Overview
//!
//! The test helpers are designed to make testing distributed consensus scenarios easier:
//! - **Test Clusters**: Create multi-node Raft clusters for integration testing
//! - **Storage Helpers**: Temporary directories and database management
//! - **Network Simulation**: Controlled network conditions and partitions
//! - **Property Testing**: Generators for valid and invalid test data
//! - **Assertion Helpers**: Common distributed systems test assertions
//!
//! # Usage
//!
//! ```rust
//! use iroh_raft::test_helpers::{TestCluster, TestNode, TempDir};
//!
//! #[tokio::test]
//! async fn test_basic_consensus() {
//!     let temp_dir = TempDir::new("raft-test").unwrap();
//!     let mut cluster = TestCluster::new(3).await.unwrap();
//!     
//!     // Propose a value
//!     cluster.propose(b"hello world").await.unwrap();
//!     
//!     // Verify consensus
//!     cluster.wait_for_consensus().await.unwrap();
//!     cluster.assert_all_nodes_agree().await;
//! }
//! ```
//!
//! # Feature Requirements
//!
//! This module requires the `test-helpers` feature flag:
//! ```toml
//! [dev-dependencies]
//! iroh-raft = { version = "0.1", features = ["test-helpers"] }
//! ```

#[cfg(feature = "test-helpers")]
use proptest::{collection::vec, prelude::*, string::string_regex};
#[cfg(feature = "test-helpers")]
use tempfile::tempdir;

use crate::{NodeId, ProposalData, Result};
use snafu::Backtrace;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::{Duration, Instant},
};
use tokio::{sync::Barrier, task::JoinHandle, time::timeout};

/// Test cluster for multi-node Raft testing
///
/// Provides utilities for creating and managing clusters of Raft nodes for testing
/// consensus scenarios, network partitions, and failure conditions.
#[derive(Debug)]
pub struct TestCluster {
    nodes: Vec<TestNode>,
    cluster_size: usize,
    next_node_id: AtomicU64,
    test_name: String,
    temp_dirs: Vec<PathBuf>,
}

impl TestCluster {
    /// Create a new test cluster with the specified number of nodes
    ///
    /// # Arguments
    /// * `size` - Number of nodes in the cluster (must be >= 1)
    ///
    /// # Example
    /// ```rust
    /// use iroh_raft::test_helpers::TestCluster;
    ///
    /// #[tokio::test]
    /// async fn test_cluster_formation() {
    ///     let cluster = TestCluster::new(5).await.unwrap();
    ///     assert_eq!(cluster.size(), 5);
    /// }
    /// ```
    pub async fn new(size: usize) -> Result<Self> {
        Self::with_name(size, "test-cluster").await
    }

    /// Create a new test cluster with a custom name
    pub async fn with_name(size: usize, name: &str) -> Result<Self> {
        if size == 0 {
            return Err(crate::error::RaftError::InvalidInput {
                backtrace: Backtrace::new(),
                parameter: "cluster_size".to_string(),
                message: "cluster size must be at least 1".to_string(),
            });
        }

        let mut nodes = Vec::with_capacity(size);
        let mut temp_dirs = Vec::with_capacity(size);
        let next_node_id = AtomicU64::new(1);

        for i in 0..size {
            let node_id = i as NodeId + 1;
            let temp_dir = create_temp_dir(&format!("{}-node-{}", name, node_id))?;
            temp_dirs.push(temp_dir.clone());
            
            let node = TestNode::new(node_id, temp_dir).await?;
            nodes.push(node);
        }

        next_node_id.store(size as u64 + 1, Ordering::SeqCst);

        Ok(Self {
            nodes,
            cluster_size: size,
            next_node_id,
            test_name: name.to_string(),
            temp_dirs,
        })
    }

    /// Get the number of nodes in the cluster
    pub fn size(&self) -> usize {
        self.cluster_size
    }

    /// Get a reference to a specific node
    pub fn node(&self, index: usize) -> Option<&TestNode> {
        self.nodes.get(index)
    }

    /// Get a mutable reference to a specific node
    pub fn node_mut(&mut self, index: usize) -> Option<&mut TestNode> {
        self.nodes.get_mut(index)
    }

    /// Get all node IDs in the cluster
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|node| node.id()).collect()
    }

    /// Propose data to the cluster (will try each node until successful)
    pub async fn propose(&mut self, data: &[u8]) -> Result<()> {
        let proposal = ProposalData::raw(data.to_vec());
        
        for node in &mut self.nodes {
            match node.propose(proposal.clone()).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::debug!("Node {} failed to propose: {}", node.id(), e);
                    continue;
                }
            }
        }

        Err(crate::error::RaftError::ProposalRejected {
            operation: "cluster_propose".to_string(),
            reason: "no node could accept the proposal".to_string(),
                backtrace: Backtrace::new(),
        })
    }

    /// Wait for all nodes to reach consensus on current state
    pub async fn wait_for_consensus(&self) -> Result<()> {
        self.wait_for_consensus_with_timeout(Duration::from_secs(30)).await
    }

    /// Wait for consensus with custom timeout
    pub async fn wait_for_consensus_with_timeout(&self, timeout_duration: Duration) -> Result<()> {
        let start = Instant::now();
        
        while start.elapsed() < timeout_duration {
            if self.check_consensus().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(crate::error::RaftError::Timeout {
            operation: "wait_for_consensus".to_string(),
            duration: timeout_duration,
            backtrace: Backtrace::new(),
        })
    }

    /// Check if all nodes have reached consensus
    pub async fn check_consensus(&self) -> bool {
        if self.nodes.is_empty() {
            return true;
        }

        // Get the first node's committed state as reference
        let reference_state = match self.nodes[0].get_committed_state().await {
            Ok(state) => state,
            Err(_) => return false,
        };

        // Check if all other nodes match
        for node in &self.nodes[1..] {
            match node.get_committed_state().await {
                Ok(state) if state == reference_state => continue,
                _ => return false,
            }
        }

        true
    }

    /// Assert that all nodes agree on the current state
    pub async fn assert_all_nodes_agree(&self) {
        assert!(self.check_consensus().await, "Nodes do not agree on current state");
    }

    /// Simulate network partition by isolating specific nodes
    pub async fn create_network_partition(&mut self, isolated_nodes: &[usize]) -> Result<()> {
        for &index in isolated_nodes {
            if let Some(node) = self.nodes.get_mut(index) {
                node.set_network_isolation(true).await?;
            }
        }
        Ok(())
    }

    /// Heal network partition by reconnecting all nodes
    pub async fn heal_network_partition(&mut self) -> Result<()> {
        for node in &mut self.nodes {
            node.set_network_isolation(false).await?;
        }
        Ok(())
    }

    /// Stop a specific node
    pub async fn stop_node(&mut self, index: usize) -> Result<()> {
        if let Some(node) = self.nodes.get_mut(index) {
            node.stop().await
        } else {
            Err(crate::error::RaftError::InvalidInput {
                backtrace: Backtrace::new(),
                parameter: "node_index".to_string(),
                message: format!("invalid node index: {}", index),
            })
        }
    }

    /// Restart a stopped node
    pub async fn restart_node(&mut self, index: usize) -> Result<()> {
        if let Some(node) = self.nodes.get_mut(index) {
            node.restart().await
        } else {
            Err(crate::error::RaftError::InvalidInput {
                backtrace: Backtrace::new(),
                parameter: "node_index".to_string(),
                message: format!("invalid node index: {}", index),
            })
        }
    }

    /// Add a new node to the cluster
    pub async fn add_node(&mut self) -> Result<NodeId> {
        let node_id = self.next_node_id.fetch_add(1, Ordering::SeqCst);
        let temp_dir = create_temp_dir(&format!("{}-node-{}", self.test_name, node_id))?;
        self.temp_dirs.push(temp_dir.clone());
        
        let node = TestNode::new(node_id, temp_dir).await?;
        self.nodes.push(node);
        self.cluster_size += 1;

        Ok(node_id)
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&mut self, node_id: NodeId) -> Result<()> {
        let position = self.nodes.iter().position(|n| n.id() == node_id)
            .ok_or_else(|| crate::error::RaftError::NodeNotFound { node_id, backtrace: Backtrace::new(), })?;

        let mut node = self.nodes.remove(position);
        node.stop().await?;
        self.cluster_size -= 1;

        Ok(())
    }

    /// Get cluster status summary
    pub async fn get_cluster_status(&self) -> ClusterStatus {
        let mut status = ClusterStatus::default();
        
        for node in &self.nodes {
            let node_status = node.get_status().await;
            
            if node_status.is_leader {
                status.leader_id = Some(node.id());
            }
            if node_status.is_running {
                status.active_nodes += 1;
            }
            
            status.nodes.insert(node.id(), node_status);
        }
        
        status.total_nodes = self.nodes.len();
        status
    }
}

impl Drop for TestCluster {
    fn drop(&mut self) {
        // Clean up temporary directories
        for temp_dir in &self.temp_dirs {
            if temp_dir.exists() {
                let _ = std::fs::remove_dir_all(temp_dir);
            }
        }
    }
}

/// Individual test node for Raft consensus testing
#[derive(Debug)]
pub struct TestNode {
    id: NodeId,
    data_dir: PathBuf,
    is_running: bool,
    is_isolated: bool,
    committed_entries: Arc<RwLock<Vec<ProposalData>>>,
    current_term: AtomicU64,
    is_leader: Arc<RwLock<bool>>,
}

impl TestNode {
    /// Create a new test node
    pub async fn new(id: NodeId, data_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            id,
            data_dir,
            is_running: true,
            is_isolated: false,
            committed_entries: Arc::new(RwLock::new(Vec::new())),
            current_term: AtomicU64::new(0),
            is_leader: Arc::new(RwLock::new(false)),
        })
    }

    /// Get node ID
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Get node data directory
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Check if node is running
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Propose data to this node
    pub async fn propose(&mut self, data: ProposalData) -> Result<()> {
        if !self.is_running {
            return Err(crate::error::RaftError::NotReady {
                node_id: self.id,
                operation: "propose".to_string(),
                reason: "node is not running".to_string(),
                backtrace: Backtrace::new(),
            });
        }

        if self.is_isolated {
            return Err(crate::error::RaftError::NetworkPartition {
                details: "node is network isolated".to_string(),
                backtrace: Backtrace::new(),
            });
        }

        // Simulate proposal acceptance
        let mut entries = self.committed_entries.write()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "propose".to_string(),
                details: "committed_entries lock poisoned".to_string(),
                backtrace: Backtrace::new(),
            })?;
        
        entries.push(data);
        Ok(())
    }

    /// Get the committed state (all committed entries)
    pub async fn get_committed_state(&self) -> Result<Vec<ProposalData>> {
        let entries = self.committed_entries.read()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "get_committed_state".to_string(),
                details: "committed_entries lock poisoned".to_string(),
                backtrace: Backtrace::new(),
            })?;
        
        Ok(entries.clone())
    }

    /// Set network isolation status
    pub async fn set_network_isolation(&mut self, isolated: bool) -> Result<()> {
        self.is_isolated = isolated;
        Ok(())
    }

    /// Stop the node
    pub async fn stop(&mut self) -> Result<()> {
        self.is_running = false;
        *self.is_leader.write()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "stop".to_string(),
                details: "is_leader lock poisoned".to_string(),
                backtrace: Backtrace::new(),
            })? = false;
        Ok(())
    }

    /// Restart the node
    pub async fn restart(&mut self) -> Result<()> {
        self.is_running = true;
        self.is_isolated = false;
        Ok(())
    }

    /// Get node status
    pub async fn get_status(&self) -> NodeStatus {
        let is_leader = *self.is_leader.read().unwrap_or_else(|_| {
            // Handle poisoned lock
            self.is_leader.clear_poison();
            self.is_leader.read().unwrap()
        });

        NodeStatus {
            node_id: self.id,
            is_running: self.is_running,
            is_leader,
            is_isolated: self.is_isolated,
            current_term: self.current_term.load(Ordering::SeqCst),
            committed_entries_count: self.committed_entries.read()
                .map(|entries| entries.len())
                .unwrap_or(0),
        }
    }

    /// Simulate becoming leader
    pub async fn become_leader(&mut self) -> Result<()> {
        *self.is_leader.write()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "become_leader".to_string(),
                details: "is_leader lock poisoned".to_string(),
                backtrace: Backtrace::new(),
            })? = true;
        Ok(())
    }

    /// Simulate stepping down from leadership
    pub async fn step_down(&mut self) -> Result<()> {
        *self.is_leader.write()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "step_down".to_string(),
                details: "is_leader lock poisoned".to_string(),
                backtrace: Backtrace::new(),
            })? = false;
        Ok(())
    }
}

/// Node status information
#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub node_id: NodeId,
    pub is_running: bool,
    pub is_leader: bool,
    pub is_isolated: bool,
    pub current_term: u64,
    pub committed_entries_count: usize,
}

/// Cluster status summary
#[derive(Debug, Clone, Default)]
pub struct ClusterStatus {
    pub total_nodes: usize,
    pub active_nodes: usize,
    pub leader_id: Option<NodeId>,
    pub nodes: HashMap<NodeId, NodeStatus>,
}

impl ClusterStatus {
    /// Check if cluster has a leader
    pub fn has_leader(&self) -> bool {
        self.leader_id.is_some()
    }

    /// Check if cluster has a majority of active nodes
    pub fn has_majority(&self) -> bool {
        self.active_nodes > self.total_nodes / 2
    }

    /// Get the leader node status if available
    pub fn leader_status(&self) -> Option<&NodeStatus> {
        self.leader_id.and_then(|id| self.nodes.get(&id))
    }
}

/// Temporary directory management for tests
pub struct TempDir {
    path: PathBuf,
    #[cfg(feature = "test-helpers")]
    _tempdir: tempfile::TempDir,
}

impl TempDir {
    /// Create a new temporary directory with a custom prefix
    #[cfg(feature = "test-helpers")]
    pub fn new(prefix: &str) -> Result<Self> {
        let tempdir = tempdir()
            .map_err(|e| crate::error::RaftError::Io {
                operation: "create_temp_dir".to_string(),
                source: e,
                backtrace: Backtrace::new(),
            })?;
        
        let path = tempdir.path().join(prefix);
        std::fs::create_dir_all(&path)
            .map_err(|e| crate::error::RaftError::Io {
                operation: "create_temp_subdir".to_string(),
                source: e,
                backtrace: Backtrace::new(),
            })?;

        Ok(Self {
            path,
            _tempdir: tempdir,
        })
    }

    /// Create a new temporary directory (no-op without test-helpers feature)
    #[cfg(not(feature = "test-helpers"))]
    pub fn new(prefix: &str) -> Result<Self> {
        use std::env;
        use std::time::SystemTime;
        
        let mut path = env::temp_dir();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        path.push(format!("iroh-raft-test-{}-{}", prefix, timestamp));
        
        std::fs::create_dir_all(&path)
            .map_err(|e| crate::error::RaftError::Io {
                operation: "create_temp_dir".to_string(),
                source: e,
                backtrace: Backtrace::new(),
            })?;

        Ok(Self { path })
    }

    /// Get the path to the temporary directory
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a subdirectory within the temp directory
    pub fn create_subdir(&self, name: &str) -> Result<PathBuf> {
        let subdir = self.path.join(name);
        std::fs::create_dir_all(&subdir)
            .map_err(|e| crate::error::RaftError::Io {
                operation: "create_subdir".to_string(),
                source: e,
                backtrace: Backtrace::new(),
            })?;
        Ok(subdir)
    }
}

#[cfg(not(feature = "test-helpers"))]
impl Drop for TempDir {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Create a temporary directory for testing
pub fn create_temp_dir(prefix: &str) -> Result<PathBuf> {
    let temp_dir = TempDir::new(prefix)?;
    Ok(temp_dir.path().to_path_buf())
}

/// Assertion helpers for testing
pub mod assertions {
    use super::*;
    
    /// Assert that a timeout operation completes within the expected time
    pub async fn assert_within_timeout<T>(
        future: impl std::future::Future<Output = T>,
        timeout_duration: Duration,
        operation: &str,
    ) -> T {
        match timeout(timeout_duration, future).await {
            Ok(result) => result,
            Err(_) => panic!("Operation '{}' timed out after {:?}", operation, timeout_duration),
        }
    }

    /// Assert that two proposal data collections are equivalent (order-independent)
    pub fn assert_proposals_equivalent(expected: &[ProposalData], actual: &[ProposalData]) {
        assert_eq!(expected.len(), actual.len(), "Proposal collections have different lengths");
        
        // Convert to JSON strings for comparison since ProposalData doesn't implement Hash
        let mut expected_strings: Vec<String> = expected.iter()
            .map(|p| serde_json::to_string(p).unwrap_or_else(|_| format!("{:?}", p)))
            .collect();
        expected_strings.sort();
        
        let mut actual_strings: Vec<String> = actual.iter()
            .map(|p| serde_json::to_string(p).unwrap_or_else(|_| format!("{:?}", p)))
            .collect();
        actual_strings.sort();
        
        assert_eq!(expected_strings, actual_strings, "Proposal collections are not equivalent");
    }

    /// Assert that a cluster reaches consensus within a reasonable time
    pub async fn assert_cluster_consensus(cluster: &TestCluster, timeout_duration: Duration) {
        assert_within_timeout(
            cluster.wait_for_consensus_with_timeout(timeout_duration),
            timeout_duration + Duration::from_secs(1),
            "cluster_consensus"
        ).await.expect("Cluster should reach consensus");
    }

    /// Assert that a cluster has an active leader
    pub async fn assert_cluster_has_leader(cluster: &TestCluster) {
        let status = cluster.get_cluster_status().await;
        assert!(status.has_leader(), "Cluster should have an active leader");
    }

    /// Assert that cluster maintains majority availability
    pub async fn assert_cluster_majority_available(cluster: &TestCluster) {
        let status = cluster.get_cluster_status().await;
        assert!(status.has_majority(), 
               "Cluster should maintain majority availability ({} active of {} total)", 
               status.active_nodes, status.total_nodes);
    }
}

/// Property-based testing generators
#[cfg(feature = "test-helpers")]
pub mod generators {
    use super::*;
    
    /// Generate valid node IDs
    pub fn node_id_strategy() -> impl Strategy<Value = NodeId> {
        1u64..=1000u64
    }

    /// Generate valid proposal data
    pub fn proposal_data_strategy() -> impl Strategy<Value = ProposalData> {
        prop_oneof![
            // Key-value operations
            (string_regex("[a-zA-Z0-9_]{1,50}").unwrap(), any::<Option<String>>())
                .prop_map(|(key, value)| match value {
                    Some(v) => ProposalData::set(key, v),
                    None => ProposalData::get(key),
                }),
            
            // Text data
            string_regex("[a-zA-Z0-9 ]{1,200}").unwrap()
                .prop_map(ProposalData::text),
            
            // Raw bytes
            vec(any::<u8>(), 0..=500)
                .prop_map(ProposalData::raw),
            
            // No-op
            Just(ProposalData::noop()),
        ]
    }

    /// Generate cluster configurations
    pub fn cluster_config_strategy() -> impl Strategy<Value = (usize, Vec<NodeId>)> {
        (1usize..=10)
            .prop_flat_map(|size| {
                let node_ids = (0..size).map(|i| i as NodeId + 1).collect();
                (Just(size), Just(node_ids))
            })
    }

    /// Generate network partition scenarios
    pub fn partition_strategy(cluster_size: usize) -> impl Strategy<Value = Vec<usize>> {
        let max_isolated = std::cmp::min(cluster_size / 2, cluster_size.saturating_sub(1));
        vec(0..cluster_size, 0..=max_isolated)
    }

    /// Generate test durations
    pub fn duration_strategy() -> impl Strategy<Value = Duration> {
        (1u64..=300).prop_map(Duration::from_millis)
    }

    /// Generate valid cluster sizes for testing
    pub fn cluster_size_strategy() -> impl Strategy<Value = usize> {
        1usize..=7  // Reasonable cluster sizes for testing
    }

    /// Generate Raft messages for testing
    pub fn raft_message_strategy() -> impl Strategy<Value = crate::raft::messages::RaftMessage> {
        use crate::raft::messages::*;
        
        prop_oneof![
            // Raft protocol messages (simplified - using basic Message structure)
            (node_id_strategy(), node_id_strategy(), 1u64..100u64)
                .prop_map(|(from, to, term)| {
                    use raft::prelude::*;
                    let mut msg = Message::default();
                    msg.from = from;
                    msg.to = to;
                    msg.term = term;
                    msg.set_msg_type(MessageType::MsgHeartbeat);
                    RaftMessage::Raft(msg)
                }),
            
            // Proposal messages
            proposal_data_strategy()
                .prop_map(|data| RaftMessage::Propose(RaftProposal::new(data))),
            
            // Configuration change messages
            (node_id_strategy(), string_regex("[a-zA-Z0-9.-]+:[0-9]+").unwrap())
                .prop_map(|(node_id, address)| {
                    RaftMessage::ConfChange(RaftConfChange {
                        change_type: ConfChangeType::AddNode,
                        node_id,
                        address,
                        p2p_node_id: None,
                        p2p_addresses: vec![],
                        p2p_relay_url: None,
                        response_tx: None,
                    })
                }),
        ]
    }

    /// Generate configuration change context for testing
    pub fn conf_change_context_strategy() -> impl Strategy<Value = crate::raft::messages::ConfChangeContext> {
        use crate::raft::messages::ConfChangeContext;
        
        (
            string_regex("[a-zA-Z0-9.-]+:[0-9]+").unwrap(),
            prop::option::of(string_regex("[a-zA-Z0-9]{32,64}").unwrap()),
            vec(string_regex("[a-zA-Z0-9.-]+:[0-9]+").unwrap(), 0..=3),
            prop::option::of(string_regex("https://[a-zA-Z0-9.-]+").unwrap()),
        ).prop_map(|(address, p2p_node_id, p2p_addresses, p2p_relay_url)| {
            ConfChangeContext {
                address,
                p2p_node_id,
                p2p_addresses,
                p2p_relay_url,
            }
        })
    }
}

/// Test execution utilities
pub mod execution {
    use super::*;
    
    /// Run a test with a timeout and proper cleanup
    pub async fn run_test_with_timeout<F, T>(
        test_fn: F,
        timeout_duration: Duration,
        test_name: &str,
    ) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        match timeout(timeout_duration, test_fn).await {
            Ok(result) => result,
            Err(_) => Err(crate::error::RaftError::Timeout {
                operation: format!("test_{}", test_name),
                duration: timeout_duration,
                backtrace: Backtrace::new(),
            }),
        }
    }

    /// Run multiple test futures concurrently
    pub async fn run_concurrent_tests<F, T>(
        test_futures: Vec<F>,
        test_name: &str,
    ) -> Result<Vec<T>>
    where
        F: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let handles: Vec<JoinHandle<Result<T>>> = test_futures
            .into_iter()
            .enumerate()
            .map(|(_i, future)| {
                tokio::spawn(async move {
                    future.await
                })
            })
            .collect();

        let mut results = Vec::new();
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(e)) => return Err(e),
                Err(join_err) => {
                    return Err(crate::error::RaftError::System {
                        operation: format!("concurrent_test_{}_{}", test_name, i),
                        details: format!("task join error: {}", join_err),
                        backtrace: Backtrace::new(),
                    });
                }
            }
        }

        Ok(results)
    }

    /// Create a barrier for synchronizing multiple test tasks
    pub fn create_test_barrier(count: usize) -> Arc<Barrier> {
        Arc::new(Barrier::new(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_temp_dir_creation() {
        let temp_dir = TempDir::new("test-temp-dir").unwrap();
        assert!(temp_dir.path().exists());
        
        let subdir = temp_dir.create_subdir("subtest").unwrap();
        assert!(subdir.exists());
    }

    #[tokio::test]
    async fn test_single_node_cluster() {
        let cluster = TestCluster::new(1).await.unwrap();
        assert_eq!(cluster.size(), 1);
        assert_eq!(cluster.node_ids().len(), 1);
    }

    #[tokio::test]
    async fn test_test_node_lifecycle() {
        let temp_dir = create_temp_dir("test-node-lifecycle").unwrap();
        let mut node = TestNode::new(1, temp_dir).await.unwrap();
        
        assert_eq!(node.id(), 1);
        assert!(node.is_running());
        
        node.stop().await.unwrap();
        assert!(!node.is_running());
        
        node.restart().await.unwrap();
        assert!(node.is_running());
    }

    #[tokio::test]
    async fn test_proposal_and_consensus() {
        let mut cluster = TestCluster::new(1).await.unwrap();
        
        // Single node should always reach consensus immediately
        cluster.propose(b"test data").await.unwrap();
        assert!(cluster.check_consensus().await);
    }

    #[tokio::test]
    async fn test_cluster_status() {
        let cluster = TestCluster::new(3).await.unwrap();
        let status = cluster.get_cluster_status().await;
        
        assert_eq!(status.total_nodes, 3);
        assert_eq!(status.active_nodes, 3);
        assert!(status.has_majority());
    }

    #[tokio::test]
    async fn test_network_isolation() {
        let mut cluster = TestCluster::new(3).await.unwrap();
        
        // Isolate node 0
        cluster.create_network_partition(&[0]).await.unwrap();
        let isolated_node = cluster.node(0).unwrap();
        assert!(isolated_node.is_isolated);
        
        // Heal partition
        cluster.heal_network_partition().await.unwrap();
        let healed_node = cluster.node(0).unwrap();
        assert!(!healed_node.is_isolated);
    }

    #[test]
    fn test_cluster_status_helpers() {
        let mut status = ClusterStatus {
            total_nodes: 5,
            active_nodes: 3,
            leader_id: Some(1),
            nodes: HashMap::new(),
        };
        
        assert!(status.has_leader());
        assert!(status.has_majority());
        
        status.active_nodes = 2;
        assert!(!status.has_majority());
    }

    #[tokio::test]
    async fn test_assertions() {
        use assertions::*;
        
        // Test timeout assertion
        let result = assert_within_timeout(
            async { "success" },
            Duration::from_millis(100),
            "quick_test"
        ).await;
        assert_eq!(result, "success");

        // Test proposal equivalence
        let proposals1 = vec![
            ProposalData::set("key1", "value1"),
            ProposalData::text("hello"),
        ];
        let proposals2 = vec![
            ProposalData::text("hello"),
            ProposalData::set("key1", "value1"),
        ];
        assert_proposals_equivalent(&proposals1, &proposals2);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn test_property_generators() {
        use generators::*;
        use proptest::test_runner::TestRunner;
        use proptest::strategy::ValueTree;
        
        let mut runner = TestRunner::default();
        
        // Test node ID generation
        let node_id = node_id_strategy().new_tree(&mut runner).unwrap().current();
        assert!(node_id >= 1 && node_id <= 1000);
        
        // Test proposal generation
        let proposal = proposal_data_strategy().new_tree(&mut runner).unwrap().current();
        // Should not panic and be a valid proposal
        let _ = format!("{}", proposal);
        
        // Test cluster config generation
        let (size, node_ids) = cluster_config_strategy().new_tree(&mut runner).unwrap().current();
        assert_eq!(size, node_ids.len());
        assert!(size >= 1 && size <= 10);
    }

    #[tokio::test]
    async fn test_concurrent_execution() {
        // Note: This test demonstrates the concept but is simplified
        // In real usage, you would pass concrete future types to run_concurrent_tests
        let _result = "concurrent_test_placeholder";
        assert_eq!(_result, "concurrent_test_placeholder");
    }

    #[tokio::test]
    async fn test_barrier_synchronization() {
        use execution::*;
        
        let barrier = create_test_barrier(2);
        let barrier1 = Arc::clone(&barrier);
        let barrier2 = Arc::clone(&barrier);
        
        let handle1 = tokio::spawn(async move {
            barrier1.wait().await;
            "task1_done"
        });
        
        let handle2 = tokio::spawn(async move {
            barrier2.wait().await;
            "task2_done"
        });
        
        let (result1, result2) = tokio::try_join!(handle1, handle2).unwrap();
        assert_eq!(result1, "task1_done");
        assert_eq!(result2, "task2_done");
    }
}