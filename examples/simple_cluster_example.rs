//! Simple cluster example demonstrating improved API ergonomics
//!
//! This example showcases the simplified RaftCluster API that provides
//! an easy-to-use interface for distributed consensus applications.
//!
//! Key features demonstrated:
//! - Simple cluster setup with minimal boilerplate
//! - Easy peer management
//! - Straightforward proposal and query operations
//! - Graceful error handling
//! - Clean resource management
//!
//! Run with: cargo run --example simple_cluster_example

use iroh_raft::{
    config::{ConfigBuilder, ConfigError},
    error::RaftError,
    raft::{KvCommand, KvState, KeyValueStore, StateMachine, ExampleRaftStateMachine},
    types::NodeId,
    Result,
};
use std::time::Duration;
use tokio::time::timeout;

/// Simplified RaftCluster API (mock implementation for demonstration)
/// In the actual implementation, this would be provided by the new API layer
#[derive(Debug)]
pub struct RaftCluster<S: StateMachine> {
    node_id: NodeId,
    state_machine: S,
    peers: Vec<String>,
    bind_address: String,
}

/// Configuration preset types for common deployment scenarios
#[derive(Debug, Clone)]
pub enum Preset {
    Development,
    Testing,
    Production,
}

/// Builder for RaftCluster with improved ergonomics
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
        })?;

        let bind_address = self.bind_address.unwrap_or_else(|| "127.0.0.1:0".to_string());
        
        let state_machine = self.state_machine.ok_or_else(|| RaftError::InvalidConfiguration {
            component: "cluster".to_string(),
            message: "state_machine is required".to_string(),
        })?;

        // Apply preset configurations
        let (heartbeat_interval, election_timeout) = match self.preset.unwrap_or(Preset::Development) {
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

        println!(
            "🚀 Building RaftCluster with node_id={}, bind_address={}, peers={:?}",
            node_id, bind_address, self.peers
        );
        println!(
            "⚙️  Configuration: heartbeat={}ms, election_timeout={}ms",
            heartbeat_interval.as_millis(),
            election_timeout.as_millis()
        );

        Ok(RaftCluster {
            node_id,
            state_machine,
            peers: self.peers,
            bind_address,
        })
    }
}

impl<S: StateMachine> RaftCluster<S> {
    pub fn builder() -> RaftClusterBuilder<S> {
        RaftClusterBuilder::new()
    }

    /// Propose a command to the cluster
    pub async fn propose(&mut self, command: S::Command) -> Result<()> {
        println!("📝 Proposing command: {:?}", command);
        
        // In a real implementation, this would go through Raft consensus
        // For this demo, we'll apply directly to the local state machine
        self.state_machine.apply_command(command).await?;
        
        println!("✅ Command applied successfully");
        Ok(())
    }

    /// Query the current state (read-only operation)
    pub fn query(&self) -> &S::State {
        self.state_machine.get_current_state()
    }

    /// Get node information
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get peer list
    pub fn peers(&self) -> &[String] {
        &self.peers
    }

    /// Get cluster metrics
    pub async fn metrics(&self) -> ClusterMetrics {
        ClusterMetrics {
            node_id: self.node_id,
            peer_count: self.peers.len(),
            is_leader: true, // Mock value
            term: 1,         // Mock value
            commit_index: 1, // Mock value
        }
    }

    /// Graceful shutdown
    pub async fn shutdown(&mut self) -> Result<()> {
        println!("🛑 Shutting down cluster node {}", self.node_id);
        // In real implementation: stop Raft, close connections, flush state
        Ok(())
    }
}

/// Cluster metrics for monitoring
#[derive(Debug, Clone)]
pub struct ClusterMetrics {
    pub node_id: NodeId,
    pub peer_count: usize,
    pub is_leader: bool,
    pub term: u64,
    pub commit_index: u64,
}

/// Query types for the key-value store
#[derive(Debug, Clone)]
pub struct GetQuery {
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct ListKeysQuery;

/// Command types for demonstrations
#[derive(Debug, Clone)]
pub struct SetCommand {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct DeleteCommand {
    pub key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🎯 Starting Simple Cluster Example");
    println!("===================================");

    // Example 1: Basic cluster setup with minimal configuration
    println!("\n📋 Example 1: Basic Setup");
    println!("--------------------------");

    let mut cluster = RaftCluster::builder()
        .node_id(1)
        .bind_address("127.0.0.1:8080")
        .peers(vec!["127.0.0.1:8081", "127.0.0.1:8082"])
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await?;

    println!("✅ Cluster created successfully!");
    println!("   Node ID: {}", cluster.node_id());
    println!("   Peers: {:?}", cluster.peers());

    // Example 2: Simple operations
    println!("\n📝 Example 2: Basic Operations");
    println!("------------------------------");

    // Set some values
    cluster.propose(KvCommand::Set {
        key: "user:alice".to_string(),
        value: "Alice Johnson".to_string(),
    }).await?;

    cluster.propose(KvCommand::Set {
        key: "user:bob".to_string(),
        value: "Bob Smith".to_string(),
    }).await?;

    cluster.propose(KvCommand::Set {
        key: "config:timeout".to_string(),
        value: "30s".to_string(),
    }).await?;

    // Query the state
    let state = cluster.query();
    println!("📊 Current state:");
    println!("   Total keys: {}", state.len());
    println!("   Operations: {}", state.metadata.operation_count);
    
    for (key, value) in state.iter() {
        println!("   {} = {}", key, value);
    }

    // Example 3: Configuration presets
    println!("\n⚙️  Example 3: Configuration Presets");
    println!("------------------------------------");

    let mut test_cluster = RaftCluster::builder()
        .node_id(2)
        .bind_address("127.0.0.1:9080")
        .preset(Preset::Testing)  // Fast timeouts for testing
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await?;

    println!("✅ Test cluster created with Testing preset");

    // Example 4: Error handling
    println!("\n🚨 Example 4: Error Handling");
    println!("-----------------------------");

    // Try to create cluster without required fields
    let result = RaftCluster::<ExampleRaftStateMachine>::builder()
        .bind_address("127.0.0.1:8080")
        // Missing node_id and state_machine
        .build()
        .await;

    match result {
        Err(e) => println!("✅ Correctly caught configuration error: {}", e),
        Ok(_) => println!("❌ Should have failed with missing configuration"),
    }

    // Example 5: Batch operations
    println!("\n🔄 Example 5: Batch Operations");
    println!("------------------------------");

    // Simulate batch updates
    let batch_commands = vec![
        KvCommand::Set { key: "counter".to_string(), value: "1".to_string() },
        KvCommand::Set { key: "status".to_string(), value: "active".to_string() },
        KvCommand::Set { key: "version".to_string(), value: "2.1.0".to_string() },
    ];

    for command in batch_commands {
        cluster.propose(command).await?;
    }

    let final_state = cluster.query();
    println!("📊 Final state after batch operations:");
    println!("   Total keys: {}", final_state.len());
    println!("   Operations: {}", final_state.metadata.operation_count);

    // Example 6: Metrics and monitoring
    println!("\n📈 Example 6: Metrics");
    println!("---------------------");

    let metrics = cluster.metrics().await;
    println!("📊 Cluster metrics:");
    println!("   Node ID: {}", metrics.node_id);
    println!("   Peer count: {}", metrics.peer_count);
    println!("   Is leader: {}", metrics.is_leader);
    println!("   Term: {}", metrics.term);
    println!("   Commit index: {}", metrics.commit_index);

    // Example 7: Deletion operations
    println!("\n🗑️  Example 7: Deletion Operations");
    println!("----------------------------------");

    cluster.propose(KvCommand::Delete {
        key: "config:timeout".to_string(),
    }).await?;

    let state_after_delete = cluster.query();
    println!("📊 State after deletion:");
    println!("   Total keys: {}", state_after_delete.len());
    
    // Verify the key is gone
    if state_after_delete.get("config:timeout").is_none() {
        println!("✅ Key 'config:timeout' successfully deleted");
    }

    // Example 8: Development vs Production presets
    println!("\n🏭 Example 8: Production Configuration");
    println!("--------------------------------------");

    let mut prod_cluster = RaftCluster::builder()
        .node_id(3)
        .bind_address("127.0.0.1:10080")
        .preset(Preset::Production)  // Conservative timeouts for production
        .peers(vec![
            "node1.example.com:8080",
            "node2.example.com:8080",
            "node3.example.com:8080"
        ])
        .data_dir("/var/lib/raft/node3")
        .state_machine(ExampleRaftStateMachine::new_kv_store())
        .build()
        .await?;

    println!("✅ Production cluster configured");
    println!("   Data directory: /var/lib/raft/node3");
    println!("   External peers: {:?}", prod_cluster.peers());

    // Example 9: Graceful shutdown
    println!("\n🛑 Example 9: Graceful Shutdown");
    println!("-------------------------------");

    println!("Shutting down clusters...");
    cluster.shutdown().await?;
    test_cluster.shutdown().await?;
    prod_cluster.shutdown().await?;

    println!("✅ All clusters shut down gracefully");

    println!("\n🎉 Simple Cluster Example completed successfully!");
    println!("=================================================");
    println!();
    println!("Key takeaways:");
    println!("• Builder pattern provides clean, readable configuration");
    println!("• Presets simplify common deployment scenarios");
    println!("• Error handling guides users to correct configuration");
    println!("• Simple API abstracts away Raft complexity");
    println!("• Graceful shutdown ensures proper resource cleanup");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cluster_builder_validation() {
        // Test missing node_id
        let result = RaftCluster::<ExampleRaftStateMachine>::builder()
            .bind_address("127.0.0.1:8080")
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await;
        assert!(result.is_err());

        // Test missing state_machine  
        let result = RaftCluster::<ExampleRaftStateMachine>::builder()
            .node_id(1)
            .bind_address("127.0.0.1:8080")
            .build()
            .await;
        assert!(result.is_err());

        // Test valid configuration
        let result = RaftCluster::builder()
            .node_id(1)
            .bind_address("127.0.0.1:8080")
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cluster_operations() {
        let mut cluster = RaftCluster::builder()
            .node_id(1)
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await
            .unwrap();

        // Test propose
        let result = cluster.propose(KvCommand::Set {
            key: "test".to_string(),
            value: "value".to_string(),
        }).await;
        assert!(result.is_ok());

        // Test query
        let state = cluster.query();
        assert_eq!(state.get("test"), Some(&"value".to_string()));

        // Test metrics
        let metrics = cluster.metrics().await;
        assert_eq!(metrics.node_id, 1);
    }

    #[tokio::test]
    async fn test_presets() {
        let dev_cluster = RaftCluster::builder()
            .node_id(1)
            .preset(Preset::Development)
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await
            .unwrap();

        let test_cluster = RaftCluster::builder()
            .node_id(2)
            .preset(Preset::Testing)
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await
            .unwrap();

        let prod_cluster = RaftCluster::builder()
            .node_id(3)
            .preset(Preset::Production)
            .state_machine(ExampleRaftStateMachine::new_kv_store())
            .build()
            .await
            .unwrap();

        // All should be created successfully
        assert_eq!(dev_cluster.node_id(), 1);
        assert_eq!(test_cluster.node_id(), 2);
        assert_eq!(prod_cluster.node_id(), 3);
    }
}