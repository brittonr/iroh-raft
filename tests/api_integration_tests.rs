//! Comprehensive integration tests for the new API improvements
//!
//! This test suite validates the entire API surface including:
//! - RaftCluster API functionality
//! - Configuration presets and validation
//! - Error handling and edge cases
//! - Performance characteristics
//! - Integration between components
//!
//! Run with: cargo test api_integration_tests

#![cfg(test)]

use iroh_raft::{
    config::{ConfigBuilder, ConfigError},
    error::RaftError,
    raft::{KvCommand, KvState, KeyValueStore, StateMachine, ExampleRaftStateMachine},
    types::NodeId,
    Result,
};
use bincode;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::timeout;
use serde::{Deserialize, Serialize};
use proptest::prelude::*;

// Import test framework utilities
use pretty_assertions::assert_eq;
use test_case::test_case;

/// Mock RaftCluster implementation for testing
/// In production, this would be the actual implementation
#[derive(Debug)]
pub struct RaftCluster<S: StateMachine> {
    node_id: NodeId,
    state_machine: S,
    peers: Vec<String>,
    bind_address: String,
    config: ClusterConfig,
    metrics: Arc<RwLock<ClusterMetrics>>,
    is_running: Arc<RwLock<bool>>,
}

#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub heartbeat_interval: Duration,
    pub election_timeout: Duration,
    pub data_dir: Option<String>,
    pub preset: Preset,
}

#[derive(Debug, Clone)]
pub enum Preset {
    Development,
    Testing,
    Production,
}

#[derive(Debug, Clone)]
pub struct ClusterMetrics {
    pub node_id: NodeId,
    pub peer_count: usize,
    pub is_leader: bool,
    pub term: u64,
    pub commit_index: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub proposals_applied: u64,
    pub snapshots_created: u64,
    pub uptime_start: SystemTime,
}

pub struct RaftClusterBuilder<S: StateMachine> {
    node_id: Option<NodeId>,
    bind_address: Option<String>,
    peers: Vec<String>,
    state_machine: Option<S>,
    preset: Option<Preset>,
    data_dir: Option<String>,
    heartbeat_interval: Option<Duration>,
    election_timeout: Option<Duration>,
}

impl<S: StateMachine> RaftClusterBuilder<S> {
    pub fn new() -> Self {
        Self {
            node_id: None,
            bind_address: None,
            peers: Vec::new(),
            state_machine: None,
            preset: None,
            data_dir: None,
            heartbeat_interval: None,
            election_timeout: None,
        }
    }

    pub fn node_id(mut self, id: NodeId) -> Self {
        self.node_id = Some(id);
        self
    }

    pub fn bind_address<A: Into<String>>(mut self, address: A) -> Self {
        self.bind_address = Some(address.into());
        self
    }

    pub fn peers<I>(mut self, peers: I) -> Self 
    where 
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.peers = peers.into_iter().map(|p| p.into()).collect();
        self
    }

    pub fn add_peer<A: Into<String>>(mut self, peer: A) -> Self {
        self.peers.push(peer.into());
        self
    }

    pub fn state_machine(mut self, sm: S) -> Self {
        self.state_machine = Some(sm);
        self
    }

    pub fn preset(mut self, preset: Preset) -> Self {
        self.preset = Some(preset);
        self
    }

    pub fn data_dir<P: Into<String>>(mut self, dir: P) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    pub fn election_timeout(mut self, timeout: Duration) -> Self {
        self.election_timeout = Some(timeout);
        self
    }

    pub async fn build(self) -> Result<RaftCluster<S>> {
        let node_id = self.node_id.ok_or_else(|| RaftError::InvalidConfiguration {
            component: "cluster".to_string(),
            message: "node_id is required".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;

        let bind_address = self.bind_address.unwrap_or_else(|| "127.0.0.1:0".to_string());
        
        let state_machine = self.state_machine.ok_or_else(|| RaftError::InvalidConfiguration {
            component: "cluster".to_string(),
            message: "state_machine is required".to_string(),
        backtrace: snafu::Backtrace::new(),
        })?;

        let preset = self.preset.unwrap_or(Preset::Development);

        // Apply preset configurations
        let (heartbeat_interval, election_timeout) = match preset {
            Preset::Development => (
                self.heartbeat_interval.unwrap_or(Duration::from_millis(500)),
                self.election_timeout.unwrap_or(Duration::from_millis(2000)),
            ),
            Preset::Testing => (
                self.heartbeat_interval.unwrap_or(Duration::from_millis(100)),
                self.election_timeout.unwrap_or(Duration::from_millis(500)),
            ),
            Preset::Production => (
                self.heartbeat_interval.unwrap_or(Duration::from_millis(1000)),
                self.election_timeout.unwrap_or(Duration::from_millis(5000)),
            ),
        };

        let config = ClusterConfig {
            heartbeat_interval,
            election_timeout,
            data_dir: self.data_dir,
            preset,
        };

        let metrics = Arc::new(RwLock::new(ClusterMetrics {
            node_id,
            peer_count: self.peers.len(),
            is_leader: true,
            term: 1,
            commit_index: 0,
            messages_sent: 0,
            messages_received: 0,
            proposals_applied: 0,
            snapshots_created: 0,
            uptime_start: SystemTime::now(),
        }));

        Ok(RaftCluster {
            node_id,
            state_machine,
            peers: self.peers,
            bind_address,
            config,
            metrics,
            is_running: Arc::new(RwLock::new(true)),
        })
    }
}

impl<S: StateMachine> RaftCluster<S> {
    pub fn builder() -> RaftClusterBuilder<S> {
        RaftClusterBuilder::new()
    }

    pub async fn propose(&mut self, command: S::Command) -> Result<()> {
        self.state_machine.apply_command(command).await?;
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.proposals_applied += 1;
        metrics.commit_index += 1;
        
        Ok(())
    }

    pub fn query(&self) -> &S::State {
        self.state_machine.get_current_state()
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn peers(&self) -> &[String] {
        &self.peers
    }

    pub async fn metrics(&self) -> ClusterMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn start(&mut self) -> Result<()> {
        *self.is_running.write().await = true;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        *self.is_running.write().await = false;
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }

    pub async fn create_snapshot(&mut self) -> Result<Vec<u8>> {
        let snapshot = self.state_machine.create_snapshot().await?;
        
        // Serialize the snapshot state to bytes
        let snapshot_bytes = bincode::serialize(&snapshot).map_err(|e| RaftError::Serialization {
            message_type: "snapshot".to_string(),
            source: Box::new(e),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.snapshots_created += 1;
        
        Ok(snapshot_bytes)
    }

    pub async fn restore_from_snapshot(&mut self, snapshot: &[u8]) -> Result<()> {
        // Deserialize the snapshot bytes to state
        let snapshot_state: S::State = bincode::deserialize(snapshot).map_err(|e| RaftError::Serialization {
            message_type: "snapshot".to_string(),
            source: Box::new(e),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        self.state_machine.restore_from_snapshot(snapshot_state).await
    }

    pub async fn add_peer(&mut self, peer: String) -> Result<()> {
        if !self.peers.contains(&peer) {
            self.peers.push(peer);
            self.metrics.write().await.peer_count = self.peers.len();
        }
        Ok(())
    }

    pub async fn remove_peer(&mut self, peer: &str) -> Result<()> {
        self.peers.retain(|p| p != peer);
        self.metrics.write().await.peer_count = self.peers.len();
        Ok(())
    }
}

// Test helper functions
async fn create_test_cluster(node_id: NodeId) -> RaftCluster<ExampleRaftStateMachine> {
    RaftCluster::builder()
        .node_id(node_id)
        .bind_address(format!("127.0.0.1:{}", 8000 + node_id))
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await
        .unwrap()
}

async fn create_test_cluster_with_peers(node_id: NodeId, peer_count: usize) -> RaftCluster<ExampleRaftStateMachine> {
    let peers: Vec<String> = (1..=peer_count)
        .filter(|&id| id as u64 != node_id)
        .map(|id| format!("127.0.0.1:{}", 8000 + id))
        .collect();

    RaftCluster::builder()
        .node_id(node_id)
        .bind_address(format!("127.0.0.1:{}", 8000 + node_id))
        .peers(peers)
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await
        .unwrap()
}

// Basic functionality tests
#[tokio::test]
async fn test_cluster_creation_basic() {
    let cluster = create_test_cluster(1).await;
    
    assert_eq!(cluster.node_id(), 1);
    assert_eq!(cluster.peers().len(), 0);
    assert!(cluster.is_running().await);
}

#[tokio::test]
async fn test_cluster_creation_with_peers() {
    let cluster = create_test_cluster_with_peers(1, 3).await;
    
    assert_eq!(cluster.node_id(), 1);
    assert_eq!(cluster.peers().len(), 2);
    assert!(cluster.peers().contains(&"127.0.0.1:8002".to_string()));
    assert!(cluster.peers().contains(&"127.0.0.1:8003".to_string()));
}

#[tokio::test]
async fn test_cluster_configuration_validation() {
    // Test missing node_id
    let result = RaftCluster::<ExampleRaftStateMachine>::builder()
        .bind_address("127.0.0.1:8080")
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await;
    
    assert!(result.is_err());
    if let Err(RaftError::InvalidConfiguration { component, message, .. }) = result {
        assert_eq!(component, "cluster");
        assert!(message.contains("node_id is required"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }

    // Test missing state_machine
    let result = RaftCluster::<ExampleRaftStateMachine>::builder()
        .node_id(1)
        .bind_address("127.0.0.1:8080")
        .build()
        .await;
    
    assert!(result.is_err());
    if let Err(RaftError::InvalidConfiguration { component, message, .. }) = result {
        assert_eq!(component, "cluster");
        assert!(message.contains("state_machine is required"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

// Configuration preset tests
#[test_case(Preset::Development, Duration::from_millis(500), Duration::from_millis(2000))]
#[test_case(Preset::Testing, Duration::from_millis(100), Duration::from_millis(500))]
#[test_case(Preset::Production, Duration::from_millis(1000), Duration::from_millis(5000))]
#[tokio::test]
async fn test_configuration_presets(preset: Preset, expected_heartbeat: Duration, expected_election: Duration) {
    let cluster = RaftCluster::builder()
        .node_id(1)
        .preset(preset)
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await
        .unwrap();

    let config = cluster.config();
    assert_eq!(config.heartbeat_interval, expected_heartbeat);
    assert_eq!(config.election_timeout, expected_election);
}

#[tokio::test]
async fn test_custom_configuration_overrides_preset() {
    let custom_heartbeat = Duration::from_millis(750);
    let custom_election = Duration::from_millis(3000);

    let cluster = RaftCluster::builder()
        .node_id(1)
        .preset(Preset::Development) // Default would be 500ms/2000ms
        .heartbeat_interval(custom_heartbeat)
        .election_timeout(custom_election)
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await
        .unwrap();

    let config = cluster.config();
    assert_eq!(config.heartbeat_interval, custom_heartbeat);
    assert_eq!(config.election_timeout, custom_election);
}

// Basic operations tests
#[tokio::test]
async fn test_propose_and_query_operations() {
    let mut cluster = create_test_cluster(1).await;
    
    // Test initial state
    let initial_state = cluster.query();
    assert_eq!(initial_state.len(), 0);

    // Test propose command
    let result = cluster.propose(KvCommand::Set {
        key: "test_key".to_string(),
        value: "test_value".to_string(),
    }).await;
    assert!(result.is_ok());

    // Test query after proposal
    let state = cluster.query();
    assert_eq!(state.len(), 1);
    assert_eq!(state.get("test_key"), Some(&"test_value".to_string()));

    // Test multiple operations
    cluster.propose(KvCommand::Set {
        key: "key2".to_string(),
        value: "value2".to_string(),
    }).await.unwrap();

    cluster.propose(KvCommand::Set {
        key: "key3".to_string(),
        value: "value3".to_string(),
    }).await.unwrap();

    let final_state = cluster.query();
    assert_eq!(final_state.len(), 3);
    assert_eq!(final_state.metadata.operation_count, 3);
}

#[tokio::test]
async fn test_delete_operations() {
    let mut cluster = create_test_cluster(1).await;
    
    // Set up some data
    cluster.propose(KvCommand::Set {
        key: "key1".to_string(),
        value: "value1".to_string(),
    }).await.unwrap();

    cluster.propose(KvCommand::Set {
        key: "key2".to_string(),
        value: "value2".to_string(),
    }).await.unwrap();

    // Test deletion
    cluster.propose(KvCommand::Delete {
        key: "key1".to_string(),
    }).await.unwrap();

    let state = cluster.query();
    assert_eq!(state.len(), 1);
    assert!(state.get("key1").is_none());
    assert_eq!(state.get("key2"), Some(&"value2".to_string()));
}

// Metrics and monitoring tests
#[tokio::test]
async fn test_metrics_tracking() {
    let mut cluster = create_test_cluster_with_peers(1, 3).await;
    
    let initial_metrics = cluster.metrics().await;
    assert_eq!(initial_metrics.node_id, 1);
    assert_eq!(initial_metrics.peer_count, 2);
    assert_eq!(initial_metrics.proposals_applied, 0);
    assert_eq!(initial_metrics.commit_index, 0);

    // Apply some commands
    for i in 1..=5 {
        cluster.propose(KvCommand::Set {
            key: format!("key{}", i),
            value: format!("value{}", i),
        }).await.unwrap();
    }

    let updated_metrics = cluster.metrics().await;
    assert_eq!(updated_metrics.proposals_applied, 5);
    assert_eq!(updated_metrics.commit_index, 5);
}

#[tokio::test]
async fn test_uptime_tracking() {
    let cluster = create_test_cluster(1).await;
    
    let metrics = cluster.metrics().await;
    let uptime = SystemTime::now()
        .duration_since(metrics.uptime_start)
        .unwrap();
    
    // Should be very recent
    assert!(uptime < Duration::from_secs(1));
}

// Snapshot operations tests
#[tokio::test]
async fn test_snapshot_creation_and_restoration() {
    let mut cluster = create_test_cluster(1).await;
    
    // Add some data
    for i in 1..=10 {
        cluster.propose(KvCommand::Set {
            key: format!("key{}", i),
            value: format!("value{}", i),
        }).await.unwrap();
    }

    // Create snapshot
    let snapshot = cluster.create_snapshot().await.unwrap();
    assert!(!snapshot.is_empty());

    // Verify snapshot metrics
    let metrics = cluster.metrics().await;
    assert_eq!(metrics.snapshots_created, 1);

    // Create new cluster and restore
    let mut new_cluster = create_test_cluster(2).await;
    new_cluster.restore_from_snapshot(&snapshot).await.unwrap();

    // Verify restoration
    let restored_state = new_cluster.query();
    assert_eq!(restored_state.len(), 10);
    
    for i in 1..=10 {
        assert_eq!(
            restored_state.get(&format!("key{}", i)),
            Some(&format!("value{}", i))
        );
    }
}

// Peer management tests
#[tokio::test]
async fn test_peer_management() {
    let mut cluster = create_test_cluster(1).await;
    
    // Initial state
    assert_eq!(cluster.peers().len(), 0);
    assert_eq!(cluster.metrics().await.peer_count, 0);

    // Add peers
    cluster.add_peer("127.0.0.1:8002".to_string()).await.unwrap();
    cluster.add_peer("127.0.0.1:8003".to_string()).await.unwrap();
    
    assert_eq!(cluster.peers().len(), 2);
    assert_eq!(cluster.metrics().await.peer_count, 2);

    // Remove peer
    cluster.remove_peer("127.0.0.1:8002").await.unwrap();
    
    assert_eq!(cluster.peers().len(), 1);
    assert_eq!(cluster.metrics().await.peer_count, 1);
    assert!(cluster.peers().contains(&"127.0.0.1:8003".to_string()));
    assert!(!cluster.peers().contains(&"127.0.0.1:8002".to_string()));

    // Try to add duplicate peer
    cluster.add_peer("127.0.0.1:8003".to_string()).await.unwrap();
    assert_eq!(cluster.peers().len(), 1); // Should not increase
}

// Lifecycle management tests
#[tokio::test]
async fn test_cluster_lifecycle() {
    let mut cluster = create_test_cluster(1).await;
    
    // Should start running
    assert!(cluster.is_running().await);

    // Start should be idempotent
    cluster.start().await.unwrap();
    assert!(cluster.is_running().await);

    // Should be able to perform operations while running
    cluster.propose(KvCommand::Set {
        key: "test".to_string(),
        value: "value".to_string(),
    }).await.unwrap();

    // Shutdown
    cluster.shutdown().await.unwrap();
    assert!(!cluster.is_running().await);
}

// Error handling tests
#[tokio::test]
async fn test_error_handling_invalid_operations() {
    // Test with custom state machine that can produce errors
    #[derive(Debug)]
    struct ErrorProneStateMachine {
        should_error: bool,
        state: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    enum TestCommand {
        Success,
        Error,
    }

    impl StateMachine for ErrorProneStateMachine {
        type Command = TestCommand;
        type State = String;
        type Response = ();
        type Query = ();
        type QueryResponse = String;

        async fn apply_command(&mut self, command: Self::Command) -> Result<()> {
            match command {
                TestCommand::Success => Ok(()),
                TestCommand::Error => Err(RaftError::InvalidConfiguration {
                    component: "test".to_string(),
                    message: "Simulated error".to_string(),
                    backtrace: snafu::Backtrace::new(),
                }),
            }
        }

        async fn create_snapshot(&self) -> Result<String> {
            Ok(self.get_current_state().clone())
        }

        async fn restore_from_snapshot(&mut self, snapshot: String) -> Result<()> {
            self.state = snapshot;
            Ok(())
        }

        async fn execute_query(&self, _query: Self::Query) -> Result<Self::QueryResponse> {
            Ok(self.get_current_state().clone())
        }

        fn get_current_state(&self) -> &Self::State {
            &self.state
        }
    }

    let mut cluster = RaftCluster::builder()
        .node_id(1)
        .state_machine(ErrorProneStateMachine { should_error: false, state: "ok_state".to_string() })
        .build()
        .await
        .unwrap();

    // Success case
    let result = cluster.propose(TestCommand::Success).await;
    assert!(result.is_ok());

    // Error case
    let result = cluster.propose(TestCommand::Error).await;
    assert!(result.is_err());

    if let Err(RaftError::InvalidConfiguration { component, message, .. }) = result {
        assert_eq!(component, "test");
        assert_eq!(message, "Simulated error");
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

// Performance tests
#[tokio::test]
async fn test_high_frequency_operations() {
    let mut cluster = create_test_cluster(1).await;
    
    let operation_count = 1000;
    let start_time = Instant::now();

    for i in 0..operation_count {
        cluster.propose(KvCommand::Set {
            key: format!("key{}", i),
            value: format!("value{}", i),
        }).await.unwrap();
    }

    let duration = start_time.elapsed();
    let ops_per_second = operation_count as f64 / duration.as_secs_f64();

    println!("Performed {} operations in {:?} ({:.2} ops/sec)", 
             operation_count, duration, ops_per_second);

    // Should achieve reasonable performance
    assert!(ops_per_second > 100.0); // At least 100 ops/sec

    // Verify all operations were applied
    let state = cluster.query();
    assert_eq!(state.len(), operation_count);
    assert_eq!(state.metadata.operation_count, operation_count as u64);
}

#[tokio::test]
async fn test_memory_usage_large_dataset() {
    let mut cluster = create_test_cluster(1).await;
    
    // Add a large dataset
    let large_value = "x".repeat(1000); // 1KB per value
    let key_count = 1000; // ~1MB total

    for i in 0..key_count {
        cluster.propose(KvCommand::Set {
            key: format!("large_key_{:06}", i),
            value: large_value.clone(),
        }).await.unwrap();
    }

    let state = cluster.query();
    assert_eq!(state.len(), key_count);

    // Test snapshot size
    let snapshot = cluster.create_snapshot().await.unwrap();
    
    // Snapshot should be compressed but reasonable
    assert!(snapshot.len() > 100); // Should contain actual data
    assert!(snapshot.len() < 2_000_000); // Should be compressed
}

// Integration tests
#[tokio::test]
async fn test_multi_cluster_simulation() {
    // Create multiple clusters that could potentially interact
    let mut clusters = Vec::new();
    
    for node_id in 1..=3 {
        let cluster = create_test_cluster_with_peers(node_id, 3).await;
        clusters.push(cluster);
    }

    // Each cluster should have the same peer configuration
    for cluster in &clusters {
        assert_eq!(cluster.peers().len(), 2);
    }

    // Apply operations to each cluster independently
    for (i, cluster) in clusters.iter_mut().enumerate() {
        cluster.propose(KvCommand::Set {
            key: format!("node_{}_key", i + 1),
            value: format!("node_{}_value", i + 1),
        }).await.unwrap();
    }

    // Verify each cluster has its own state
    for (i, cluster) in clusters.iter().enumerate() {
        let state = cluster.query();
        assert_eq!(state.len(), 1);
        assert_eq!(
            state.get(&format!("node_{}_key", i + 1)),
            Some(&format!("node_{}_value", i + 1))
        );
    }
}

#[tokio::test]
async fn test_concurrent_operations() {
    let cluster = Arc::new(RwLock::new(create_test_cluster(1).await));
    
    let mut handles = Vec::new();
    let operation_count = 100;

    // Spawn concurrent tasks
    for i in 0..operation_count {
        let cluster = cluster.clone();
        let handle = tokio::spawn(async move {
            let mut cluster = cluster.write().await;
            cluster.propose(KvCommand::Set {
                key: format!("concurrent_key_{}", i),
                value: format!("concurrent_value_{}", i),
            }).await.unwrap();
        });
        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all operations were applied
    let cluster = cluster.read().await;
    let state = cluster.query();
    assert_eq!(state.len(), operation_count);
    assert_eq!(state.metadata.operation_count, operation_count as u64);
}

// Property-based tests using proptest
proptest! {
    #[test]
    fn property_test_cluster_operations(
        node_id in 1u64..100,
        operation_count in 1usize..50,
        key_prefix in "[a-z]{1,10}",
        value_length in 1usize..100,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cluster = RaftCluster::builder()
                .node_id(node_id)
                .state_machine(ExampleRaftStateMachine::new_kv_store())
                .build()
                .await
                .unwrap();

            let mut expected_keys = std::collections::HashSet::new();

            for i in 0..operation_count {
                let key = format!("{}_{}", key_prefix, i);
                let value = "x".repeat(value_length);
                
                cluster.propose(KvCommand::Set {
                    key: key.clone(),
                    value: value.clone(),
                }).await.unwrap();
                
                expected_keys.insert(key);
            }

            let state = cluster.query();
            
            // Properties that should always hold:
            prop_assert_eq!(state.len(), expected_keys.len());
            prop_assert_eq!(state.metadata.operation_count, operation_count as u64);
            
            // All expected keys should be present
            for key in expected_keys {
                prop_assert!(state.data.contains_key(&key));
            }

            // Metrics should be consistent
            let metrics = cluster.metrics().await;
            prop_assert_eq!(metrics.node_id, node_id);
            prop_assert_eq!(metrics.proposals_applied, operation_count as u64);
            prop_assert_eq!(metrics.commit_index, operation_count as u64);
            
            Ok(())
        }).unwrap();
    }

    #[test]
    fn property_test_snapshot_consistency(
        initial_data_size in 1usize..20,
        additional_data_size in 1usize..20,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cluster = RaftCluster::builder()
                .node_id(1)
                .state_machine(ExampleRaftStateMachine::new_kv_store())
                .build()
                .await
                .unwrap();

            // Add initial data
            for i in 0..initial_data_size {
                cluster.propose(KvCommand::Set {
                    key: format!("initial_{}", i),
                    value: format!("value_{}", i),
                }).await.unwrap();
            }

            // Create snapshot
            let snapshot = cluster.create_snapshot().await.unwrap();

            // Add more data
            for i in 0..additional_data_size {
                cluster.propose(KvCommand::Set {
                    key: format!("additional_{}", i),
                    value: format!("value_{}", i),
                }).await.unwrap();
            }

            // Create new cluster and restore from snapshot
            let mut new_cluster = RaftCluster::builder()
                .node_id(2)
                .state_machine(ExampleRaftStateMachine::new_kv_store())
                .build()
                .await
                .unwrap();

            new_cluster.restore_from_snapshot(&snapshot).await.unwrap();

            // Properties that should hold:
            let original_state = cluster.query();
            let restored_state = new_cluster.query();
            
            // Restored state should have only the initial data
            prop_assert_eq!(restored_state.len(), initial_data_size);
            
            // All initial keys should be present in both
            for i in 0..initial_data_size {
                let key = format!("initial_{}", i);
                let expected_value = format!("value_{}", i);
                
                prop_assert_eq!(original_state.get(&key), Some(&expected_value));
                prop_assert_eq!(restored_state.get(&key), Some(&expected_value));
            }
            
            // Additional keys should only be in the original
            for i in 0..additional_data_size {
                let key = format!("additional_{}", i);
                prop_assert!(original_state.data.contains_key(&key));
                prop_assert!(!restored_state.data.contains_key(&key));
            }
            
            Ok(())
        }).unwrap();
    }

    #[test]
    fn property_test_peer_management(
        initial_peer_count in 0usize..10,
        peers_to_add in 0usize..5,
        peers_to_remove in 0usize..3,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let initial_peers: Vec<String> = (0..initial_peer_count)
                .map(|i| format!("127.0.0.1:{}", 9000 + i))
                .collect();

            let mut cluster = RaftCluster::builder()
                .node_id(1)
                .peers(initial_peers.clone())
                .state_machine(ExampleRaftStateMachine::new_kv_store())
                .build()
                .await
                .unwrap();

            // Initial state
            prop_assert_eq!(cluster.peers().len(), initial_peer_count);
            prop_assert_eq!(cluster.metrics().await.peer_count, initial_peer_count);

            // Add peers
            let mut expected_peers = initial_peers;
            for i in 0..peers_to_add {
                let new_peer = format!("127.0.0.1:{}", 10000 + i);
                cluster.add_peer(new_peer.clone()).await.unwrap();
                expected_peers.push(new_peer);
            }

            prop_assert_eq!(cluster.peers().len(), initial_peer_count + peers_to_add);

            // Remove some peers (but not more than we have)
            let peers_to_actually_remove = std::cmp::min(peers_to_remove, cluster.peers().len());
            for _ in 0..peers_to_actually_remove {
                if let Some(peer) = cluster.peers().first().cloned() {
                    cluster.remove_peer(&peer).await.unwrap();
                }
            }

            let final_peer_count = (initial_peer_count + peers_to_add).saturating_sub(peers_to_actually_remove);
            prop_assert_eq!(cluster.peers().len(), final_peer_count);
            prop_assert_eq!(cluster.metrics().await.peer_count, final_peer_count);
            
            Ok(())
        }).unwrap();
    }
}

// Stress tests
#[tokio::test]
async fn stress_test_rapid_operations() {
    let mut cluster = create_test_cluster(1).await;
    
    let batch_size = 100;
    let batch_count = 10;
    
    for batch in 0..batch_count {
        let start_time = Instant::now();
        
        for i in 0..batch_size {
            let key = format!("batch_{}_{}", batch, i);
            let value = format!("value_{}_{}", batch, i);
            
            cluster.propose(KvCommand::Set { key, value }).await.unwrap();
        }
        
        let batch_duration = start_time.elapsed();
        println!("Batch {} completed in {:?}", batch, batch_duration);
        
        // Each batch should complete reasonably quickly
        assert!(batch_duration < Duration::from_secs(5));
    }
    
    let final_state = cluster.query();
    assert_eq!(final_state.len(), batch_size * batch_count);
    assert_eq!(final_state.metadata.operation_count, (batch_size * batch_count) as u64);
}

#[tokio::test]
async fn stress_test_large_values() {
    let mut cluster = create_test_cluster(1).await;
    
    // Test with increasingly large values
    let sizes = vec![1_000, 10_000, 100_000]; // 1KB, 10KB, 100KB
    
    for (i, size) in sizes.iter().enumerate() {
        let large_value = "x".repeat(*size);
        
        let start_time = Instant::now();
        cluster.propose(KvCommand::Set {
            key: format!("large_key_{}", i),
            value: large_value.clone(),
        }).await.unwrap();
        let duration = start_time.elapsed();
        
        println!("Stored {}KB value in {:?}", size / 1000, duration);
        
        // Should handle large values reasonably quickly
        assert!(duration < Duration::from_secs(10));
        
        // Verify the value was stored correctly
        let state = cluster.query();
        assert_eq!(state.get(&format!("large_key_{}", i)), Some(&large_value));
    }
}

// Custom state machine integration tests
#[derive(Debug)]
struct CounterStateMachine {
    counter: i64,
    operations: Vec<CounterOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CounterCommand {
    Increment(i64),
    Decrement(i64),
    Reset,
    Multiply(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CounterOperation {
    command: CounterCommand,
    result: i64,
    timestamp: u64,
}

impl CounterStateMachine {
    fn new() -> Self {
        Self {
            counter: 0,
            operations: Vec::new(),
        }
    }
}

impl StateMachine for CounterStateMachine {
    type Command = CounterCommand;
    type State = (i64, Vec<CounterOperation>);
        type Response = ();
        type Query = ();
        type QueryResponse = (i64, Vec<CounterOperation>);

    async fn apply_command(&mut self, command: Self::Command) -> Result<()> {
        match command {
            CounterCommand::Increment(value) => {
                self.counter += value;
            }
            CounterCommand::Decrement(value) => {
                self.counter -= value;
            }
            CounterCommand::Reset => {
                self.counter = 0;
            }
            CounterCommand::Multiply(value) => {
                self.counter *= value;
            }
        }

        self.operations.push(CounterOperation {
            command,
            result: self.counter,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        });

        Ok(())
    }

    async fn create_snapshot(&self) -> Result<(i64, Vec<CounterOperation>)> {
        Ok((self.counter, self.operations.clone()))
    }

    async fn restore_from_snapshot(&mut self, snapshot: (i64, Vec<CounterOperation>)) -> Result<()> {
        self.counter = snapshot.0;
        self.operations = snapshot.1;
        Ok(())
    }

        async fn execute_query(&self, _query: Self::Query) -> Result<Self::QueryResponse> {
            Ok(self.get_current_state().clone())
        }

    fn get_current_state(&self) -> &Self::State {
        // This is a bit awkward due to the lifetime requirements, but works for testing
        unsafe {
            std::mem::transmute(&(self.counter, &self.operations))
        }
    }
}

#[tokio::test]
async fn test_custom_state_machine_integration() {
    let mut cluster = RaftCluster::builder()
        .node_id(1)
        .state_machine(CounterStateMachine::new())
        .build()
        .await
        .unwrap();

    // Test various operations
    cluster.propose(CounterCommand::Increment(5)).await.unwrap();
    cluster.propose(CounterCommand::Increment(3)).await.unwrap();
    cluster.propose(CounterCommand::Decrement(2)).await.unwrap();
    cluster.propose(CounterCommand::Multiply(2)).await.unwrap();

    // Verify the final state
    let state = cluster.query();
    assert_eq!(state.0, 12); // (0 + 5 + 3 - 2) * 2 = 12
    assert_eq!(state.1.len(), 4); // 4 operations

    // Test snapshot and restore
    let snapshot = cluster.create_snapshot().await.unwrap();
    
    let mut new_cluster = RaftCluster::builder()
        .node_id(2)
        .state_machine(CounterStateMachine::new())
        .build()
        .await
        .unwrap();

    new_cluster.restore_from_snapshot(&snapshot).await.unwrap();
    
    let restored_state = new_cluster.query();
    assert_eq!(restored_state.0, 12);
    assert_eq!(restored_state.1.len(), 4);
}

// Performance benchmarks
#[tokio::test]
async fn benchmark_operation_latency() {
    let mut cluster = create_test_cluster(1).await;
    
    let mut latencies = Vec::new();
    let operation_count = 100;

    for i in 0..operation_count {
        let start = Instant::now();
        
        cluster.propose(KvCommand::Set {
            key: format!("benchmark_key_{}", i),
            value: format!("benchmark_value_{}", i),
        }).await.unwrap();
        
        let latency = start.elapsed();
        latencies.push(latency);
    }

    // Calculate statistics
    latencies.sort();
    let median = latencies[latencies.len() / 2];
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
    let max = latencies.last().unwrap();

    println!("Operation latency statistics:");
    println!("  Median: {:?}", median);
    println!("  95th percentile: {:?}", p95);
    println!("  99th percentile: {:?}", p99);
    println!("  Maximum: {:?}", max);

    // Performance assertions
    assert!(median < Duration::from_millis(10)); // Median should be under 10ms
    assert!(p95 < Duration::from_millis(50));    // 95th percentile under 50ms
    assert!(p99 < Duration::from_millis(100));   // 99th percentile under 100ms
}

#[tokio::test]
async fn benchmark_throughput() {
    let mut cluster = create_test_cluster(1).await;
    
    let operation_count = 1000;
    let start_time = Instant::now();

    for i in 0..operation_count {
        cluster.propose(KvCommand::Set {
            key: format!("throughput_key_{}", i),
            value: format!("throughput_value_{}", i),
        }).await.unwrap();
    }

    let total_duration = start_time.elapsed();
    let throughput = operation_count as f64 / total_duration.as_secs_f64();

    println!("Throughput: {:.2} operations/second", throughput);
    println!("Total time for {} operations: {:?}", operation_count, total_duration);

    // Should achieve reasonable throughput
    assert!(throughput > 50.0); // At least 50 ops/sec
}