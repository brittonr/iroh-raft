//! Example demonstrating metrics collection and graceful shutdown
//!
//! This example shows how to set up comprehensive metrics collection and
//! implement graceful shutdown procedures for production Raft deployments.

use iroh_raft::config::ConfigBuilder;
use iroh_raft::metrics::{MetricsRegistry, LatencyTimer};
use iroh_raft::transport::iroh::IrohRaftTransport;
use iroh_raft::time_operation;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{mpsc, Notify};
use tokio::time::{interval, sleep};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Metrics Collection and Graceful Shutdown Example");
    println!("===============================================");

    // Initialize logging
    tracing_subscriber::fmt::init();

    // 1. Metrics Registry Setup
    println!("\n1. Setting Up Metrics Collection:");
    
    let metrics = MetricsRegistry::new("iroh-raft-demo")?;
    println!("   ✅ Metrics registry created");
    
    // Get different metric categories
    let raft_metrics = metrics.raft_metrics();
    let storage_metrics = metrics.storage_metrics();
    let transport_metrics = metrics.transport_metrics();
    let node_metrics = metrics.node_metrics();
    
    println!("   📊 Available metric categories:");
    println!("      - Raft consensus metrics");
    println!("      - Storage operation metrics");
    println!("      - Transport/networking metrics");
    println!("      - Node health metrics");

    // 2. Demonstrate Raft Metrics
    println!("\n2. Recording Raft Consensus Metrics:");
    
    // Simulate leader election
    raft_metrics.record_leader_election(1, 42);
    println!("   📈 Leader election recorded (node: 1, term: 42)");
    
    // Simulate log operations with timing
    let log_timer = LatencyTimer::start("log_append");
    sleep(Duration::from_millis(5)).await; // Simulate work
    let latency_ms = log_timer.elapsed_ms();
    raft_metrics.record_log_append(10, latency_ms);
    println!("   📈 Log append recorded (10 entries, {:.2}ms latency)", latency_ms);
    
    // Record consensus errors
    raft_metrics.increment_consensus_errors("append_entries_timeout");
    println!("   📈 Consensus error recorded (append_entries_timeout)");

    // 3. Demonstrate Storage Metrics
    println!("\n3. Recording Storage Operation Metrics:");
    
    // Time a storage operation using the convenience macro
    let data = time_operation!("database_read", {
        sleep(Duration::from_millis(3)).await;
        "simulated data".to_string()
    });
    println!("   📈 Storage read operation timed: {}", data);
    
    // Record storage operations manually
    storage_metrics.record_operation("write", Duration::from_millis(8));
    storage_metrics.record_operation("read", Duration::from_millis(3));
    storage_metrics.update_disk_usage(1024 * 1024 * 100); // 100MB
    println!("   📈 Storage operations and disk usage recorded");

    // 4. Demonstrate Transport Metrics  
    println!("\n4. Recording Transport/Network Metrics:");
    
    transport_metrics.record_connection_established("peer-node-2");
    transport_metrics.record_message_sent("append_entries", "peer-node-2");
    transport_metrics.record_message_received("vote_request", "peer-node-3");
    transport_metrics.record_message_latency("heartbeat", Duration::from_millis(2));
    transport_metrics.record_network_error("connection_timeout");
    
    println!("   📈 Transport metrics recorded:");
    println!("      - Connection established to peer-node-2");
    println!("      - Messages sent/received");
    println!("      - Network latency and errors");

    // 5. Demonstrate Node Health Metrics
    println!("\n5. Recording Node Health Metrics:");
    
    node_metrics.update_node_status(1, true); // Node 1 is up
    node_metrics.update_resource_usage(512 * 1024 * 1024, 45.5); // 512MB RAM, 45.5% CPU
    node_metrics.update_uptime(Duration::from_secs(3600)); // 1 hour uptime
    
    println!("   📈 Node health metrics recorded:");
    println!("      - Node status: healthy");
    println!("      - Resource usage: 512MB RAM, 45.5% CPU");
    println!("      - Uptime: 1 hour");

    // 6. Export Metrics
    println!("\n6. Exporting Metrics for Monitoring:");
    
    let prometheus_output = metrics.export_prometheus()?;
    println!("   📤 Prometheus format export:");
    for line in prometheus_output.lines().take(5) {
        println!("      {}", line);
    }
    println!("      ... (truncated)");
    
    #[cfg(feature = "metrics-otel")]
    {
        println!("   ✅ OpenTelemetry integration enabled");
        println!("   💡 Metrics can be exported to Prometheus, Jaeger, etc.");
    }
    
    #[cfg(not(feature = "metrics-otel"))]
    {
        println!("   ⚠️  OpenTelemetry integration disabled");
        println!("   💡 Enable 'metrics-otel' feature for full functionality");
    }

    // 7. Custom Metrics Example
    println!("\n7. Custom Application Metrics:");
    
    metrics.register_custom_counter("custom_operations_total", "Total custom operations")?;
    println!("   📊 Custom counter registered: custom_operations_total");

    // 8. Graceful Shutdown Demonstration
    println!("\n8. Graceful Shutdown Demonstration:");
    
    // Create shutdown coordination
    let shutdown_notify = Arc::new(Notify::new());
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    
    // Simulate a service with background tasks
    let service_task = {
        let shutdown_notify = shutdown_notify.clone();
        let metrics = metrics.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            let mut counter = 0;
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        counter += 1;
                        
                        // Simulate some Raft operations
                        let raft_metrics = metrics.raft_metrics();
                        if counter % 5 == 0 {
                            raft_metrics.record_leader_election(1, counter);
                        }
                        raft_metrics.record_log_append(1, 2.5);
                        
                        println!("   🔄 Service iteration {} (background work)", counter);
                        
                        if counter >= 3 {
                            // Simulate shutdown signal after a few iterations
                            println!("   📤 Sending shutdown signal...");
                            let _ = shutdown_tx.send(()).await;
                        }
                    }
                    _ = shutdown_notify.notified() => {
                        println!("   🔔 Service received shutdown notification");
                        break;
                    }
                }
            }
            
            println!("   ✅ Service task completed gracefully");
        })
    };
    
    // Wait for shutdown signal
    println!("   ⏳ Waiting for shutdown signal...");
    let _ = shutdown_rx.recv().await;
    
    println!("   🛑 Shutdown signal received, initiating graceful shutdown:");
    
    // Step 1: Stop accepting new work
    println!("   1️⃣ Stopping new work acceptance");
    
    // Step 2: Signal background tasks to stop
    println!("   2️⃣ Signaling background tasks to stop");
    shutdown_notify.notify_waiters();
    
    // Step 3: Wait for tasks to complete
    println!("   3️⃣ Waiting for background tasks to complete");
    if let Err(e) = service_task.await {
        println!("   ⚠️  Task failed during shutdown: {:?}", e);
    }
    
    // Step 4: Final metrics export
    println!("   4️⃣ Final metrics export before shutdown");
    let final_metrics = metrics.export_prometheus()?;
    println!("   📊 Final metrics exported ({} bytes)", final_metrics.len());
    
    // Step 5: Cleanup
    println!("   5️⃣ Cleanup complete");
    
    println!("   ✅ Graceful shutdown completed successfully");

    // 9. Transport-Specific Shutdown Example
    println!("\n9. Transport Layer Shutdown Example:");
    println!("   
   For real transport shutdown:
   ```rust
   // Create transport
   let transport = IrohRaftTransport::new(config).await?;
   
   // In main loop
   tokio::select! {{
       _ = signal::ctrl_c() => {{
           println!(\"Shutdown signal received\");
           
           // Graceful transport shutdown
           transport.shutdown().await;
           
           // Wait for complete shutdown
           transport.wait_for_shutdown().await;
           
           println!(\"Transport shutdown complete\");
       }}
       _ = transport.run() => {{
           println!(\"Transport terminated\");
       }}
   }}
   ```
   ");

    // 10. Best Practices Summary
    println!("\n10. Metrics and Shutdown Best Practices:");
    println!("
   📊 Metrics Best Practices:
   ✅ Record metrics at key decision points
   ✅ Use timing macros for operation latency
   ✅ Monitor both success and error rates
   ✅ Export metrics for external monitoring
   ✅ Set up alerting on key metrics
   
   🛑 Graceful Shutdown Best Practices:
   ✅ Use coordination primitives (Notify, channels)
   ✅ Stop accepting new work first
   ✅ Wait for in-flight operations to complete
   ✅ Export final metrics before shutdown
   ✅ Clean up resources (connections, files)
   ✅ Set reasonable shutdown timeouts
   
   ⚠️  Common Pitfalls:
   ❌ Don't ignore shutdown signals
   ❌ Don't force-kill background tasks
   ❌ Don't skip final metrics export
   ❌ Don't leak resources during shutdown
   ");

    println!("\nMetrics and shutdown example completed!");
    Ok(())
}

/// Example of setting up a monitoring endpoint for metrics
/// This would typically be a separate HTTP server in production
async fn setup_metrics_endpoint(
    metrics: MetricsRegistry
) -> Result<(), Box<dyn std::error::Error>> {
    // In a real application, you'd set up an HTTP server:
    /*
    use warp::Filter;
    
    let metrics_clone = metrics.clone();
    let metrics_route = warp::path("metrics")
        .map(move || {
            let prometheus_output = metrics_clone.export_prometheus()
                .unwrap_or_else(|_| "# Metrics export failed".to_string());
            warp::reply::with_header(
                prometheus_output,
                "content-type",
                "text/plain; version=0.0.4; charset=utf-8"
            )
        });
    
    warp::serve(metrics_route)
        .run(([127, 0, 0, 1], 9090))
        .await;
    */
    
    println!("📡 Metrics endpoint example:");
    println!("   - Expose metrics on HTTP endpoint (e.g., :9090/metrics)");
    println!("   - Use proper content-type for Prometheus");
    println!("   - Consider authentication for sensitive metrics");
    
    Ok(())
}

/// Example signal handler for Unix systems
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");
        
        tokio::select! {
            _ = sigterm.recv() => println!("Received SIGTERM"),
            _ = sigint.recv() => println!("Received SIGINT"),
            _ = signal::ctrl_c() => println!("Received Ctrl+C"),
        }
    }
    
    #[cfg(not(unix))]
    {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("Received Ctrl+C");
    }
}