//! Administrative operations example
//! 
//! This example demonstrates the administrative operations available
//! for managing Raft clusters, including health monitoring, maintenance,
//! and cluster management tasks.

use iroh_raft::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("🔧 Starting administrative operations example");

    // Create cluster configuration for administration
    let config = ConfigBuilder::development_preset()
        .node_id(1)
        .data_dir("./data/admin_node")
        .bind_address("127.0.0.1:8080")
        .metrics(true)
        .build()?;

    // Create cluster
    let mut cluster: RaftCluster<KeyValueStore> = RaftCluster::new(config).await?;
    println!("✅ Cluster created for administrative operations");

    // Initialize administrative tools
    let health_checker = HealthChecker::new();
    let maintenance_ops = MaintenanceOps::new();
    let cluster_monitor = ClusterMonitor::new();

    println!("\n🏥 Health Monitoring Operations");
    println!("================================");

    // Comprehensive health check
    let health = cluster.health_check().await;
    println!("Overall health: {:?}", health.overall);
    println!("Component health:");
    for (component, status) in &health.components {
        println!("  - {}: {:?}", component, status);
    }
    println!("Last check: {:?}", health.last_check);

    // Check health aggregation
    let component_health = vec![
        ("consensus".to_string(), Health::Healthy),
        ("storage".to_string(), Health::Healthy),
        ("transport".to_string(), Health::Degraded),
    ];
    let aggregated = health_checker.aggregate_health(&component_health);
    println!("Aggregated health: {:?}", aggregated);

    println!("\n📊 Cluster Information");
    println!("======================");

    // Get detailed cluster information
    let info = cluster.cluster_info().await;
    println!("Cluster configuration:");
    println!("  - Node count: {}", info.config.node_count);
    println!("  - Replication factor: {}", info.config.replication_factor);
    println!("  - Election timeout: {:?}", info.config.election_timeout_ms);

    println!("Membership information:");
    println!("  - Total members: {}", info.membership.members.len());
    println!("  - Leader: {:?}", info.membership.leader.as_ref().map(|l| l.node_id));
    println!("  - Joining: {}", info.membership.joining.len());
    println!("  - Leaving: {}", info.membership.leaving.len());

    println!("Performance statistics:");
    println!("  - Uptime: {} seconds", info.statistics.uptime_seconds);
    println!("  - Total operations: {}", info.statistics.total_operations);
    println!("  - Operations/sec: {:.2}", info.statistics.ops_per_second);
    println!("  - Average latency: {:.2}ms", info.statistics.avg_latency_ms);
    println!("  - Error rate: {:.2}%", info.statistics.error_rate_percent);

    println!("\n🔧 Maintenance Operations");
    println!("=========================");

    // Check if maintenance is needed
    let current_entries = 5000;
    let current_size = 50 * 1024 * 1024; // 50MB
    let entries_since_snapshot = 25000;

    let maintenance_needed = cluster_monitor.needs_maintenance(
        current_entries,
        current_size,
        entries_since_snapshot,
    );

    println!("Maintenance assessment:");
    println!("  - Current entries: {}", current_entries);
    println!("  - Current size: {} bytes", current_size);
    println!("  - Entries since snapshot: {}", entries_since_snapshot);
    println!("  - Compaction needed: {}", maintenance_needed.compact_log);
    println!("  - Snapshot needed: {}", maintenance_needed.take_snapshot);

    if maintenance_needed.any_needed() {
        println!("⚠️  Maintenance operations are recommended");
    } else {
        println!("✅ No maintenance needed at this time");
    }

    // Demonstrate maintenance thresholds
    println!("\nMaintenance thresholds:");
    println!("  - Auto compact: {}", maintenance_ops.auto_compact);
    println!("  - Compact threshold (entries): {}", maintenance_ops.compact_threshold_entries);
    println!("  - Compact threshold (bytes): {}", maintenance_ops.compact_threshold_bytes);
    println!("  - Auto snapshot: {}", maintenance_ops.auto_snapshot);
    println!("  - Snapshot threshold: {}", maintenance_ops.snapshot_threshold_entries);

    println!("\n📸 Snapshot Operations");
    println!("======================");

    // Try to create a snapshot
    match tokio::time::timeout(Duration::from_millis(100), cluster.take_snapshot()).await {
        Ok(Ok(snapshot_id)) => {
            println!("✅ Snapshot created successfully: {}", snapshot_id);
        }
        Ok(Err(e)) => {
            println!("❌ Snapshot creation failed: {}", e);
            println!("   (This is expected in the current development state)");
        }
        Err(_) => {
            println!("⏰ Snapshot creation timed out");
            println!("   (This is expected in the current development state)");
        }
    }

    println!("\n🗜️  Log Compaction");
    println!("==================");

    // Try to compact logs
    let retain_index = 1000;
    match tokio::time::timeout(Duration::from_millis(100), cluster.compact_log(retain_index)).await {
        Ok(Ok(())) => {
            println!("✅ Log compaction completed, retained up to index {}", retain_index);
        }
        Ok(Err(e)) => {
            println!("❌ Log compaction failed: {}", e);
            println!("   (This is expected in the current development state)");
        }
        Err(_) => {
            println!("⏰ Log compaction timed out");
            println!("   (This is expected in the current development state)");
        }
    }

    println!("\n👥 Member Management");
    println!("====================");

    // Current cluster status
    let status = cluster.status().await;
    println!("Current cluster status:");
    println!("  - Node ID: {}", status.node_id);
    println!("  - Role: {:?}", status.role);
    println!("  - Term: {}", status.term);
    println!("  - Members: {:?}", status.members);

    // Try to add a member
    let new_member = MemberConfig {
        node_id: 2,
        address: "127.0.0.1:8081".to_string(),
        metadata: Some([
            ("role".to_string(), "secondary".to_string()),
            ("zone".to_string(), "us-west-2b".to_string()),
        ].into_iter().collect()),
    };

    println!("Attempting to add member: {:?}", new_member.node_id);
    match tokio::time::timeout(Duration::from_millis(100), cluster.add_member(new_member)).await {
        Ok(Ok(())) => {
            println!("✅ Member added successfully");
        }
        Ok(Err(e)) => {
            println!("❌ Add member failed: {}", e);
            println!("   (This is expected in the current development state)");
        }
        Err(_) => {
            println!("⏰ Add member timed out");
            println!("   (This is expected in the current development state)");
        }
    }

    // Try leadership transfer
    let target_node = 2;
    println!("Attempting to transfer leadership to node: {}", target_node);
    match tokio::time::timeout(Duration::from_millis(100), cluster.transfer_leadership(target_node)).await {
        Ok(Ok(())) => {
            println!("✅ Leadership transfer completed");
        }
        Ok(Err(e)) => {
            println!("❌ Leadership transfer failed: {}", e);
            println!("   (This is expected in the current development state)");
        }
        Err(_) => {
            println!("⏰ Leadership transfer timed out");
            println!("   (This is expected in the current development state)");
        }
    }

    println!("\n📈 Custom Monitoring");
    println!("====================");

    // Custom health checker with different settings
    let custom_checker = HealthChecker::with_timeouts(
        Duration::from_secs(10),
        Duration::from_secs(60),
    );

    println!("Custom health checker:");
    println!("  - Check timeout: {:?}", custom_checker.check_timeout);
    println!("  - Check interval: {:?}", custom_checker.check_interval);

    // Custom maintenance operations
    let custom_maintenance = MaintenanceOps::with_thresholds(
        5000,  // Compact after 5K entries
        10 * 1024 * 1024,  // Compact after 10MB
        10000, // Snapshot after 10K entries
    );

    println!("Custom maintenance thresholds:");
    println!("  - Compact entries: {}", custom_maintenance.compact_threshold_entries);
    println!("  - Compact bytes: {}", custom_maintenance.compact_threshold_bytes);
    println!("  - Snapshot entries: {}", custom_maintenance.snapshot_threshold_entries);

    // Test maintenance decisions
    let test_cases = vec![
        (2000, 5 * 1024 * 1024, 5000),   // No maintenance needed
        (6000, 5 * 1024 * 1024, 5000),   // Compaction needed
        (2000, 15 * 1024 * 1024, 5000),  // Compaction needed (size)
        (2000, 5 * 1024 * 1024, 15000),  // Snapshot needed
        (6000, 15 * 1024 * 1024, 15000), // Both needed
    ];

    println!("\nMaintenance decision matrix:");
    for (i, (entries, size, since_snapshot)) in test_cases.iter().enumerate() {
        let compact = custom_maintenance.should_compact(*entries, *size);
        let snapshot = custom_maintenance.should_snapshot(*since_snapshot);
        println!("  Case {}: entries={}, size={}MB, since_snapshot={} -> compact={}, snapshot={}",
                 i + 1, entries, size / (1024 * 1024), since_snapshot, compact, snapshot);
    }

    println!("\n🎯 Final Status Check");
    println!("=====================");

    // Final comprehensive status
    let final_status = cluster.status().await;
    let final_health = cluster.health_check().await;

    println!("Final cluster state:");
    println!("  - Status: {:?}", final_status.role);
    println!("  - Health: {:?}", final_health.overall);
    println!("  - Metrics: {} proposals, {} queries", 
             final_status.metrics.total_proposals,
             final_status.metrics.total_queries);

    // Graceful shutdown
    println!("\n🛑 Shutting down...");
    cluster.shutdown().await?;
    println!("✅ Administrative operations example completed!");

    Ok(())
}
