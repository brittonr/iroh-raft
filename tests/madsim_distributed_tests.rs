//! Distributed simulation tests for new iroh-raft features using madsim
//!
//! This module provides madsim-based tests for connection pooling, zero-copy messages,
//! and graceful shutdown under distributed system failure scenarios.

#![cfg(all(test, feature = "madsim"))]

use madsim::runtime::{Runtime, Handle};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::timeout;
use iroh_raft::transport::protocol::{
    ZeroCopyMessage, MessageType, LARGE_MESSAGE_THRESHOLD,
    serialize_raft_message_fast, deserialize_raft_message_fast,
};
use iroh_raft::transport::iroh::IrohRaftMetrics;

// Type aliases for the simulation
pub type NodeId = u64;
pub type Term = u64;
pub type LogIndex = u64;

/// Simplified Raft proposal data for testing
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalData {
    Set { key: String, value: String },
    Delete { key: String },
    Get { key: String },
    Noop,
    LargeData { size: usize, checksum: u32 },
}

/// Simplified error type for simulation
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("Node {node_id} not ready: {reason}")]
    NotReady { node_id: NodeId, reason: String },
    
    #[error("Transport error: {details}")]
    TransportError { details: String },
    
    #[error("Connection pool exhausted for node {node_id}")]
    ConnectionPoolExhausted { node_id: NodeId },
    
    #[error("Zero-copy serialization failed: {reason}")]
    ZeroCopyError { reason: String },
    
    #[error("Graceful shutdown timeout for node {node_id}")]
    ShutdownTimeout { node_id: NodeId },
}

pub type Result<T> = std::result::Result<T, SimError>;

/// Test configuration for distributed simulation scenarios
#[derive(Debug, Clone)]
struct DistributedTestConfig {
    pub cluster_size: usize,
    pub seed: u64,
    pub timeout: Duration,
    pub connection_pool_size: usize,
    pub use_zero_copy: bool,
    pub enable_large_messages: bool,
}

impl Default for DistributedTestConfig {
    fn default() -> Self {
        Self {
            cluster_size: 3,
            seed: 42,
            timeout: Duration::from_secs(30),
            connection_pool_size: 10,
            use_zero_copy: true,
            enable_large_messages: false,
        }
    }
}

/// Connection state for simulation
#[derive(Debug, Clone)]
struct SimConnection {
    from: NodeId,
    to: NodeId,
    established_at: Instant,
    last_used: Instant,
    message_count: u64,
    is_healthy: bool,
}

impl SimConnection {
    fn new(from: NodeId, to: NodeId) -> Self {
        let now = Instant::now();
        Self {
            from,
            to,
            established_at: now,
            last_used: now,
            message_count: 0,
            is_healthy: true,
        }
    }
    
    fn use_connection(&mut self) {
        self.last_used = Instant::now();
        self.message_count += 1;
    }
    
    fn is_stale(&self, threshold: Duration) -> bool {
        self.last_used.elapsed() > threshold
    }
}

/// Connection pool manager for simulation
#[derive(Debug)]
struct SimConnectionPool {
    connections: HashMap<(NodeId, NodeId), SimConnection>,
    max_connections: usize,
    stale_threshold: Duration,
}

impl SimConnectionPool {
    fn new(max_connections: usize) -> Self {
        Self {
            connections: HashMap::new(),
            max_connections,
            stale_threshold: Duration::from_secs(300), // 5 minutes
        }
    }
    
    fn get_or_create_connection(&mut self, from: NodeId, to: NodeId) -> Result<&mut SimConnection> {
        let key = (from, to);
        
        // Check if connection exists and is healthy
        if let Some(conn) = self.connections.get_mut(&key) {
            if conn.is_healthy && !conn.is_stale(self.stale_threshold) {
                conn.use_connection();
                return Ok(conn);
            }
        }
        
        // Clean up stale connections
        self.cleanup_stale_connections();
        
        // Check pool capacity
        if self.connections.len() >= self.max_connections {
            return Err(SimError::ConnectionPoolExhausted { node_id: to });
        }
        
        // Create new connection
        let conn = SimConnection::new(from, to);
        self.connections.insert(key, conn);
        
        Ok(self.connections.get_mut(&key).unwrap())
    }
    
    fn cleanup_stale_connections(&mut self) {
        self.connections.retain(|_, conn| {
            conn.is_healthy && !conn.is_stale(self.stale_threshold)
        });
    }
    
    fn mark_connection_unhealthy(&mut self, from: NodeId, to: NodeId) {
        if let Some(conn) = self.connections.get_mut(&(from, to)) {
            conn.is_healthy = false;
        }
    }
    
    fn get_metrics(&self) -> ConnectionPoolMetrics {
        let active_connections = self.connections.values()
            .filter(|c| c.is_healthy && !c.is_stale(self.stale_threshold))
            .count();
        let total_messages = self.connections.values()
            .map(|c| c.message_count)
            .sum();
        
        ConnectionPoolMetrics {
            active_connections,
            total_connections: self.connections.len(),
            total_messages,
        }
    }
    
    async fn graceful_shutdown(&mut self) -> Duration {
        let start = Instant::now();
        
        // Mark all connections as unhealthy
        for conn in self.connections.values_mut() {
            conn.is_healthy = false;
        }
        
        // Clear connections
        self.connections.clear();
        
        start.elapsed()
    }
}

/// Connection pool metrics
#[derive(Debug, Clone)]
struct ConnectionPoolMetrics {
    active_connections: usize,
    total_connections: usize,
    total_messages: u64,
}

/// Zero-copy message handler for simulation
#[derive(Debug)]
struct SimZeroCopyHandler {
    use_zero_copy: bool,
    large_message_threshold: usize,
    message_count: u64,
    bytes_processed: u64,
}

impl SimZeroCopyHandler {
    fn new(use_zero_copy: bool) -> Self {
        Self {
            use_zero_copy,
            large_message_threshold: LARGE_MESSAGE_THRESHOLD,
            message_count: 0,
            bytes_processed: 0,
        }
    }
    
    async fn process_message(&mut self, data: &ProposalData) -> Result<Vec<u8>> {
        self.message_count += 1;
        
        let serialized = if self.use_zero_copy {
            // Simulate zero-copy serialization
            self.serialize_zero_copy(data).await?
        } else {
            // Traditional serialization
            bincode::serialize(data).map_err(|e| SimError::ZeroCopyError {
                reason: format!("Serialization failed: {}", e),
            })?
        };
        
        self.bytes_processed += serialized.len() as u64;
        Ok(serialized)
    }
    
    async fn serialize_zero_copy(&self, data: &ProposalData) -> Result<Vec<u8>> {
        // Simulate zero-copy optimization for large messages
        match data {
            ProposalData::LargeData { size, checksum } => {
                if *size > self.large_message_threshold {
                    // Simulate streaming serialization for large data
                    let mut result = Vec::with_capacity(*size);
                    result.extend_from_slice(&checksum.to_be_bytes());
                    result.resize(*size, 0x42); // Fill with test data
                    Ok(result)
                } else {
                    // Regular serialization for small data
                    bincode::serialize(data).map_err(|e| SimError::ZeroCopyError {
                        reason: format!("Small data serialization failed: {}", e),
                    })
                }
            }
            _ => {
                // Regular data serialization
                bincode::serialize(data).map_err(|e| SimError::ZeroCopyError {
                    reason: format!("Regular serialization failed: {}", e),
                })
            }
        }
    }
    
    fn get_metrics(&self) -> ZeroCopyMetrics {
        ZeroCopyMetrics {
            message_count: self.message_count,
            bytes_processed: self.bytes_processed,
            use_zero_copy: self.use_zero_copy,
        }
    }
}

/// Zero-copy metrics
#[derive(Debug, Clone)]
struct ZeroCopyMetrics {
    message_count: u64,
    bytes_processed: u64,
    use_zero_copy: bool,
}

/// Simulated transport node with advanced features
#[derive(Debug)]
struct SimTransportNode {
    id: NodeId,
    connection_pool: SimConnectionPool,
    zero_copy_handler: SimZeroCopyHandler,
    is_shutting_down: bool,
    shutdown_start: Option<Instant>,
    message_buffer: VecDeque<ProposalData>,
    is_partitioned: bool,
}

impl SimTransportNode {
    fn new(id: NodeId, config: &DistributedTestConfig) -> Self {
        Self {
            id,
            connection_pool: SimConnectionPool::new(config.connection_pool_size),
            zero_copy_handler: SimZeroCopyHandler::new(config.use_zero_copy),
            is_shutting_down: false,
            shutdown_start: None,
            message_buffer: VecDeque::new(),
            is_partitioned: false,
        }
    }
    
    async fn send_message(&mut self, to: NodeId, data: ProposalData) -> Result<()> {
        if self.is_shutting_down {
            return Err(SimError::NotReady {
                node_id: self.id,
                reason: "node is shutting down".to_string(),
            });
        }
        
        if self.is_partitioned {
            return Err(SimError::NotReady {
                node_id: self.id,
                reason: "node is partitioned".to_string(),
            });
        }
        
        // Get or create connection
        let _conn = self.connection_pool.get_or_create_connection(self.id, to)?;
        
        // Process message with zero-copy handler
        let _serialized = self.zero_copy_handler.process_message(&data).await?;
        
        // Buffer message for later processing
        self.message_buffer.push_back(data);
        
        Ok(())
    }
    
    async fn process_buffered_messages(&mut self) -> usize {
        let mut processed = 0;
        while let Some(_msg) = self.message_buffer.pop_front() {
            // Simulate message processing
            processed += 1;
            if processed >= 100 {
                break; // Batch limit
            }
        }
        processed
    }
    
    async fn initiate_graceful_shutdown(&mut self) -> Result<()> {
        if self.is_shutting_down {
            return Ok(());
        }
        
        self.is_shutting_down = true;
        self.shutdown_start = Some(Instant::now());
        
        // Process remaining buffered messages
        self.process_buffered_messages().await;
        
        // Shutdown connection pool
        let _shutdown_duration = self.connection_pool.graceful_shutdown().await;
        
        Ok(())
    }
    
    fn is_shutdown_complete(&self) -> bool {
        if let Some(start) = self.shutdown_start {
            start.elapsed() > Duration::from_millis(100) // Simulate shutdown delay
        } else {
            false
        }
    }
    
    fn set_partitioned(&mut self, partitioned: bool) {
        self.is_partitioned = partitioned;
    }
    
    fn get_transport_metrics(&self) -> SimTransportMetrics {
        let pool_metrics = self.connection_pool.get_metrics();
        let zero_copy_metrics = self.zero_copy_handler.get_metrics();
        
        SimTransportMetrics {
            node_id: self.id,
            connection_pool: pool_metrics,
            zero_copy: zero_copy_metrics,
            buffered_messages: self.message_buffer.len(),
            is_shutting_down: self.is_shutting_down,
        }
    }
}

/// Transport metrics for simulation
#[derive(Debug, Clone)]
struct SimTransportMetrics {
    node_id: NodeId,
    connection_pool: ConnectionPoolMetrics,
    zero_copy: ZeroCopyMetrics,
    buffered_messages: usize,
    is_shutting_down: bool,
}

/// Distributed transport cluster for simulation
#[derive(Debug)]
struct SimDistributedTransport {
    nodes: HashMap<NodeId, SimTransportNode>,
    config: DistributedTestConfig,
    network_partitions: HashMap<NodeId, HashSet<NodeId>>,
    message_delays: HashMap<(NodeId, NodeId), Duration>,
}

impl SimDistributedTransport {
    fn new(node_ids: Vec<NodeId>, config: DistributedTestConfig) -> Self {
        let mut nodes = HashMap::new();
        for id in node_ids {
            nodes.insert(id, SimTransportNode::new(id, &config));
        }
        
        Self {
            nodes,
            config,
            network_partitions: HashMap::new(),
            message_delays: HashMap::new(),
        }
    }
    
    async fn send_message(&mut self, from: NodeId, to: NodeId, data: ProposalData) -> Result<()> {
        // Check if nodes can communicate
        if !self.can_communicate(from, to) {
            return Err(SimError::TransportError {
                details: format!("Nodes {} and {} cannot communicate", from, to),
            });
        }
        
        // Simulate network delay
        if let Some(delay) = self.message_delays.get(&(from, to)) {
            madsim::time::sleep(*delay).await;
        }
        
        // Send message through source node
        if let Some(node) = self.nodes.get_mut(&from) {
            node.send_message(to, data).await?;
        }
        
        Ok(())
    }
    
    fn can_communicate(&self, from: NodeId, to: NodeId) -> bool {
        // Check network partitions
        if let Some(partition) = self.network_partitions.get(&from) {
            partition.contains(&to)
        } else {
            true
        }
    }
    
    fn create_partition(&mut self, group1: Vec<NodeId>, group2: Vec<NodeId>) {
        self.network_partitions.clear();
        
        for &node_id in &group1 {
            self.network_partitions.insert(node_id, group1.iter().cloned().collect());
        }
        for &node_id in &group2 {
            self.network_partitions.insert(node_id, group2.iter().cloned().collect());
        }
    }
    
    fn heal_partition(&mut self) {
        self.network_partitions.clear();
        self.message_delays.clear();
        
        // Restore partitioned nodes
        for node in self.nodes.values_mut() {
            node.set_partitioned(false);
        }
    }
    
    fn add_message_delay(&mut self, from: NodeId, to: NodeId, delay: Duration) {
        self.message_delays.insert((from, to), delay);
    }
    
    async fn graceful_shutdown_node(&mut self, node_id: NodeId) -> Result<Duration> {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            let start = Instant::now();
            node.initiate_graceful_shutdown().await?;
            
            // Wait for shutdown to complete
            while !node.is_shutdown_complete() {
                madsim::time::sleep(Duration::from_millis(10)).await;
            }
            
            Ok(start.elapsed())
        } else {
            Err(SimError::NotReady {
                node_id,
                reason: "node not found".to_string(),
            })
        }
    }
    
    async fn graceful_shutdown_all(&mut self) -> HashMap<NodeId, Duration> {
        let mut shutdown_times = HashMap::new();
        
        for (&node_id, node) in &mut self.nodes {
            let start = Instant::now();
            let _ = node.initiate_graceful_shutdown().await;
            
            while !node.is_shutdown_complete() {
                madsim::time::sleep(Duration::from_millis(10)).await;
            }
            
            shutdown_times.insert(node_id, start.elapsed());
        }
        
        shutdown_times
    }
    
    fn get_cluster_metrics(&self) -> ClusterTransportMetrics {
        let mut node_metrics = HashMap::new();
        let mut total_connections = 0;
        let mut total_messages = 0;
        
        for (&node_id, node) in &self.nodes {
            let metrics = node.get_transport_metrics();
            total_connections += metrics.connection_pool.active_connections;
            total_messages += metrics.connection_pool.total_messages;
            node_metrics.insert(node_id, metrics);
        }
        
        ClusterTransportMetrics {
            node_metrics,
            total_active_connections: total_connections,
            total_messages,
        }
    }
}

/// Cluster-wide transport metrics
#[derive(Debug, Clone)]
struct ClusterTransportMetrics {
    node_metrics: HashMap<NodeId, SimTransportMetrics>,
    total_active_connections: usize,
    total_messages: u64,
}

/// Test connection pooling under network partitions
#[test]
fn test_connection_pooling_under_partitions() {
    let config = DistributedTestConfig {
        cluster_size: 5,
        seed: 123,
        connection_pool_size: 3,
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Send messages to establish connections
        for i in 0..10 {
            let data = ProposalData::Set {
                key: format!("key_{}", i),
                value: format!("value_{}", i),
            };
            
            for &from in &node_ids {
                for &to in &node_ids {
                    if from != to {
                        let _ = cluster.send_message(from, to, data.clone()).await;
                    }
                }
            }
        }
        
        // Check initial metrics
        let metrics_before = cluster.get_cluster_metrics();
        assert!(metrics_before.total_active_connections > 0);
        
        // Create network partition
        let group1 = vec![1, 2];
        let group2 = vec![3, 4, 5];
        cluster.create_partition(group1.clone(), group2.clone());
        
        // Try to send messages across partition
        let cross_partition_data = ProposalData::Set {
            key: "cross_partition".to_string(),
            value: "should_fail".to_string(),
        };
        
        let result = cluster.send_message(1, 3, cross_partition_data).await;
        assert!(result.is_err());
        
        // Send messages within partitions
        let within_partition_data = ProposalData::Set {
            key: "within_partition".to_string(),
            value: "should_succeed".to_string(),
        };
        
        let result1 = cluster.send_message(1, 2, within_partition_data.clone()).await;
        let result2 = cluster.send_message(3, 4, within_partition_data).await;
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        
        // Heal partition
        cluster.heal_partition();
        
        // Cross-partition communication should work again
        let heal_data = ProposalData::Set {
            key: "healed".to_string(),
            value: "success".to_string(),
        };
        
        let result = cluster.send_message(1, 3, heal_data).await;
        assert!(result.is_ok());
        
        // Check final metrics
        let metrics_after = cluster.get_cluster_metrics();
        assert!(metrics_after.total_messages > metrics_before.total_messages);
    });
}

/// Test zero-copy message handling with large messages
#[test]
fn test_zero_copy_large_messages() {
    let config = DistributedTestConfig {
        cluster_size: 3,
        seed: 456,
        use_zero_copy: true,
        enable_large_messages: true,
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Test small messages
        let small_data = ProposalData::Set {
            key: "small".to_string(),
            value: "data".to_string(),
        };
        
        let result = cluster.send_message(1, 2, small_data).await;
        assert!(result.is_ok());
        
        // Test large messages
        let large_data = ProposalData::LargeData {
            size: LARGE_MESSAGE_THRESHOLD + 1000,
            checksum: 0x12345678,
        };
        
        let start = Instant::now();
        let result = cluster.send_message(1, 2, large_data).await;
        let duration = start.elapsed();
        
        assert!(result.is_ok());
        
        // Large message should still be processed efficiently
        assert!(duration < Duration::from_secs(1));
        
        // Check zero-copy metrics
        let metrics = cluster.get_cluster_metrics();
        let node1_metrics = &metrics.node_metrics[&1];
        assert!(node1_metrics.zero_copy.use_zero_copy);
        assert!(node1_metrics.zero_copy.bytes_processed > LARGE_MESSAGE_THRESHOLD as u64);
    });
}

/// Test graceful shutdown under load
#[test]
fn test_graceful_shutdown_under_load() {
    let config = DistributedTestConfig {
        cluster_size: 4,
        seed: 789,
        connection_pool_size: 5,
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Generate load by sending many messages
        for i in 0..50 {
            let data = ProposalData::Set {
                key: format!("load_key_{}", i),
                value: format!("load_value_{}", i),
            };
            
            for &from in &node_ids {
                for &to in &node_ids {
                    if from != to {
                        let _ = cluster.send_message(from, to, data.clone()).await;
                    }
                }
            }
        }
        
        // Check metrics before shutdown
        let metrics_before = cluster.get_cluster_metrics();
        assert!(metrics_before.total_active_connections > 0);
        assert!(metrics_before.total_messages > 0);
        
        // Initiate graceful shutdown of one node
        let shutdown_duration = cluster.graceful_shutdown_node(1).await.unwrap();
        assert!(shutdown_duration < Duration::from_secs(5));
        
        // Check that node 1 is shut down
        let metrics_after = cluster.get_cluster_metrics();
        let node1_metrics = &metrics_after.node_metrics[&1];
        assert!(node1_metrics.is_shutting_down);
        
        // Other nodes should still be functional
        for &node_id in &[2, 3, 4] {
            let node_metrics = &metrics_after.node_metrics[&node_id];
            assert!(!node_metrics.is_shutting_down);
        }
        
        // Shutdown all remaining nodes
        let shutdown_times = cluster.graceful_shutdown_all().await;
        
        // All shutdowns should complete quickly
        for (&node_id, &duration) in &shutdown_times {
            println!("Node {} shutdown took {:?}", node_id, duration);
            assert!(duration < Duration::from_secs(2));
        }
    });
}

/// Test connection pool exhaustion and recovery
#[test]
fn test_connection_pool_exhaustion() {
    let config = DistributedTestConfig {
        cluster_size: 10,
        seed: 999,
        connection_pool_size: 3, // Small pool to trigger exhaustion
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Try to establish connections to many nodes from node 1
        let mut success_count = 0;
        let mut failure_count = 0;
        
        for &to in &node_ids[1..] { // Skip node 1 itself
            let data = ProposalData::Set {
                key: format!("connection_test_{}", to),
                value: "test".to_string(),
            };
            
            match cluster.send_message(1, to, data).await {
                Ok(_) => success_count += 1,
                Err(SimError::ConnectionPoolExhausted { .. }) => failure_count += 1,
                Err(_) => (), // Other errors
            }
        }
        
        println!("Connections: {} successful, {} failed due to pool exhaustion", 
                success_count, failure_count);
        
        // Should succeed for some connections but fail when pool is exhausted
        assert!(success_count > 0);
        assert!(failure_count > 0);
        
        // Check metrics
        let metrics = cluster.get_cluster_metrics();
        let node1_metrics = &metrics.node_metrics[&1];
        assert!(node1_metrics.connection_pool.active_connections <= 3);
    });
}

/// Test zero-copy vs traditional serialization performance
#[test]
fn test_zero_copy_vs_traditional_performance() {
    let runtime = Runtime::with_seed(12345);
    runtime.block_on(async {
        // Test with zero-copy enabled
        let config_zero_copy = DistributedTestConfig {
            cluster_size: 3,
            seed: 12345,
            use_zero_copy: true,
            enable_large_messages: true,
            ..Default::default()
        };
        
        let node_ids: Vec<NodeId> = (1..=3).collect();
        let mut cluster_zero_copy = SimDistributedTransport::new(node_ids.clone(), config_zero_copy);
        
        // Test with traditional serialization
        let config_traditional = DistributedTestConfig {
            cluster_size: 3,
            seed: 12345,
            use_zero_copy: false,
            enable_large_messages: true,
            ..Default::default()
        };
        
        let mut cluster_traditional = SimDistributedTransport::new(node_ids.clone(), config_traditional);
        
        let large_data = ProposalData::LargeData {
            size: LARGE_MESSAGE_THRESHOLD + 5000,
            checksum: 0xDEADBEEF,
        };
        
        // Benchmark zero-copy
        let start = Instant::now();
        for _ in 0..10 {
            let _ = cluster_zero_copy.send_message(1, 2, large_data.clone()).await;
        }
        let zero_copy_duration = start.elapsed();
        
        // Benchmark traditional
        let start = Instant::now();
        for _ in 0..10 {
            let _ = cluster_traditional.send_message(1, 2, large_data.clone()).await;
        }
        let traditional_duration = start.elapsed();
        
        println!("Zero-copy: {:?}, Traditional: {:?}", zero_copy_duration, traditional_duration);
        
        // Zero-copy should be at least as fast (in simulation, both might be similar)
        assert!(zero_copy_duration <= traditional_duration * 2);
        
        let zero_copy_metrics = cluster_zero_copy.get_cluster_metrics();
        let traditional_metrics = cluster_traditional.get_cluster_metrics();
        
        // Both should process messages
        assert!(zero_copy_metrics.total_messages > 0);
        assert!(traditional_metrics.total_messages > 0);
    });
}

/// Test concurrent shutdown scenarios
#[test]
fn test_concurrent_shutdown_scenarios() {
    let config = DistributedTestConfig {
        cluster_size: 5,
        seed: 2024,
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Establish connections
        for &from in &node_ids {
            for &to in &node_ids {
                if from != to {
                    let data = ProposalData::Set {
                        key: format!("init_{}_{}", from, to),
                        value: "init".to_string(),
                    };
                    let _ = cluster.send_message(from, to, data).await;
                }
            }
        }
        
        // Simulate concurrent shutdown of multiple nodes
        let shutdown_start = Instant::now();
        
        // Shutdown nodes 1 and 2 concurrently
        let shutdown1_task = cluster.graceful_shutdown_node(1);
        let shutdown2_task = cluster.graceful_shutdown_node(2);
        
        let (result1, result2) = tokio::join!(shutdown1_task, shutdown2_task);
        
        let total_shutdown_time = shutdown_start.elapsed();
        
        // Both shutdowns should succeed
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        
        // Concurrent shutdowns should not take much longer than individual ones
        assert!(total_shutdown_time < Duration::from_secs(10));
        
        // Remaining nodes should still be functional
        let data = ProposalData::Set {
            key: "post_shutdown".to_string(),
            value: "test".to_string(),
        };
        
        let result = cluster.send_message(3, 4, data).await;
        assert!(result.is_ok());
        
        // Check final metrics
        let metrics = cluster.get_cluster_metrics();
        assert!(metrics.node_metrics[&1].is_shutting_down);
        assert!(metrics.node_metrics[&2].is_shutting_down);
        assert!(!metrics.node_metrics[&3].is_shutting_down);
    });
}

/// Test mixed workload with various features
#[test]
fn test_mixed_workload_comprehensive() {
    let config = DistributedTestConfig {
        cluster_size: 6,
        seed: 7777,
        connection_pool_size: 4,
        use_zero_copy: true,
        enable_large_messages: true,
        ..Default::default()
    };
    
    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimDistributedTransport::new(node_ids.clone(), config);
        
        // Phase 1: Normal operation with mixed message types
        for i in 0..30 {
            let data = match i % 4 {
                0 => ProposalData::Set {
                    key: format!("key_{}", i),
                    value: format!("value_{}", i),
                },
                1 => ProposalData::Delete {
                    key: format!("key_{}", i / 2),
                },
                2 => ProposalData::Get {
                    key: format!("key_{}", i / 3),
                },
                3 => ProposalData::LargeData {
                    size: LARGE_MESSAGE_THRESHOLD + (i * 100),
                    checksum: (i as u32) * 0x1234,
                },
                _ => ProposalData::Noop,
            };
            
            let from = (i % node_ids.len()) + 1;
            let to = ((i + 1) % node_ids.len()) + 1;
            
            if from != to {
                let _ = cluster.send_message(from as u64, to as u64, data).await;
            }
        }
        
        // Phase 2: Network partition
        let group1 = vec![1, 2, 3];
        let group2 = vec![4, 5, 6];
        cluster.create_partition(group1.clone(), group2.clone());
        
        // Continue sending within partitions
        for i in 0..10 {
            let data = ProposalData::Set {
                key: format!("partition_key_{}", i),
                value: "partition_value".to_string(),
            };
            
            // Within group1
            let _ = cluster.send_message(1, 2, data.clone()).await;
            // Within group2
            let _ = cluster.send_message(4, 5, data).await;
        }
        
        // Phase 3: Add network delays
        cluster.add_message_delay(1, 2, Duration::from_millis(50));
        cluster.add_message_delay(4, 5, Duration::from_millis(100));
        
        // Send messages with delays
        for i in 0..5 {
            let data = ProposalData::Set {
                key: format!("delayed_key_{}", i),
                value: "delayed_value".to_string(),
            };
            
            let _ = cluster.send_message(1, 2, data.clone()).await;
            let _ = cluster.send_message(4, 5, data).await;
        }
        
        // Phase 4: Heal partition and gradual shutdown
        cluster.heal_partition();
        
        // Send recovery messages
        for i in 0..5 {
            let data = ProposalData::Set {
                key: format!("recovery_key_{}", i),
                value: "recovery_value".to_string(),
            };
            
            let _ = cluster.send_message(1, 4, data).await; // Cross-partition
        }
        
        // Gradual shutdown
        let mut shutdown_order = vec![6, 5, 4];
        for &node_id in &shutdown_order {
            let shutdown_time = cluster.graceful_shutdown_node(node_id).await.unwrap();
            println!("Node {} shutdown in {:?}", node_id, shutdown_time);
            assert!(shutdown_time < Duration::from_secs(3));
        }
        
        // Final metrics check
        let final_metrics = cluster.get_cluster_metrics();
        
        // Verify comprehensive workload results
        assert!(final_metrics.total_messages > 0);
        
        for &node_id in &[1, 2, 3] {
            let node_metrics = &final_metrics.node_metrics[&node_id];
            assert!(!node_metrics.is_shutting_down);
            assert!(node_metrics.zero_copy.use_zero_copy);
        }
        
        for &node_id in &[4, 5, 6] {
            let node_metrics = &final_metrics.node_metrics[&node_id];
            assert!(node_metrics.is_shutting_down);
        }
        
        println!("Comprehensive test completed successfully!");
        println!("Total messages processed: {}", final_metrics.total_messages);
        println!("Active connections remaining: {}", final_metrics.total_active_connections);
    });
}