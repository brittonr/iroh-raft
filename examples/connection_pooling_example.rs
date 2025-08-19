//! Example demonstrating connection pooling configuration and usage
//!
//! This example shows how to configure and use connection pooling for optimal
//! performance in high-throughput Raft deployments.

use iroh_raft::config::{ConfigBuilder, ConnectionConfig, TransportConfig};
use iroh_raft::metrics::MetricsRegistry;
use iroh_raft::transport::iroh::IrohRaftTransport;
use std::net::SocketAddr;
use std::time::Duration;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connection Pooling Configuration Example");
    println!("=======================================");

    // Initialize logging
    tracing_subscriber::init();

    // 1. Basic Connection Pool Configuration
    println!("\n1. Basic Connection Pool Configuration:");
    
    let connection_config = ConnectionConfig {
        // Enable connection pooling for better performance
        enable_pooling: true,
        
        // Allow up to 3 connections per peer for parallelism
        max_connections_per_peer: 3,
        
        // 10 second timeout for new connections
        connect_timeout: Duration::from_secs(10),
        
        // Send keep-alives every 30 seconds
        keep_alive_interval: Duration::from_secs(30),
        
        // Close idle connections after 5 minutes
        idle_timeout: Duration::from_secs(300),
        
        // Limit messages to 4MB (including large snapshots)
        max_message_size: 4 * 1024 * 1024,
    };
    
    println!("   Connection pooling: {}", connection_config.enable_pooling);
    println!("   Max connections per peer: {}", connection_config.max_connections_per_peer);
    println!("   Connect timeout: {:?}", connection_config.connect_timeout);
    println!("   Idle timeout: {:?}", connection_config.idle_timeout);

    // 2. High-Performance Configuration
    println!("\n2. High-Performance Configuration (for busy clusters):");
    
    let high_perf_config = ConnectionConfig {
        enable_pooling: true,
        max_connections_per_peer: 10,  // More parallelism
        connect_timeout: Duration::from_secs(5),   // Faster failure detection
        keep_alive_interval: Duration::from_secs(15), // More frequent health checks
        idle_timeout: Duration::from_secs(600),    // Keep connections longer
        max_message_size: 16 * 1024 * 1024,       // Larger message limit
    };
    
    println!("   Max connections per peer: {}", high_perf_config.max_connections_per_peer);
    println!("   Connect timeout: {:?}", high_perf_config.connect_timeout);
    println!("   Keep-alive interval: {:?}", high_perf_config.keep_alive_interval);

    // 3. Resource-Constrained Configuration  
    println!("\n3. Resource-Constrained Configuration (for small deployments):");
    
    let low_resource_config = ConnectionConfig {
        enable_pooling: false,  // Disable pooling to save memory
        max_connections_per_peer: 1,
        connect_timeout: Duration::from_secs(30), // More patient on slow networks
        keep_alive_interval: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(120),   // Close connections sooner
        max_message_size: 1024 * 1024,           // Smaller message limit
    };
    
    println!("   Connection pooling: {}", low_resource_config.enable_pooling);
    println!("   Max connections per peer: {}", low_resource_config.max_connections_per_peer);
    println!("   Idle timeout: {:?}", low_resource_config.idle_timeout);

    // 4. Full Configuration Example
    println!("\n4. Building Complete Configuration with Connection Pooling:");
    
    let config = ConfigBuilder::new()
        .node_id(1)
        .data_dir("./data/node1")
        .bind_address("127.0.0.1:8080")
        .add_peer("127.0.0.1:8081")
        .add_peer("127.0.0.1:8082")
        .transport_config(TransportConfig {
            bind_address: SocketAddr::from_str("127.0.0.1:8080")?,
            peers: vec![
                SocketAddr::from_str("127.0.0.1:8081")?,
                SocketAddr::from_str("127.0.0.1:8082")?,
            ],
            connection: connection_config.clone(),
            iroh: Default::default(),
        })
        .build()?;
    
    println!("   Node ID: {}", config.node.node_id);
    println!("   Bind address: {}", config.transport.bind_address);
    println!("   Peer count: {}", config.transport.peers.len());
    println!("   Pooling enabled: {}", config.transport.connection.enable_pooling);

    // 5. Demonstrate Metrics Collection with Connection Pooling
    println!("\n5. Metrics Collection for Connection Pool Monitoring:");
    
    #[cfg(feature = "metrics-otel")]
    {
        let metrics = MetricsRegistry::new("connection-pool-example")?;
        
        // Transport metrics track connection pool performance
        let transport_metrics = metrics.transport_metrics();
        
        // These would be called automatically by the transport layer
        transport_metrics.record_connection_established("peer-1");
        transport_metrics.record_message_sent("append_entries", "peer-1");
        transport_metrics.record_message_received("vote_request", "peer-2");
        
        println!("   Metrics registry created for monitoring");
        println!("   Transport metrics available for connection monitoring");
        
        // Export metrics for monitoring systems
        let prometheus_output = metrics.export_prometheus()?;
        println!("   Prometheus metrics available:");
        println!("   {}", prometheus_output.lines().take(3).collect::<Vec<_>>().join("\n"));
    }
    
    #[cfg(not(feature = "metrics-otel"))]
    {
        println!("   Metrics collection disabled (enable 'metrics-otel' feature)");
    }

    // 6. Connection Pool Tuning Guidelines
    println!("\n6. Connection Pool Tuning Guidelines:");
    println!("   
   📊 Performance Tuning:
   
   For High Throughput:
   - enable_pooling: true
   - max_connections_per_peer: 5-10
   - shorter keep_alive_interval (15-30s)
   - longer idle_timeout (300-600s)
   
   For Low Latency:
   - enable_pooling: true  
   - max_connections_per_peer: 2-5
   - shorter connect_timeout (5-10s)
   - frequent health checks
   
   For Resource Efficiency:
   - enable_pooling: false (if very low traffic)
   - max_connections_per_peer: 1-2
   - shorter idle_timeout (60-120s)
   - larger message size limits only if needed
   
   🔧 Monitoring:
   - Track active_connections via get_metrics()
   - Monitor connection establishment rates
   - Watch for connection timeout errors
   - Observe message throughput changes
   ");

    // 7. Best Practices Summary
    println!("\n7. Best Practices Summary:");
    println!("   ✅ Enable pooling for clusters with >3 nodes");
    println!("   ✅ Set max_connections_per_peer based on message volume");
    println!("   ✅ Use shorter timeouts for fast failure detection");
    println!("   ✅ Monitor metrics to tune configuration");
    println!("   ✅ Test configuration under expected load");
    println!("   ⚠️  Don't over-allocate connections on resource-constrained nodes");
    println!("   ⚠️  Don't set timeouts too aggressively on slow networks");

    println!("\nConnection pooling configuration example completed!");
    Ok(())
}

/// Helper function to demonstrate connection pool monitoring
async fn demonstrate_connection_monitoring() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📈 Connection Pool Monitoring Example:");
    
    // This would typically be done in your application's monitoring loop
    /* 
    let transport = IrohRaftTransport::new(config).await?;
    
    // Monitor connection pool health
    let metrics = transport.get_metrics().await;
    println!("Active connections: {}", metrics.active_connections);
    println!("Messages sent: {}", metrics.messages_sent);
    println!("Messages received: {}", metrics.messages_received);
    
    // Check if pool is healthy
    if metrics.active_connections == 0 {
        println!("⚠️  Warning: No active connections to peers");
    } else if metrics.active_connections > 50 {
        println!("⚠️  Warning: Very high connection count, consider tuning");
    } else {
        println!("✅ Connection pool healthy");
    }
    */
    
    println!("   Connection monitoring helps detect:");
    println!("   - Network partitions (active_connections drops)");
    println!("   - Resource leaks (connections growing without bound)");
    println!("   - Performance issues (low message throughput)");
    
    Ok(())
}