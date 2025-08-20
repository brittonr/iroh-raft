//! Performance-optimized example demonstrating the async/performance fixes
//!
//! This example shows how to use the new lock-free patterns, backpressure control,
//! circuit breakers, and timeout management for optimal performance.

use iroh_raft::{
    actor_patterns::NodeStatusActor,
    backpressure::{bounded_with_backpressure, BackpressureConfig, Priority},
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitBreakerManager},
    timeout_manager::{TimeoutManager, OperationType},
    lock_free::{LockFreeStatsAggregator, LockFreeConnectionPool},
    Result,
};
use std::time::Duration;
use tokio::time::sleep;

/// Performance-optimized Raft node example
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize simple tracing subscriber
    // tracing_subscriber::init();
    
    println!("🚀 Starting performance-optimized Raft node example");
    
    // 1. Demonstrate lock-free actor patterns
    demonstrate_actor_patterns().await?;
    
    // 2. Demonstrate backpressure control
    demonstrate_backpressure().await?;
    
    // 3. Demonstrate circuit breaker protection
    demonstrate_circuit_breakers().await?;
    
    // 4. Demonstrate timeout management
    demonstrate_timeout_management().await?;
    
    // 5. Demonstrate lock-free data structures
    demonstrate_lock_free_structures().await?;
    
    // 6. Demonstrate integrated performance monitoring
    demonstrate_performance_monitoring().await?;
    
    println!("✅ Performance optimization demonstration completed");
    Ok(())
}

/// Demonstrate actor patterns replacing Arc<RwLock<T>>
async fn demonstrate_actor_patterns() -> Result<()> {
    println!("\n📡 Demonstrating Actor Patterns");
    
    // Create node status actor to replace Arc<RwLock<NodeStatus>>
    use iroh_raft::types::{NodeStatus, NodeRole};
    
    let initial_status = NodeStatus {
        node_id: 1,
        role: NodeRole::Follower,
        term: 0,
        leader_id: None,
        last_log_index: 0,
        committed_index: 0,
        applied_index: 0,
        connected_peers: vec![],
        uptime_seconds: 0,
    };
    
    let (actor, handle) = NodeStatusActor::new(initial_status);
    
    // Spawn actor in background with a timeout to prevent hanging
    let actor_handle = tokio::spawn(async move {
        actor.run().await;
    });
    
    // Demonstrate lock-free status updates
    println!("  📝 Updating node role to Leader");
    handle.update_role(NodeRole::Leader, 1, Some(1)).await?;
    
    println!("  📊 Updating log indexes");
    handle.update_indexes(100, 95, 90).await?;
    
    // Get status without locks
    let status = handle.get_status().await?;
    println!("  📈 Current status: role={:?}, term={}, leader_id={:?}", 
             status.role, status.term, status.leader_id);
    
    // Abort the actor to prevent hanging
    actor_handle.abort();
    
    println!("  ✅ Actor patterns: No locks, no contention!");
    Ok(())
}

/// Demonstrate backpressure and flow control
async fn demonstrate_backpressure() -> Result<()> {
    println!("\n🌊 Demonstrating Backpressure Control");
    
    let config = BackpressureConfig {
        max_pending: 5,  // Small for demo
        send_timeout: Duration::from_millis(100),
        enable_priority: true,
        ..Default::default()
    };
    
    let (sender, mut receiver) = bounded_with_backpressure(config);
    
    // Fill up the channel to capacity
    println!("  📤 Sending messages with different priorities");
    for i in 0..3 {
        sender.send(format!("low-priority-{}", i), Priority::Low).await?;
    }
    
    // Try to send high priority (should work)
    sender.send("urgent-message".to_string(), Priority::Critical).await?;
    
    // This should apply backpressure and timeout
    println!("  ⏰ Testing backpressure timeout");
    let result = sender.send("overflow-message".to_string(), Priority::Normal).await;
    match result {
        Err(_) => println!("  ✅ Backpressure correctly applied - message rejected"),
        Ok(_) => println!("  ⚠️  Unexpected: message should have been rejected"),
    }
    
    // Consume messages to show priority ordering
    println!("  📥 Receiving messages (should prioritize urgent first)");
    while let Some(msg) = receiver.recv().await {
        println!("    📋 Received: {} (priority: {:?}, age: {:?})", 
                 msg.data, msg.priority, msg.age());
        // Break after receiving all messages
        if msg.data == "overflow-message" {
            break;
        }
    }
    
    // Show metrics
    let metrics = sender.metrics();
    println!("  📊 Backpressure metrics: sent={}, timeouts={}, pending={}", 
             metrics.total_sent, metrics.total_timeouts, metrics.current_pending);
    
    Ok(())
}

/// Demonstrate circuit breaker protection
async fn demonstrate_circuit_breakers() -> Result<()> {
    println!("\n⚡ Demonstrating Circuit Breaker Protection");
    
    let config = CircuitBreakerConfig {
        failure_threshold: 2,
        timeout: Duration::from_millis(500),
        success_threshold: 1,
        ..Default::default()
    };
    
    let breaker = CircuitBreaker::new(config);
    
    // Simulate failing operation
    println!("  💥 Simulating failing operations");
    for i in 0..3 {
        let result = breaker.call(async {
            Err::<(), _>(format!("Simulated failure {}", i))
        }).await;
        
        println!("    🔄 Attempt {}: {:?}", i + 1, result.is_err());
    }
    
    println!("  🛑 Circuit breaker should now be OPEN");
    println!("  🔍 State: {:?}", breaker.state().await);
    
    // Try operation while circuit is open
    let result = breaker.call(async {
        Ok::<&str, String>("This should be rejected")
    }).await;
    
    match result {
        Err(_) => println!("  ✅ Circuit breaker correctly rejected request"),
        Ok(_) => println!("  ⚠️  Unexpected: request should have been rejected"),
    }
    
    // Wait for timeout and test half-open
    println!("  ⏱️  Waiting for circuit breaker timeout");
    sleep(Duration::from_millis(600)).await;
    
    // This should succeed and close the circuit
    let result = breaker.call(async {
        Ok::<&str, String>("Recovery successful")
    }).await;
    
    println!("  🔄 Recovery attempt: {:?}", result.is_ok());
    println!("  🔍 Final state: {:?}", breaker.state().await);
    
    let metrics = breaker.metrics();
    println!("  📊 Circuit breaker metrics: failures={}, opened={}, closed={}", 
             metrics.total_failures, metrics.circuit_opened_count, metrics.circuit_closed_count);
    
    Ok(())
}

/// Demonstrate timeout management and adaptive timeouts
async fn demonstrate_timeout_management() -> Result<()> {
    println!("\n⏱️  Demonstrating Timeout Management");
    
    let mut manager = TimeoutManager::new();
    
    // Test successful operation
    println!("  ✅ Testing successful operation");
    let result = manager.execute_with_timeout(
        OperationType::RaftMessage,
        async {
            sleep(Duration::from_millis(50)).await;
            Ok::<_, &str>("Fast operation")
        }
    ).await;
    println!("    📈 Result: {:?}", result.is_ok());
    
    // Test timeout
    println!("  ⏰ Testing timeout behavior");
    let result = manager.execute_with_timeout(
        OperationType::RaftMessage,
        async {
            sleep(Duration::from_millis(1000)).await;
            Ok::<_, &str>("Slow operation")
        }
    ).await;
    println!("    📉 Result: {:?} (should timeout)", result.is_err());
    
    // Test retry with exponential backoff
    println!("  🔄 Testing retry with exponential backoff");
    let mut attempt_count = 0;
    let result = manager.execute_with_retry(
        OperationType::Network,
        || {
            attempt_count += 1;
            async move {
                if attempt_count < 3 {
                    Err("Temporary failure")
                } else {
                    Ok("Success after retries")
                }
            }
        }
    ).await;
    
    println!("    📊 Retry result: {:?} after {} attempts", result.is_ok(), attempt_count);
    
    // Show timeout statistics
    if let Some(stats) = manager.get_stats(OperationType::RaftMessage) {
        println!("  📊 Timeout stats: operations={}, timeouts={}, avg_latency={:?}", 
                 stats.total_operations, stats.timeout_count, stats.avg_duration);
    }
    
    Ok(())
}

/// Demonstrate lock-free data structures
async fn demonstrate_lock_free_structures() -> Result<()> {
    println!("\n🔓 Demonstrating Lock-Free Data Structures");
    
    // Lock-free connection pool
    println!("  🏊 Testing lock-free connection pool");
    let pool = LockFreeConnectionPool::new(Duration::from_secs(60));
    
    pool.insert(1, "connection-1".to_string());
    pool.insert(2, "connection-2".to_string());
    
    if let Some(conn_ref) = pool.get(&1) {
        println!("    📡 Retrieved connection: {}", conn_ref.0);
    }
    
    let metrics = pool.metrics();
    println!("    📊 Pool metrics: active={}, created={}", 
             metrics.active_connections, metrics.total_created);
    
    // Lock-free statistics aggregator
    println!("  📈 Testing lock-free statistics");
    let stats = LockFreeStatsAggregator::new();
    
    // Simulate concurrent metric updates
    for i in 0..10 {
        stats.increment_counter("raft_messages");
        stats.record_latency("message_latency", Duration::from_millis(i * 10));
        stats.set_gauge("active_connections", i);
    }
    
    println!("    📊 Message count: {:?}", stats.get_counter("raft_messages"));
    if let Some(histogram) = stats.get_latency_histogram("message_latency") {
        println!("    📊 Latency p95: {:?}μs", histogram.p95());
    }
    println!("    📊 Active connections: {:?}", stats.get_gauge("active_connections"));
    
    Ok(())
}

/// Demonstrate integrated performance monitoring
async fn demonstrate_performance_monitoring() -> Result<()> {
    println!("\n📊 Demonstrating Integrated Performance Monitoring");
    
    // Create circuit breaker manager for multiple services
    let config = CircuitBreakerConfig::default();
    let cb_manager = CircuitBreakerManager::new(config);
    
    // Set up different services with circuit breakers
    let services = ["peer-1", "peer-2", "storage", "transport"];
    
    println!("  🔧 Setting up circuit breakers for {} services", services.len());
    for service in &services {
        let breaker = cb_manager.get_breaker(service).await;
        
        // Simulate some operations
        for i in 0..3 {
            let result = breaker.call(async {
                if i == 2 && service == &"peer-2" {
                    // Fail the last operation for peer-2
                    Err("Simulated failure")
                } else {
                    sleep(Duration::from_millis(10)).await;
                    Ok("Success")
                }
            }).await;
            
            if result.is_err() {
                println!("    ❌ {} operation {} failed", service, i + 1);
            }
        }
    }
    
    // Get overall system state
    let all_states = cb_manager.get_all_states().await;
    let all_metrics = cb_manager.get_all_metrics().await;
    
    println!("  🌐 System-wide circuit breaker status:");
    for (service, state) in all_states {
        let metrics = all_metrics.get(&service).unwrap();
        println!("    📡 {}: {:?} (failures: {}, requests: {})", 
                 service, state, metrics.total_failures, metrics.total_requests);
    }
    
    // Demonstrate timeout manager statistics
    let timeout_manager = TimeoutManager::new();
    let all_timeout_stats = timeout_manager.get_all_stats();
    
    if !all_timeout_stats.is_empty() {
        println!("  ⏱️  Timeout statistics:");
        for (op_type, stats) in all_timeout_stats {
            println!("    📊 {:?}: ops={}, timeouts={}, avg_latency={:?}", 
                     op_type, stats.total_operations, stats.timeout_count, stats.avg_duration);
        }
    } else {
        println!("  ⏱️  No timeout statistics yet (use the timeout manager in operations)");
    }
    
    println!("  ✅ Performance monitoring: Complete system observability!");
    Ok(())
}

/// Example of how to integrate all optimizations in a real Raft node
#[allow(dead_code)]
async fn example_optimized_raft_node() -> Result<()> {
    println!("\n🔗 Example: Fully Optimized Raft Node Integration");
    
    // This is pseudo-code showing how all components would work together
    // in a real iroh-raft node implementation
    
    println!("  1. 🏗️  Create lock-free actors for shared state");
    // NodeStatusActor replaces Arc<RwLock<NodeStatus>>
    // ConnectionPoolActor replaces Arc<RwLock<HashMap<u64, Connection>>>
    
    println!("  2. 🌊 Set up backpressure channels");
    // Replace mpsc::unbounded_channel with bounded_with_backpressure
    // Configure priority queues for different message types
    
    println!("  3. ⚡ Initialize circuit breakers");
    // One per peer for network operations
    // One for storage operations
    // One for proposal processing
    
    println!("  4. ⏱️  Configure timeout management");
    // Different timeout configs per operation type
    // Adaptive timeout adjustment based on network conditions
    
    println!("  5. 🔓 Use lock-free data structures");
    // Lock-free metrics collection
    // Lock-free connection pools
    // Lock-free message queues
    
    println!("  6. 📊 Enable comprehensive monitoring");
    // Real-time performance metrics
    // Circuit breaker status
    // Backpressure statistics
    // Timeout performance
    
    println!("  ✅ Result: High-performance, resilient Raft node!");
    
    Ok(())
}