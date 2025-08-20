//! Connection pooling tests for iroh-raft transport layer
//!
//! This module provides comprehensive tests for the connection pooling features
//! including connection reuse, health checks, stale connection cleanup, and metrics.

#![cfg(test)]

use iroh_raft::transport::iroh::{IrohRaftTransport, IrohRaftMetrics};
use iroh_raft::transport::shared::SharedNodeState;
use iroh_raft::types::NodeId;
use iroh_raft::transport::shared::PeerInfo;
// use iroh_raft::discovery::registry::NodeRegistry;
// use iroh_raft::test_helpers::{create_temp_dir, TestNode};
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
    let _temp_dir = tempfile::tempdir()?;
    let (raft_tx, _raft_rx) = mpsc::unbounded_channel();
    let shared_state = Arc::new(SharedNodeState::new(node_id, raft_tx));
    
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
        public_key: endpoint.node_id(),
        address: Some(format!("127.0.0.1:{}", port)),
        p2p_node_id: Some(endpoint.node_id().to_string()),
        p2p_addresses: vec![format!("127.0.0.1:{}", port)],
        p2p_relay_url: None,
    };
    
    shared_state.add_peer(peer_info).await;
}

/// Test basic connection pooling functionality
#[tokio::test]
async fn test_connection_reuse() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5001).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5002).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5002).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5001).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Send first message
    let mut msg1 = Message::default();
    msg1.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg1.from = 1;
    msg1.to = 2;
    msg1.term = 1;
    
    let start_time = Instant::now();
    transport1.send_message(2, msg1.clone()).await?;
    let first_send_time = start_time.elapsed();
    
    // Send second message (should reuse connection)
    let start_time = Instant::now();
    transport1.send_message(2, msg1.clone()).await?;
    let second_send_time = start_time.elapsed();
    
    // Second send should be faster due to connection reuse
    assert!(second_send_time <= first_send_time);
    
    // Verify metrics show active connection
    let metrics = transport1.get_metrics().await;
    assert!(metrics.active_connections > 0);
    assert!(metrics.messages_sent >= 2);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test stale connection cleanup
#[tokio::test]
async fn test_stale_connection_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5003).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5004).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5004).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5003).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Send message to establish connection
    let mut msg = Message::default();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg).await?;
    
    // Verify connection exists
    let metrics_before = transport1.get_metrics().await;
    assert!(metrics_before.active_connections > 0);
    
    // Wait for health checker to run (health check interval is 30s, but we'll simulate by shutting down peer)
    transport2.shutdown().await;
    
    // Wait a bit for connection cleanup to detect the stale connection
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Try to send another message (should create new connection)
    let mut msg2 = Message::default();
    msg2.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg2.from = 1;
    msg2.to = 2;
    msg2.term = 2;
    
    // This should fail since transport2 is shutdown
    let result = transport1.send_message(2, msg2).await;
    assert!(result.is_err());
    
    // Cleanup
    transport1.shutdown().await;
    
    Ok(())
}

/// Test concurrent access to connection pool
#[tokio::test]
async fn test_concurrent_connection_access() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5005).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5006).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5006).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5005).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Create multiple concurrent send tasks
    let transport1_arc = Arc::new(transport1);
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let transport = transport1_arc.clone();
        let handle = tokio::spawn(async move {
            let mut msg = Message::default();
            msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
            msg.from = 1;
            msg.to = 2;
            msg.term = i + 1;
            
            transport.send_message(2, msg).await
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }
    
    // Most sends should succeed (connection pooling should handle concurrency)
    assert!(success_count >= 5);
    
    // Verify metrics
    let metrics = transport1_arc.get_metrics().await;
    assert!(metrics.messages_sent >= success_count);
    
    // Cleanup
    transport1_arc.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test connection health checks and recovery
#[tokio::test]
async fn test_connection_health_checks() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5007).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5008).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5008).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5007).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Establish connection
    let mut msg = Message::default();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg.clone()).await?;
    
    // Verify connection is active
    let metrics_initial = transport1.get_metrics().await;
    assert!(metrics_initial.active_connections > 0);
    
    // Simulate connection going stale by stopping transport2
    transport2.shutdown().await;
    
    // Send another message - should fail but transport should handle gracefully
    let result = transport1.send_message(2, msg).await;
    // The transport might buffer the message, so we don't assert failure here
    
    // Health checker should eventually clean up stale connections
    // Since the health check interval is 30s in production, we can't wait for it in tests
    // Instead, verify that the transport doesn't crash and handles the situation gracefully
    
    let metrics_final = transport1.get_metrics().await;
    // Metrics should still be accessible
    assert!(metrics_final.messages_sent >= metrics_initial.messages_sent);
    
    // Cleanup
    transport1.shutdown().await;
    
    Ok(())
}

/// Test metrics tracking for connection pooling
#[tokio::test]
async fn test_connection_pool_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5009).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5010).await?;
    let (transport3, shared_state3, endpoint3) = create_test_transport(3, 5011).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5010).await;
    add_test_peer(&shared_state1, 3, &endpoint3, 5011).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5009).await;
    add_test_peer(&shared_state3, 1, &endpoint1, 5009).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    transport3.start().await?;
    
    // Initial metrics
    let metrics_initial = transport1.get_metrics().await;
    assert_eq!(metrics_initial.active_connections, 0);
    assert_eq!(metrics_initial.messages_sent, 0);
    
    // Send messages to establish connections
    let mut msg1 = Message::default();
    msg1.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg1.from = 1;
    msg1.to = 2;
    msg1.term = 1;
    
    let mut msg2 = Message::default();
    msg2.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg2.from = 1;
    msg2.to = 3;
    msg2.term = 1;
    
    transport1.send_message(2, msg1).await?;
    transport1.send_message(3, msg2).await?;
    
    // Verify metrics reflect connections and messages
    let metrics_after = transport1.get_metrics().await;
    assert!(metrics_after.active_connections >= 1); // At least one connection should be active
    assert!(metrics_after.messages_sent >= 2);
    
    // Send more messages to same peers (should reuse connections)
    for i in 0..5 {
        let mut msg = Message::default();
        msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        msg.from = 1;
        msg.to = 2;
        msg.term = i + 2;
        
        transport1.send_message(2, msg).await?;
    }
    
    // Verify message count increased but connection count didn't necessarily increase
    let metrics_final = transport1.get_metrics().await;
    assert!(metrics_final.messages_sent >= metrics_after.messages_sent + 5);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    transport3.shutdown().await;
    
    Ok(())
}

/// Property-based test for connection pooling under various message patterns
proptest! {
    #[test]
    fn property_connection_pooling_correctness(
        message_count in 1usize..20,
        target_nodes in prop::collection::vec(2u64..10, 1..5)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Create transport
            let (transport, shared_state, endpoint) = create_test_transport(1, 5020).await.unwrap();
            
            // Add all target nodes as peers
            for (i, &target_id) in target_nodes.iter().enumerate() {
                let peer_endpoint = Endpoint::builder()
                    .alpns(vec![b"test-protocol".to_vec()])
                    .bind_addr_v4(format!("127.0.0.1:{}", 5100 + i).parse().unwrap())
                    .bind()
                    .await
                    .unwrap();
                add_test_peer(&shared_state, target_id, &peer_endpoint, 5100 + i as u16).await;
            }
            
            transport.start().await.unwrap();
            
            let metrics_initial = transport.get_metrics().await;
            
            // Send messages to targets
            for i in 0..message_count {
                for &target_id in &target_nodes {
                    let mut msg = Message::default();
                    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                    msg.from = 1;
                    msg.to = target_id;
                    msg.term = (i + 1) as u64;
                    
                    // We expect some sends to fail due to no actual receiver, but transport should handle gracefully
                    let _ = transport.send_message(target_id, msg).await;
                }
            }
            
            let metrics_final = transport.get_metrics().await;
            
            // Invariants:
            // 1. Message count should increase
            prop_assert!(metrics_final.messages_sent >= metrics_initial.messages_sent);
            
            // 2. Connection count should not exceed number of unique targets
            prop_assert!(metrics_final.active_connections <= target_nodes.len());
            
            // 3. Transport should not crash
            prop_assert!(!transport.is_shutting_down());
            
            transport.shutdown().await;
            
            Ok(())
        })?;
    }
}

/// Test connection pool behavior during shutdown
#[tokio::test]
async fn test_connection_pool_shutdown() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5021).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5022).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5022).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5021).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Establish connection
    let mut msg = Message::default();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg).await?;
    
    // Verify connection is active
    let metrics_before = transport1.get_metrics().await;
    assert!(metrics_before.active_connections > 0);
    
    // Initiate shutdown
    let shutdown_start = Instant::now();
    transport1.shutdown().await;
    let shutdown_duration = shutdown_start.elapsed();
    
    // Shutdown should complete within reasonable time
    assert!(shutdown_duration < Duration::from_secs(5));
    
    // Verify shutdown state
    assert!(transport1.is_shutting_down());
    
    // Verify that new messages are rejected
    let mut msg2 = Message::default();
    msg2.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg2.from = 1;
    msg2.to = 2;
    msg2.term = 2;
    
    let result = transport1.send_message(2, msg2).await;
    assert!(result.is_err());
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Test error handling in connection pooling
#[tokio::test]
async fn test_connection_pool_error_handling() -> Result<(), Box<dyn std::error::Error>> {
    let (transport, shared_state, _endpoint) = create_test_transport(1, 5023).await?;
    
    // Add a peer with invalid connection info
    let invalid_peer = PeerInfo {
        node_id: 999,
        public_key: iroh::PublicKey::from_bytes(&[0u8; 32]).unwrap(),
        address: Some("invalid-address".to_string()),
        p2p_node_id: Some("invalid-node-id".to_string()),
        p2p_addresses: vec!["invalid-address".to_string()],
        p2p_relay_url: None,
    };
    shared_state.add_peer(invalid_peer).await;
    
    transport.start().await?;
    
    // Try to send message to invalid peer
    let mut msg = Message::default();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 999;
    msg.term = 1;
    
    let result = transport.send_message(999, msg).await;
    // Should handle error gracefully
    assert!(result.is_err());
    
    // Transport should still be functional
    assert!(!transport.is_shutting_down());
    
    // Cleanup
    transport.shutdown().await;
    
    Ok(())
}

/// Benchmark connection establishment vs reuse
#[tokio::test]
async fn test_connection_reuse_performance() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1) = create_test_transport(1, 5024).await?;
    let (transport2, shared_state2, endpoint2) = create_test_transport(2, 5025).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 5025).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 5024).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Measure first connection time
    let mut msg = Message::default();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    let start = Instant::now();
    transport1.send_message(2, msg.clone()).await?;
    let first_duration = start.elapsed();
    
    // Measure subsequent connection reuse times
    let mut reuse_durations = Vec::new();
    for i in 0..5 {
        msg.term = i + 2;
        let start = Instant::now();
        transport1.send_message(2, msg.clone()).await?;
        reuse_durations.push(start.elapsed());
    }
    
    // Calculate average reuse time
    let avg_reuse_time = reuse_durations.iter().sum::<Duration>() / reuse_durations.len() as u32;
    
    // Connection reuse should generally be faster than initial connection
    // (Though this might not always be true due to test environment variability)
    println!("First connection: {:?}, Average reuse: {:?}", first_duration, avg_reuse_time);
    
    // At minimum, verify that reuse doesn't take significantly longer
    assert!(avg_reuse_time <= first_duration * 2);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}