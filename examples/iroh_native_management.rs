//! Example demonstrating Iroh-native P2P management operations
//!
//! This example shows how to:
//! - Connect to a Raft cluster using P2P discovery
//! - Perform management operations over QUIC streams
//! - Monitor cluster health and events
//! - Handle leadership changes and member management
//!
//! Run with: cargo run --example iroh_native_management

use iroh_raft::{
    management::{
        IrohRaftManagementClient, ClusterEvent, MemberRole, 
        BackupMetadata, MetricCategory, EventFilter,
    },
    prelude::*,
};
use iroh::{Endpoint, NodeAddr, NodeId};
use std::time::{Duration, SystemTime};
use futures_util::StreamExt;
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Starting Iroh-native management example");

    // Example 1: Connect using P2P discovery
    info!("Example 1: Connecting to cluster using P2P discovery");
    connect_with_discovery().await?;

    // Example 2: Direct connection to known members
    info!("\nExample 2: Direct connection to known cluster members");
    connect_direct().await?;

    // Example 3: Monitor cluster events
    info!("\nExample 3: Monitoring cluster events");
    monitor_events().await?;

    // Example 4: Perform administrative operations
    info!("\nExample 4: Administrative operations");
    admin_operations().await?;

    // Example 5: Health monitoring and metrics
    info!("\nExample 5: Health monitoring and metrics");
    health_monitoring().await?;

    Ok(())
}

/// Connect to cluster using P2P discovery
async fn connect_with_discovery() -> Result<(), Box<dyn std::error::Error>> {
    info!("Discovering cluster 'my-cluster'...");
    
    // Connect using automatic discovery
    let client = IrohRaftManagementClient::connect_to_cluster("my-cluster").await?;
    
    // Get cluster status
    let status = client.get_cluster_status().await?;
    info!("Connected to cluster:");
    info!("  Cluster ID: {}", status.cluster_id);
    info!("  Leader: {:?}", status.leader);
    info!("  Term: {}", status.term);
    info!("  Members: {}", status.members.len());
    
    for member in &status.members {
        info!("    - Node {}: {} ({:?}, {:?})", 
            member.node_id, member.address, member.role, member.health);
    }
    
    Ok(())
}

/// Connect directly to known cluster members
async fn connect_direct() -> Result<(), Box<dyn std::error::Error>> {
    info!("Connecting directly to known cluster members...");
    
    // Create node addresses for known members
    // In a real application, these would be actual Iroh NodeAddrs
    let members = vec![
        // (node_id, NodeAddr)
        // Example: (1, create_node_addr("127.0.0.1:8080", node1_id)),
        // (2, create_node_addr("127.0.0.1:8081", node2_id)),
        // (3, create_node_addr("127.0.0.1:8082", node3_id)),
    ];
    
    if !members.is_empty() {
        let client = IrohRaftManagementClient::connect_to_members(members).await?;
        
        // Test connectivity to each node
        for node_id in 1..=3 {
            match client.ping_node(node_id).await {
                Ok(latency) => {
                    info!("  Node {} reachable: {:?} latency", node_id, latency);
                }
                Err(e) => {
                    warn!("  Node {} unreachable: {}", node_id, e);
                }
            }
        }
    } else {
        info!("  (Skipping - no real node addresses configured)");
    }
    
    Ok(())
}

/// Monitor cluster events in real-time
async fn monitor_events() -> Result<(), Box<dyn std::error::Error>> {
    info!("Setting up event monitoring...");
    
    // Create a mock client for demonstration
    // In real usage, this would connect to an actual cluster
    let client = create_mock_client().await?;
    
    // Subscribe to events with a filter
    let filter = EventFilter {
        event_types: vec![
            "leader_elected".to_string(),
            "member_added".to_string(),
            "member_health_changed".to_string(),
        ],
        node_ids: vec![], // Monitor all nodes
        since: Some(SystemTime::now()),
        until: None,
    };
    
    // Note: In a real implementation, you'd use:
    // let mut events = client.subscribe_to_events().await?;
    // events.set_filter(filter);
    
    info!("  Event subscription configured");
    info!("  Monitoring for: leader changes, member additions, health changes");
    
    // Simulate receiving some events
    simulate_events().await;
    
    Ok(())
}

/// Perform administrative operations
async fn admin_operations() -> Result<(), Box<dyn std::error::Error>> {
    info!("Performing administrative operations...");
    
    let client = create_mock_client().await?;
    
    // 1. Transfer leadership
    info!("  Transferring leadership to node 2...");
    match client.transfer_leadership(Some(2)).await {
        Ok(result) => {
            if result.success {
                info!("    Leadership transferred to node {:?} in {:?}", 
                    result.new_leader, result.transfer_duration);
            } else {
                warn!("    Leadership transfer failed: {:?}", result.error);
            }
        }
        Err(e) => error!("    Transfer failed: {}", e),
    }
    
    // 2. Add a new member
    info!("  Adding new member (node 4)...");
    match client.add_member(4, "new-node.example.com:8083".to_string(), MemberRole::Learner).await {
        Ok(result) => {
            if result.success {
                info!("    Member {} added successfully", result.member_id.unwrap_or(0));
            } else {
                warn!("    Failed to add member: {:?}", result.error);
            }
        }
        Err(e) => error!("    Add member failed: {}", e),
    }
    
    // 3. Trigger a snapshot
    info!("  Triggering snapshot...");
    match client.trigger_snapshot().await {
        Ok(()) => info!("    Snapshot triggered successfully"),
        Err(e) => error!("    Snapshot failed: {}", e),
    }
    
    // 4. Create a backup
    info!("  Creating backup...");
    let backup_metadata = BackupMetadata {
        backup_id: format!("backup_{}", chrono::Utc::now().timestamp()),
        description: "Automated backup from management example".to_string(),
        created_at: SystemTime::now(),
        cluster_state: iroh_raft::management::RaftState {
            term: 42,
            vote: Some(1),
            leader: Some(1),
            commit_index: 1000,
            last_applied: 999,
            log_length: 1001,
        },
    };
    
    match client.create_backup(backup_metadata).await {
        Ok(result) => {
            if result.success {
                info!("    Backup created: {} ({} bytes)", 
                    result.backup_id.unwrap_or_default(),
                    result.size_bytes.unwrap_or(0));
            } else {
                warn!("    Backup failed: {:?}", result.error);
            }
        }
        Err(e) => error!("    Backup creation failed: {}", e),
    }
    
    Ok(())
}

/// Monitor cluster health and metrics
async fn health_monitoring() -> Result<(), Box<dyn std::error::Error>> {
    info!("Monitoring cluster health and metrics...");
    
    let client = create_mock_client().await?;
    
    // 1. Check overall health
    info!("  Checking cluster health...");
    match client.get_health_status().await {
        Ok(health) => {
            info!("    Overall health: {:?}", health.overall_health);
            info!("    Cluster health: {:?}", health.cluster_health.status);
            info!("    Healthy members: {}/{}", 
                health.cluster_health.healthy_members,
                health.cluster_health.total_members);
            info!("    Quorum available: {}", health.cluster_health.quorum_available);
            info!("    Leader available: {}", health.cluster_health.leader_available);
            
            for (node_id, status) in &health.node_health {
                info!("    Node {} health: {:?}", node_id, status);
            }
        }
        Err(e) => error!("    Health check failed: {}", e),
    }
    
    // 2. Get metrics
    info!("  Fetching metrics...");
    let categories = vec![
        MetricCategory::Raft,
        MetricCategory::Transport,
        MetricCategory::Performance,
    ];
    
    match client.get_metrics(categories).await {
        Ok(metrics) => {
            info!("    Metrics at {:?}:", metrics.timestamp);
            for (key, value) in metrics.metrics.iter().take(10) {
                info!("      {}: {:.2}", key, value);
            }
            if metrics.metrics.len() > 10 {
                info!("      ... and {} more metrics", metrics.metrics.len() - 10);
            }
        }
        Err(e) => error!("    Metrics fetch failed: {}", e),
    }
    
    // 3. Continuous monitoring example
    info!("  Setting up continuous health monitoring...");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            // In a real application, you'd check health here
            info!("    [Monitor] Periodic health check would run here");
        }
    });
    
    // Let the monitoring run for a bit
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    Ok(())
}

/// Create a mock client for demonstration
async fn create_mock_client() -> Result<IrohRaftManagementClient, Box<dyn std::error::Error>> {
    // In a real application, this would connect to actual nodes
    // For now, we create a client with no connections for demonstration
    let endpoint = Endpoint::builder()
        .bind()
        .await?;
    
    Ok(IrohRaftManagementClient::new(endpoint))
}

/// Simulate receiving cluster events
async fn simulate_events() {
    use iroh_raft::management::{ClusterEvent, HealthStatus};
    
    // Simulate some events
    let events = vec![
        ClusterEvent::leader_elected(2, 43, Some(1)),
        ClusterEvent::member_added(4, MemberRole::Learner, "node4:8083".to_string()),
        ClusterEvent::health_changed(3, HealthStatus::Healthy, HealthStatus::Degraded),
    ];
    
    for event in events {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match event {
            ClusterEvent::LeaderElected { new_leader, term, previous_leader, .. } => {
                info!("  [Event] Leader elected: {} (term {}, previous: {:?})", 
                    new_leader, term, previous_leader);
            }
            ClusterEvent::MemberAdded { node_id, role, address, .. } => {
                info!("  [Event] Member added: {} at {} with role {:?}", 
                    node_id, address, role);
            }
            ClusterEvent::MemberHealthChanged { node_id, old_status, new_status, .. } => {
                info!("  [Event] Health changed for node {}: {:?} -> {:?}", 
                    node_id, old_status, new_status);
            }
            _ => {}
        }
    }
}

// Helper function to create a NodeAddr (would be real in production)
#[allow(dead_code)]
fn create_node_addr(addr_str: &str, node_id: NodeId) -> NodeAddr {
    NodeAddr::new(node_id)
        .with_direct_addresses(
            addr_str.parse::<std::net::SocketAddr>()
                .map(|addr| vec![addr])
                .unwrap_or_default()
        )
}