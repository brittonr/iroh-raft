//! Production cluster example with advanced configuration
//! 
//! This example shows how to configure a cluster for production use
//! with proper error handling, monitoring, and operational practices.

use iroh_raft::prelude::*;
use std::time::Duration;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize structured logging for production
    tracing_subscriber::fmt()
        .with_env_filter("info,iroh_raft=debug")
        .json()
        .init();

    println!("🏭 Starting production cluster example");

    // Create production configuration
    let config = ConfigBuilder::production_preset()
        .node_id(1)
        .data_dir("/var/lib/raft/node1")
        .bind_address("0.0.0.0:8080")
        .add_peer("10.0.1.2:8080")
        .add_peer("10.0.1.3:8080")
        .node_name("raft-node-1")
        .node_zone("us-west-2a")
        .add_node_tag("role", "primary")
        .add_node_tag("datacenter", "us-west-2")
        .metrics(true)
        .metrics_address("0.0.0.0:9090")
        .log_level("info")
        .build()?;

    println!("📝 Production configuration created");

    // Create cluster with advanced error handling
    let mut cluster: RaftCluster<KeyValueStore> = match RaftCluster::new(config).await {
        Ok(cluster) => {
            println!("✅ Cluster initialized successfully");
            cluster
        }
        Err(e) => {
            eprintln!("❌ Failed to initialize cluster: {}", e);
            std::process::exit(1);
        }
    };

    // Set up graceful shutdown handling
    let shutdown_signal = signal::ctrl_c();
    tokio::pin!(shutdown_signal);

    // Production operation loop
    let mut operation_count = 0;
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            // Handle shutdown signal
            _ = &mut shutdown_signal => {
                println!("🛑 Received shutdown signal");
                break;
            }
            
            // Periodic health monitoring
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                let health = cluster.health_check().await;
                match health.overall {
                    Health::Healthy => {
                        println!("✅ Cluster health check: healthy");
                    }
                    Health::Degraded => {
                        println!("⚠️  Cluster health check: degraded");
                        // In production, you might want to alert here
                    }
                    Health::Unhealthy => {
                        println!("❌ Cluster health check: unhealthy");
                        // In production, this might trigger failover or alerts
                    }
                }
            }
            
            // Simulate application operations
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                operation_count += 1;
                
                // Simulate various operations
                match operation_count % 10 {
                    0..=6 => {
                        // Write operations (70% of traffic)
                        let command = match operation_count % 3 {
                            0 => KvCommand::Set {
                                key: format!("session:{}", operation_count),
                                value: format!("data-{}", operation_count),
                            },
                            1 => KvCommand::Set {
                                key: format!("config:setting_{}", operation_count % 100),
                                value: format!("value_{}", operation_count),
                            },
                            _ => KvCommand::Delete {
                                key: format!("session:{}", operation_count - 50),
                            },
                        };

                        // Propose command with timeout and error handling
                        match tokio::time::timeout(Duration::from_secs(5), cluster.propose(command)).await {
                            Ok(Ok(_response)) => {
                                // Success - in production you might log metrics
                            }
                            Ok(Err(e)) => {
                                eprintln!("⚠️  Command failed: {}", e);
                                // In production, track error rates and alert if needed
                            }
                            Err(_) => {
                                eprintln!("⏰ Command timed out");
                                // In production, this might indicate cluster issues
                            }
                        }
                    }
                    _ => {
                        // Read operations (30% of traffic)
                        let query = match operation_count % 4 {
                            0 => KvQuery::Get { key: format!("session:{}", operation_count - 10) },
                            1 => KvQuery::List { prefix: Some("config:".to_string()) },
                            2 => KvQuery::Exists { key: format!("session:{}", operation_count - 5) },
                            _ => KvQuery::Size,
                        };

                        match tokio::time::timeout(Duration::from_millis(100), cluster.query(query)).await {
                            Ok(Ok(_response)) => {
                                // Success
                            }
                            Ok(Err(e)) => {
                                eprintln!("⚠️  Query failed: {}", e);
                            }
                            Err(_) => {
                                eprintln!("⏰ Query timed out");
                            }
                        }
                    }
                }

                // Periodic administrative tasks
                if operation_count % 1000 == 0 {
                    let uptime = start_time.elapsed();
                    println!("📊 Operations: {}, Uptime: {:?}", operation_count, uptime);

                    // Get cluster status
                    let status = cluster.status().await;
                    println!("   Role: {:?}, Term: {}, Members: {}", 
                             status.role, status.term, status.members.len());

                    // In production, you might:
                    // - Send metrics to monitoring systems
                    // - Check if maintenance is needed
                    // - Rotate logs
                    // - Create periodic snapshots
                }

                // Trigger maintenance operations
                if operation_count % 5000 == 0 {
                    println!("🔧 Performing maintenance operations...");

                    // Try to create a snapshot for backup
                    match tokio::time::timeout(Duration::from_secs(30), cluster.take_snapshot()).await {
                        Ok(Ok(snapshot_id)) => {
                            println!("📸 Snapshot created: {}", snapshot_id);
                        }
                        Ok(Err(e)) => {
                            eprintln!("❌ Snapshot failed: {}", e);
                        }
                        Err(_) => {
                            eprintln!("⏰ Snapshot timed out");
                        }
                    }

                    // Try to compact logs if needed
                    if operation_count > 10000 {
                        let retain_index = (operation_count / 2) as u64;
                        match tokio::time::timeout(Duration::from_secs(10), cluster.compact_log(retain_index)).await {
                            Ok(Ok(())) => {
                                println!("🗜️  Log compaction completed");
                            }
                            Ok(Err(e)) => {
                                eprintln!("❌ Log compaction failed: {}", e);
                            }
                            Err(_) => {
                                eprintln!("⏰ Log compaction timed out");
                            }
                        }
                    }
                }

                // Exit after demonstration
                if operation_count >= 100 {
                    println!("🎯 Demonstration complete, initiating shutdown");
                    break;
                }
            }
        }
    }

    // Graceful shutdown with proper cleanup
    println!("🛑 Initiating graceful shutdown...");
    
    // Get final statistics
    let info = cluster.cluster_info().await;
    println!("📊 Final statistics:");
    println!("   Total operations: {}", operation_count);
    println!("   Uptime: {:?}", start_time.elapsed());
    println!("   Cluster members: {}", info.membership.members.len());

    // Shutdown the cluster
    match tokio::time::timeout(Duration::from_secs(30), cluster.shutdown()).await {
        Ok(Ok(())) => {
            println!("✅ Cluster shut down successfully");
        }
        Ok(Err(e)) => {
            eprintln!("⚠️  Cluster shutdown error: {}", e);
        }
        Err(_) => {
            eprintln!("⏰ Cluster shutdown timed out");
        }
    }

    println!("🎉 Production cluster example completed!");
    Ok(())
}
