//! Example demonstrating the management HTTP API
//!
//! This example shows how to:
//! - Start a management HTTP server
//! - Use REST endpoints for cluster management
//! - Stream events via WebSocket
//! - Monitor health and metrics
//!
//! Run with: cargo run --features management-api --example management_api_example

use iroh_raft::{
    management::{ManagementConfig, ManagementServer, handlers::ClusterHandle},
    prelude::*,
};
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize simple tracing
    use tracing_subscriber;
    tracing_subscriber::fmt::init();

    info!("Starting iroh-raft management API example");

    // Create a cluster handle (in real usage this would be your actual Raft cluster)
    let cluster_handle = Arc::new(ClusterHandle::new());

    // Configure the management API server
    let bind_addr = "127.0.0.1:8080".parse().expect("Invalid bind address");
    let management_config = ManagementConfig::builder()
        .bind_address(bind_addr)
        .enable_swagger_ui(true)
        .enable_cors(true)
        .detailed_errors(true)
        .enable_rate_limiting()
        .build();

    info!("Management server configuration:");
    info!("  Bind address: {}", management_config.bind_address);
    info!("  Swagger UI: {}", management_config.enable_swagger_ui);
    info!("  CORS: {}", management_config.enable_cors);
    info!("  WebSocket: {}", management_config.websocket.enabled);

    // Create and start the management server
    let server = ManagementServer::new(cluster_handle.clone(), management_config);

    info!("Starting management server...");
    info!("Available endpoints:");
    info!("  Health check:     GET  http://127.0.0.1:8080/v1/health");
    info!("  Readiness check:  GET  http://127.0.0.1:8080/v1/ready");
    info!("  Cluster status:   GET  http://127.0.0.1:8080/v1/cluster/status");
    info!("  Add member:       POST http://127.0.0.1:8080/v1/cluster/members");
    info!("  Remove member:    DEL  http://127.0.0.1:8080/v1/cluster/members/{{id}}");
    info!("  Transfer leader:  POST http://127.0.0.1:8080/v1/leadership/transfer");
    info!("  Metrics:          GET  http://127.0.0.1:8080/v1/metrics");
    info!("  WebSocket events: WS   http://127.0.0.1:8080/v1/events/ws");
    info!("  Server events:    GET  http://127.0.0.1:8080/v1/events/sse");
    info!("  API docs:         GET  http://127.0.0.1:8080/swagger-ui/");

    // Start a background task to simulate cluster events (placeholder)
    info!("Note: Event simulation not implemented with ClusterHandle placeholder");

    // Start the server (this blocks until shutdown)
    match server.serve().await {
        Ok(()) => info!("Management server shut down gracefully"),
        Err(e) => warn!("Management server error: {}", e),
    }

    Ok(())
}

// Note: In a real implementation, you would use your actual Raft cluster handle here

/// Example of making HTTP requests to the management API
#[allow(dead_code)]
async fn example_api_usage() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("HTTP client example not implemented in this simplified demo");
    println!("You can test the API using curl or any HTTP client:");
    println!();
    println!("# Check health:");
    println!("curl http://127.0.0.1:8080/v1/health");
    println!();
    println!("# Get cluster status:");
    println!("curl http://127.0.0.1:8080/v1/cluster/status");
    println!();
    println!("# Get metrics:");
    println!("curl http://127.0.0.1:8080/v1/metrics");
    println!();
    println!("# Add a member:");
    println!("curl -X POST http://127.0.0.1:8080/v1/cluster/members \\");
    println!("  -H \"Content-Type: application/json\" \\");
    println!("  -d '{{\"node_id\": 4, \"address\": \"127.0.0.1:8083\", \"role\": \"voter\"}}'");
    
    Ok(())
}

/// Example of WebSocket event streaming (simplified)
#[allow(dead_code)]
async fn example_websocket_usage() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("WebSocket example not implemented in this simplified demo");
    println!("In a real application, you would:");
    println!("1. Connect to ws://127.0.0.1:8080/v1/events/ws");
    println!("2. Send subscription messages for specific event types");
    println!("3. Listen for real-time cluster events");
    Ok(())
}