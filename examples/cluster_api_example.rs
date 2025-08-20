//! Example demonstrating the new high-level cluster API
//!
//! This example shows how to use the simplified iroh-raft cluster API
//! to create a distributed key-value store with minimal boilerplate.
//!
//! Run with: cargo run --example cluster_api_example --features cluster-api

use iroh_raft::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use tokio::time::{sleep, Duration};

/// Commands that can be applied to our key-value store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvCommand {
    /// Set a key-value pair
    Set { key: String, value: String },
    /// Delete a key
    Delete { key: String },
    /// Clear all data
    Clear,
}

/// State of our key-value store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvState {
    /// The actual key-value data
    pub data: HashMap<String, String>,
    /// Metadata about the state
    pub version: u64,
    /// Number of operations performed
    pub operation_count: u64,
}

/// Our simple key-value store state machine
pub struct KeyValueStateMachine {
    state: KvState,
}

impl KeyValueStateMachine {
    pub fn new() -> Self {
        Self {
            state: KvState {
                data: HashMap::new(),
                version: 0,
                operation_count: 0,
            },
        }
    }
}

impl ClusterStateMachine for KeyValueStateMachine {
    type Command = KvCommand;
    type State = KvState;
    type Error = Box<dyn Error + Send + Sync>;

    async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error> {
        self.state.operation_count += 1;
        self.state.version += 1;

        match command {
            KvCommand::Set { key, value } => {
                println!("  📝 Setting {} = {}", key, value);
                self.state.data.insert(key, value);
            }
            KvCommand::Delete { key } => {
                if let Some(old_value) = self.state.data.remove(&key) {
                    println!("  🗑️  Deleted {} (was: {})", key, old_value);
                } else {
                    println!("  ⚠️  Key {} not found for deletion", key);
                }
            }
            KvCommand::Clear => {
                let count = self.state.data.len();
                self.state.data.clear();
                println!("  🧹 Cleared {} keys", count);
            }
        }

        Ok(())
    }

    async fn snapshot(&self) -> Result<Self::State, Self::Error> {
        println!("  📸 Creating snapshot with {} keys", self.state.data.len());
        Ok(self.state.clone())
    }

    async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error> {
        println!("  🔄 Restoring from snapshot with {} keys", snapshot.data.len());
        self.state = snapshot;
        Ok(())
    }

    fn query(&self) -> &Self::State {
        &self.state
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("🚀 Starting iroh-raft cluster API example\n");

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("📋 Creating cluster configuration...");
    
    // Create a single-node cluster (for demonstration)
    // In production, you would join multiple nodes
    let cluster = RaftCluster::builder(1)
        .bind_address("127.0.0.1:0".parse()?) // Use random port
        .data_dir("/tmp/iroh-raft-cluster-example")
        .election_timeout(Duration::from_millis(3000))
        .heartbeat_interval(Duration::from_millis(1000))
        .log_level("info")
        .debug(true)
        .build(KeyValueStateMachine::new())
        .await?;

    println!("✅ Cluster created successfully!\n");

    // Get initial cluster status
    let status = cluster.status().await;
    println!("📊 Initial cluster status:");
    println!("  - Cluster ID: {}", status.cluster_id);
    println!("  - Node ID: {}", status.nodes[0].id);
    println!("  - Node State: {:?}", status.state);
    println!("  - Nodes: {}", status.nodes.len());
    println!();

    // Check cluster health
    let health = cluster.health().await;
    println!("🏥 Cluster health: {:?}", health.status);
    for check in &health.checks {
        println!("  - {}: {:?} - {}", check.name, check.status, check.message);
    }
    println!();

    // Demonstrate proposing commands
    println!("💡 Proposing commands to the cluster...\n");

    // Set some key-value pairs
    cluster.propose(KvCommand::Set {
        key: "user:123".to_string(),
        value: "Alice".to_string(),
    }).await?;

    cluster.propose(KvCommand::Set {
        key: "user:456".to_string(),
        value: "Bob".to_string(),
    }).await?;

    cluster.propose(KvCommand::Set {
        key: "config:timeout".to_string(),
        value: "30s".to_string(),
    }).await?;

    // Query the current state
    let state = cluster.query().await;
    println!("\n📊 Current state after insertions:");
    println!("  - Keys: {}", state.data.len());
    println!("  - Version: {}", state.version);
    println!("  - Operations: {}", state.operation_count);
    println!("  - Data:");
    for (key, value) in &state.data {
        println!("    {} = {}", key, value);
    }
    println!();

    // Delete a key
    cluster.propose(KvCommand::Delete {
        key: "user:456".to_string(),
    }).await?;

    // Query state again
    let state = cluster.query().await;
    println!("📊 State after deletion:");
    println!("  - Keys: {}", state.data.len());
    println!("  - Version: {}", state.version);
    println!("  - Operations: {}", state.operation_count);
    println!("  - Data:");
    for (key, value) in &state.data {
        println!("    {} = {}", key, value);
    }
    println!();

    // Demonstrate administrative operations
    println!("🔧 Testing administrative operations...\n");

    // Simulate adding a node (in real scenarios, this would be a real node)
    let add_result = cluster.add_node(2, "127.0.0.1:8081".parse()?).await?;
    println!("➕ Add node result: {}", add_result.message);

    // Check updated status
    let status = cluster.status().await;
    println!("📊 Updated cluster status:");
    println!("  - Nodes: {}", status.nodes.len());
    for node in &status.nodes {
        println!("    Node {}: {:?} at {} ({})", 
                 node.id, node.role, node.address, 
                 match node.status {
                     crate::cluster_api::NodeStatus::Online => "Online",
                     crate::cluster_api::NodeStatus::Offline => "Offline", 
                     crate::cluster_api::NodeStatus::Unknown => "Unknown",
                 });
    }
    println!();

    // Remove the node we just added
    let remove_result = cluster.remove_node(2).await?;
    println!("➖ Remove node result: {}", remove_result.message);

    // Final health check
    println!("🏥 Final health check...");
    let health = cluster.health().await;
    println!("  Overall status: {:?}", health.status);
    println!("  Summary: {}", health.summary);
    println!();

    // Demonstrate graceful shutdown
    println!("🛑 Shutting down cluster gracefully...");
    cluster.shutdown().await?;
    println!("✅ Cluster shutdown complete!");

    println!("\n🎉 Cluster API example completed successfully!");
    println!("\nKey benefits demonstrated:");
    println!("  ✅ Simple cluster setup with sensible defaults");
    println!("  ✅ Easy command proposal and state querying");
    println!("  ✅ Built-in health checking and status monitoring");
    println!("  ✅ Administrative operations (add/remove nodes)");
    println!("  ✅ Graceful shutdown handling");
    println!("  ✅ Type-safe state machine implementation");

    Ok(())
}