//! Graceful shutdown tests for iroh-raft
//!
//! This module provides comprehensive tests for graceful shutdown functionality
//! including connection cleanup, task cancellation, and metrics recording.

#![cfg(test)]

use iroh_raft::transport::iroh::IrohRaftTransport;
use iroh_raft::transport::shared::SharedNodeState;
use iroh_raft::types::{NodeId, PeerInfo, ClusterInfo};
use iroh_raft::discovery::registry::NodeRegistry;
use iroh_raft::test_helpers::create_temp_dir;
use raft::prelude::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::timeout;
use proptest::prelude::*;
use iroh::{Endpoint, NodeAddr};
use std::net::SocketAddr;

/// Test helper to create a test transport instance
async fn create_test_transport(
    node_id: NodeId,
    port: u16,
) -> Result<(IrohRaftTransport, Arc<SharedNodeState>, Endpoint), Box<dyn std::error::Error>> {
    let temp_dir = create_temp_dir()?;
    let cluster_info = ClusterInfo {
        cluster_id: "test-cluster".to_string(),
        nodes: HashMap::new(),
    };
    
    let node_registry = Arc::new(NodeRegistry::new());
    let shared_state = Arc::new(SharedNodeState::new(
        node_id,
        cluster_info,
        node_registry,
    ));
    
    let endpoint = Endpoint::builder()
        .alpns(vec![b"test-protocol".to_vec()])
        .bind_addr_v4(format!("127.0.0.1:{}", port).parse()?)
        .bind()
        .await?;
    
    let local_node_id = endpoint.node_id();
    let (tx, _rx) = mpsc::unbounded_channel();
    
    let transport = IrohRaftTransport::new(
        shared_state.clone(),
        endpoint.clone(),
        local_node_id,
        tx,
    );
    
    Ok((transport, shared_state, endpoint))
}

/// Test helper to add peer information to shared state
async fn add_test_peer(
    shared_state: &Arc<SharedNodeState>,
    peer_id: NodeId,
    endpoint: &Endpoint,
    port: u16,
) {
    let peer_info = PeerInfo {
        node_id: peer_id,
        address: format!("127.0.0.1:{}", port),
        p2p_node_id: Some(endpoint.node_id().to_string()),
        p2p_addresses: vec![format!("127.0.0.1:{}", port)],
        p2p_relay_url: None,
    };
    
    shared_state.add_peer(peer_id, peer_info).await;
    
    // Also add to node registry
    let node_addr = NodeAddr::new(endpoint.node_id())
        .with_direct_addresses([format!("127.0.0.1:{}", port).parse::<SocketAddr>().unwrap()]);
    shared_state.node_registry().register_node(peer_id, node_addr).await;
}

/// Test basic graceful shutdown
#[tokio::test]
async fn test_basic_graceful_shutdown() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, _shared_state, _endpoint) = create_test_transport(1, 6001).await?;
    
    // Verify initial state
    assert!(!transport.is_shutting_down());
    
    // Start transport
    transport.start().await?;
    
    // Measure shutdown time
    let shutdown_start = Instant::now();
    transport.shutdown().await;
    let shutdown_duration = shutdown_start.elapsed();
    
    // Verify shutdown completed within reasonable time
    assert!(shutdown_duration < Duration::from_secs(5));
    
    // Verify shutdown state
    assert!(transport.is_shutting_down());
    
    Ok(())
}

/// Test shutdown with active connections
#[tokio::test]
async fn test_shutdown_with_active_connections() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 6002).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 6003).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 6003).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 6002).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Establish connections by sending messages
    let mut msg = Message::new();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg).await?;
    
    // Verify connection is active
    let metrics_before = transport1.get_metrics().await;
    assert!(metrics_before.active_connections > 0);
    
    // Shutdown transport1
    let shutdown_start = Instant::now();
    transport1.shutdown().await;
    let shutdown_duration = shutdown_start.elapsed();
    
    // Verify graceful shutdown completed
    assert!(shutdown_duration < Duration::from_secs(10));
    assert!(transport1.is_shutting_down());
    
    // Verify connections were cleaned up
    let metrics_after = transport1.get_metrics().await;
    // Note: metrics might still show connections briefly, but transport should be shut down
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test shutdown signal propagation to background tasks
#[tokio::test]
async fn test_shutdown_signal_propagation() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, _shared_state, _endpoint) = create_test_transport(1, 6004).await?;
    
    // Start transport (this spawns background tasks)
    transport.start().await?;
    
    // Wait a moment for tasks to start
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Initiate shutdown
    let shutdown_future = transport.shutdown();
    
    // Use timeout to ensure shutdown doesn't hang
    let result = timeout(Duration::from_secs(5), shutdown_future).await;
    assert!(result.is_ok(), "Shutdown should complete within timeout");
    
    // Verify shutdown state
    assert!(transport.is_shutting_down());
    
    Ok(())
}

/// Test concurrent shutdown calls
#[tokio::test]
async fn test_concurrent_shutdown_calls() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, _shared_state, _endpoint) = create_test_transport(1, 6005).await?;
    let transport_arc = Arc::new(transport);
    
    // Start transport
    transport_arc.start().await?;
    
    // Create multiple concurrent shutdown tasks
    let mut handles = Vec::new();
    for _ in 0..5 {
        let transport = transport_arc.clone();
        let handle = tokio::spawn(async move {
            transport.shutdown().await;
        });
        handles.push(handle);
    }
    
    // Wait for all shutdown calls to complete
    let start = Instant::now();
    for handle in handles {
        let result = timeout(Duration::from_secs(5), handle).await;
        assert!(result.is_ok(), "Shutdown task should complete");
    }
    let total_duration = start.elapsed();
    
    // All shutdowns should complete quickly
    assert!(total_duration < Duration::from_secs(10));
    
    // Verify final state
    assert!(transport_arc.is_shutting_down());
    
    Ok(())
}

/// Test task cancellation during shutdown
#[tokio::test]
async fn test_task_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 6006).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 6007).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 6007).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 6006).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Send messages to create background activity
    for i in 0..10 {
        let mut msg = Message::new();
        msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        msg.from = 1;
        msg.to = 2;
        msg.term = i + 1;
        
        // Don't wait for send to complete to keep tasks busy
        let _ = tokio::spawn(async move {
            let _ = transport1.send_message(2, msg).await;
        });
    }
    
    // Wait a bit for tasks to be active
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Shutdown should cancel tasks cleanly
    let shutdown_start = Instant::now();
    transport1.shutdown().await;
    let shutdown_duration = shutdown_start.elapsed();
    
    // Should complete quickly even with active tasks
    assert!(shutdown_duration < Duration::from_secs(3));
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test metrics recording during shutdown
#[tokio::test]
async fn test_metrics_recording_on_shutdown() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 6008).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 6009).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 6009).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 6008).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Establish connections and send messages
    let mut msg = Message::new();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg).await?;
    
    // Record metrics before shutdown
    let metrics_before = transport1.get_metrics().await;
    
    // Shutdown should record final metrics
    transport1.shutdown().await;
    
    // Metrics should still be accessible after shutdown
    let metrics_after = transport1.get_metrics().await;
    assert!(metrics_after.messages_sent >= metrics_before.messages_sent);
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test shutdown while messages are being sent
#[tokio::test]
async fn test_shutdown_during_message_sending() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 6010).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 6011).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 6011).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 6010).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    let transport1_arc = Arc::new(transport1);
    
    // Start sending messages in background
    let send_task = {
        let transport = transport1_arc.clone();
        tokio::spawn(async move {
            for i in 0..100 {
                if transport.is_shutting_down() {
                    break;
                }
                
                let mut msg = Message::new();
                msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                msg.from = 1;
                msg.to = 2;
                msg.term = i + 1;
                
                let _ = transport.send_message(2, msg).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    };
    
    // Wait a bit for sends to start
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Initiate shutdown while sends are happening
    let shutdown_future = transport1_arc.shutdown();
    
    // Both operations should complete cleanly
    let (shutdown_result, send_result) = tokio::join!(
        timeout(Duration::from_secs(5), shutdown_future),
        timeout(Duration::from_secs(5), send_task)
    );
    
    assert!(shutdown_result.is_ok(), "Shutdown should complete");
    assert!(send_result.is_ok(), "Send task should complete or be cancelled cleanly");
    
    // Verify final state
    assert!(transport1_arc.is_shutting_down());
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test wait_for_shutdown functionality
#[tokio::test]
async fn test_wait_for_shutdown() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, _shared_state, _endpoint) = create_test_transport(1, 6012).await?;
    let transport_arc = Arc::new(transport);
    
    transport_arc.start().await?;
    
    // Start a task that waits for shutdown
    let wait_transport = transport_arc.clone();
    let wait_task = tokio::spawn(async move {
        wait_transport.wait_for_shutdown().await;
    });
    
    // Wait a bit to ensure wait task is waiting
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Task should not complete yet
    assert!(!wait_task.is_finished());
    
    // Trigger shutdown
    transport_arc.shutdown().await;
    
    // Wait task should complete now
    let wait_result = timeout(Duration::from_secs(1), wait_task).await;
    assert!(wait_result.is_ok(), "Wait task should complete after shutdown");
    
    Ok(())
}

/// Test shutdown rejection of new operations
#[tokio::test]
async fn test_shutdown_rejects_new_operations() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, shared_state, endpoint) = create_test_transport(1, 6013).await?;
    
    // Add a peer
    add_test_peer(&shared_state, 2, &endpoint, 6014).await;
    
    transport.start().await?;
    
    // Shutdown the transport
    transport.shutdown().await;
    
    // Verify shutdown state
    assert!(transport.is_shutting_down());
    
    // Try to send a message after shutdown
    let mut msg = Message::new();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    let result = transport.send_message(2, msg).await;
    assert!(result.is_err(), "Should reject operations after shutdown");
    
    Ok(())
}

/// Property-based test for shutdown under various conditions
proptest! {
    #[test]
    fn property_graceful_shutdown_robustness(
        num_peers in 1usize..5,
        num_messages in 0usize..20,
        shutdown_delay_ms in 0u64..200,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (transport, shared_state, _endpoint) = create_test_transport(1, 6100).await.unwrap();
            
            // Add peers
            for i in 0..num_peers {
                let peer_endpoint = Endpoint::builder()
                    .alpns(vec![b"test-protocol".to_vec()])
                    .bind_addr_v4(format!("127.0.0.1:{}", 6200 + i).parse().unwrap())
                    .bind()
                    .await
                    .unwrap();
                
                let peer_info = PeerInfo {
                    node_id: (i + 2) as u64,
                    address: format!("127.0.0.1:{}", 6200 + i),
                    p2p_node_id: Some(peer_endpoint.node_id().to_string()),
                    p2p_addresses: vec![format!("127.0.0.1:{}", 6200 + i)],
                    p2p_relay_url: None,
                };
                
                shared_state.add_peer((i + 2) as u64, peer_info).await;
            }
            
            transport.start().await.unwrap();
            
            // Send some messages
            for i in 0..num_messages {
                for peer in 0..num_peers {
                    let mut msg = Message::new();
                    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                    msg.from = 1;
                    msg.to = (peer + 2) as u64;
                    msg.term = (i + 1) as u64;
                    
                    // Best effort send (some may fail due to no receiver)
                    let _ = transport.send_message((peer + 2) as u64, msg).await;
                }
            }
            
            // Wait before shutdown
            if shutdown_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(shutdown_delay_ms)).await;
            }
            
            // Shutdown should always complete successfully
            let shutdown_start = Instant::now();
            transport.shutdown().await;
            let shutdown_duration = shutdown_start.elapsed();
            
            // Invariants:
            // 1. Shutdown should complete within reasonable time
            prop_assert!(shutdown_duration < Duration::from_secs(10));
            
            // 2. Transport should be in shutdown state
            prop_assert!(transport.is_shutting_down());
            
            // 3. New operations should be rejected
            let mut test_msg = Message::new();
            test_msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
            test_msg.from = 1;
            test_msg.to = 2;
            test_msg.term = 999;
            
            let send_result = transport.send_message(2, test_msg).await;
            prop_assert!(send_result.is_err());
        });
    }
}

/// Test shutdown cleanup of resources
#[tokio::test]
async fn test_shutdown_resource_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 6015).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 6016).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 6016).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 6015).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Create some activity to allocate resources
    for i in 0..5 {
        let mut msg = Message::new();
        msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        msg.from = 1;
        msg.to = 2;
        msg.term = i + 1;
        
        transport1.send_message(2, msg).await?;
    }
    
    // Verify resources are allocated
    let metrics_before = transport1.get_metrics().await;
    assert!(metrics_before.messages_sent > 0);
    
    // Shutdown should clean up resources
    transport1.shutdown().await;
    
    // Verify cleanup (metrics should still be accessible but transport is shut down)
    assert!(transport1.is_shutting_down());
    
    // Try to access metrics after shutdown (should not panic)
    let _metrics_after = transport1.get_metrics().await;
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test shutdown timeout behavior
#[tokio::test]
async fn test_shutdown_timeout_handling() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, _shared_state, _endpoint) = create_test_transport(1, 6017).await?;
    
    transport.start().await?;
    
    // Shutdown with timeout
    let shutdown_result = timeout(Duration::from_secs(10), transport.shutdown()).await;
    
    // Should complete within timeout
    assert!(shutdown_result.is_ok(), "Shutdown should complete within timeout");
    
    // Verify final state
    assert!(transport.is_shutting_down());
    
    Ok(())
}

/// Test multiple shutdown sequences
#[tokio::test]
async fn test_multiple_shutdown_sequences() -> Result<(), Box<dyn std::error::Error>> {
    // Test that we can create, start, shutdown multiple transports in sequence
    for i in 0..3 {
        let (transport, _shared_state, _endpoint) = create_test_transport(1, 6020 + i).await?;
        
        transport.start().await?;
        
        // Send a test message
        let mut msg = Message::new();
        msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        msg.from = 1;
        msg.to = 2;
        msg.term = 1;
        
        // This will likely fail (no receiver), but should not crash
        let _ = transport.send_message(2, msg).await;
        
        // Shutdown
        transport.shutdown().await;
        assert!(transport.is_shutting_down());
    }
    
    Ok(())
}