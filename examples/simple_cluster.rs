//! Simple cluster example showing the high-level RaftCluster API
//! 
//! This example demonstrates the improved ergonomics of the iroh-raft
//! cluster API, showing how to create a cluster, propose commands,
//! execute queries, and perform administrative operations.

use iroh_raft::prelude::*;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize simple logging
    println!("📝 Logger initialized");

    println!("🚀 Starting simple cluster example");

    // Create cluster configuration with development preset
    let config = ConfigBuilder::development_preset()
        .node_id(1)
        .data_dir("./data/node1")
        .bind_address("127.0.0.1:8080")
        .build()?;

    println!("📝 Configuration created with node_id: {:?}", config.node.id);

    // Create and start cluster
    let mut cluster: RaftCluster<KeyValueStore> = RaftCluster::new(config).await?;
    println!("🎯 Cluster created successfully");

    // Get initial cluster status
    let status = cluster.status().await;
    println!("📊 Initial cluster status: role={:?}, term={}", status.role, status.term);

    // Propose some commands to the cluster
    println!("\n🔄 Proposing commands...");
    
    // Note: These will fail with NotImplemented until the full Raft integration is complete
    // but they demonstrate the improved API ergonomics
    
    let commands = vec![
        KvCommand::Set {
            key: "user:123".to_string(),
            value: "Alice".to_string(),
        },
        KvCommand::Set {
            key: "user:456".to_string(),
            value: "Bob".to_string(),
        },
        KvCommand::Set {
            key: "config:timeout".to_string(),
            value: "30s".to_string(),
        },
    ];

    for (i, command) in commands.into_iter().enumerate() {
        match timeout(Duration::from_millis(100), cluster.propose(command.clone())).await {
            Ok(Ok(response)) => {
                println!("✅ Command {} succeeded: {:?}", i + 1, response);
            }
            Ok(Err(e)) => {
                println!("❌ Command {} failed: {}", i + 1, e);
            }
            Err(_) => {
                println!("⏰ Command {} timed out (expected during development)", i + 1);
            }
        }
    }

    // Execute read-only queries
    println!("\n🔍 Executing queries...");
    
    let queries = vec![
        KvQuery::Get { key: "user:123".to_string() },
        KvQuery::List { prefix: Some("user:".to_string()) },
        KvQuery::Exists { key: "config:timeout".to_string() },
        KvQuery::Size,
    ];

    for (i, query) in queries.into_iter().enumerate() {
        match timeout(Duration::from_millis(100), cluster.query(query.clone())).await {
            Ok(Ok(response)) => {
                println!("✅ Query {} succeeded: {:?}", i + 1, response);
            }
            Ok(Err(e)) => {
                println!("❌ Query {} failed: {}", i + 1, e);
            }
            Err(_) => {
                println!("⏰ Query {} timed out (expected during development)", i + 1);
            }
        }
    }

    // Demonstrate administrative operations
    println!("\n🔧 Administrative operations...");

    // Health check
    let health = cluster.health_check().await;
    println!("🏥 Cluster health: {:?}", health.overall);

    // Get cluster information
    let info = cluster.cluster_info().await;
    println!("📋 Cluster info: {} nodes", info.membership.members.len());

    // Try to take a snapshot (will fail with NotImplemented currently)
    match timeout(Duration::from_millis(100), cluster.take_snapshot()).await {
        Ok(Ok(snapshot_id)) => {
            println!("📸 Snapshot created: {}", snapshot_id);
        }
        Ok(Err(e)) => {
            println!("❌ Snapshot failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Snapshot timed out (expected during development)");
        }
    }

    // Try member management (these would typically be used in multi-node clusters)
    println!("\n👥 Member management...");
    
    let member_config = MemberConfig {
        node_id: 2,
        address: "127.0.0.1:8081".to_string(),
        metadata: None,
    };

    match timeout(Duration::from_millis(100), cluster.add_member(member_config)).await {
        Ok(Ok(())) => {
            println!("✅ Member added successfully");
        }
        Ok(Err(e)) => {
            println!("❌ Add member failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Add member timed out (expected during development)");
        }
    }

    // Get final status
    let final_status = cluster.status().await;
    println!("\n📊 Final cluster status:");
    println!("   Role: {:?}", final_status.role);
    println!("   Term: {}", final_status.term);
    println!("   Members: {:?}", final_status.members);
    println!("   Health: {:?}", final_status.health.overall);

    // Graceful shutdown
    println!("\n🛑 Shutting down cluster...");
    cluster.shutdown().await?;
    println!("✅ Cluster shut down successfully");

    println!("\n🎉 Simple cluster example completed!");
    Ok(())
}
