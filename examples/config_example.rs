//! Configuration example for iroh-raft
//! 
//! This example demonstrates how to use the configuration system
//! with the builder pattern and environment variables.

use iroh_raft::{ConfigBuilder, ConfigError};
use std::time::Duration;

fn main() -> Result<(), ConfigError> {
    // Example 1: Using the builder pattern
    println!("=== Example 1: Builder Pattern ===");
    
    let config = ConfigBuilder::new()
        .node_id(1)
        .data_dir("./example-data")
        .bind_address("127.0.0.1:8080")
        .add_peer("127.0.0.1:8081")
        .add_peer("127.0.0.1:8082")
        .election_timeout(Duration::from_millis(5000))
        .heartbeat_interval(Duration::from_millis(1000))
        .log_level("debug")
        .metrics(true)
        .pre_vote(true)
        .log_compaction(true, 10000)
        .snapshots(true, 50000, 3)
        .cache_size_mb(256)
        .node_name("example-node")
        .node_zone("us-west-2")
        .add_node_tag("role", "primary")
        .add_node_tag("environment", "development")
        .build()?;

    println!("Created configuration:");
    println!("  Node ID: {:?}", config.node.id);
    println!("  Data directory: {:?}", config.node.data_dir);
    println!("  Bind address: {}", config.transport.bind_address);
    println!("  Peers: {:?}", config.transport.peers);
    println!("  Election timeout: {:?}", config.raft.election_timeout_min);
    println!("  Heartbeat interval: {:?}", config.raft.heartbeat_interval);
    println!("  Log level: {}", config.observability.logging.level);
    println!("  Pre-vote enabled: {}", config.raft.pre_vote);
    println!("  Compaction enabled: {}", config.raft.compaction.enabled);
    println!("  Snapshot enabled: {}", config.raft.snapshot.enabled);
    println!("  Node name: {:?}", config.node.metadata.name);
    println!("  Node tags: {:?}", config.node.metadata.tags);
    println!();

    // Example 2: Save and load from file
    println!("=== Example 2: File Operations ===");
    
    let config_path = "example-config.toml";
    config.save_to_file(config_path)?;
    println!("Saved configuration to {}", config_path);
    
    let loaded_config = iroh_raft::Config::from_file(config_path)?;
    println!("Loaded configuration from file:");
    println!("  Node ID: {:?}", loaded_config.node.id);
    println!("  Peers count: {}", loaded_config.transport.peers.len());
    println!();

    // Example 3: Environment variable demonstration
    println!("=== Example 3: Environment Variables ===");
    
    // Set some environment variables
    std::env::set_var("IROH_RAFT_NODE_ID", "42");
    std::env::set_var("IROH_RAFT_LOG_LEVEL", "trace");
    std::env::set_var("IROH_RAFT_ELECTION_TIMEOUT_MS", "3000");
    std::env::set_var("IROH_RAFT_METRICS_ENABLED", "false");
    
    let env_config = iroh_raft::Config::from_env()?;
    println!("Configuration from environment:");
    println!("  Node ID: {:?}", env_config.node.id);
    println!("  Log level: {}", env_config.observability.logging.level);
    println!("  Election timeout: {:?}", env_config.raft.election_timeout_min);
    println!("  Metrics enabled: {}", env_config.observability.metrics.enabled);
    println!();

    // Clean up environment variables
    std::env::remove_var("IROH_RAFT_NODE_ID");
    std::env::remove_var("IROH_RAFT_LOG_LEVEL");
    std::env::remove_var("IROH_RAFT_ELECTION_TIMEOUT_MS");
    std::env::remove_var("IROH_RAFT_METRICS_ENABLED");

    // Example 4: Validation demonstration
    println!("=== Example 4: Configuration Validation ===");
    
    // Try to create an invalid configuration
    println!("Attempting to create invalid configuration (no node ID):");
    let invalid_config = ConfigBuilder::new()
        .bind_address("127.0.0.1:8080")
        .build();
    
    match invalid_config {
        Ok(_) => println!("ERROR: Should have failed validation!"),
        Err(e) => println!("✓ Validation correctly failed: {}", e),
    }
    
    // Try invalid timing
    println!("Attempting to create invalid configuration (heartbeat >= election timeout):");
    let invalid_config = ConfigBuilder::new()
        .node_id(1)
        .heartbeat_interval(Duration::from_millis(6000))
        .election_timeout(Duration::from_millis(5000))
        .build();
    
    match invalid_config {
        Ok(_) => println!("ERROR: Should have failed validation!"),
        Err(e) => println!("✓ Validation correctly failed: {}", e),
    }

    // Clean up the config file
    if std::path::Path::new(config_path).exists() {
        std::fs::remove_file(config_path).unwrap();
        println!("Cleaned up {}", config_path);
    }

    println!("\n✓ All examples completed successfully!");
    Ok(())
}