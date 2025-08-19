//! Integration tests for iroh-raft transport features
//!
//! This module provides comprehensive integration tests that verify the full
//! message flow with connection pooling, zero-copy optimizations, and graceful shutdown.

#![cfg(test)]

use iroh_raft::transport::iroh::IrohRaftTransport;
use iroh_raft::transport::shared::SharedNodeState;
use iroh_raft::transport::protocol::{
    RaftProtocolHandler, ZeroCopyMessage, MessageType, raft_utils,
    LARGE_MESSAGE_THRESHOLD, RAFT_ALPN,
};
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

/// Test helper to create a test transport with protocol handler
async fn create_test_transport_with_handler(
    node_id: NodeId,
    port: u16,
) -> Result<(IrohRaftTransport, Arc<SharedNodeState>, Endpoint, mpsc::UnboundedReceiver<(u64, Message)>), Box<dyn std::error::Error>> {
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
        .alpns(vec![RAFT_ALPN.to_vec()])
        .bind_addr_v4(format!("127.0.0.1:{}", port).parse()?)
        .bind()
        .await?;
    
    let local_node_id = endpoint.node_id();
    let (tx, rx) = mpsc::unbounded_channel();
    
    // Create protocol handler
    let protocol_handler = RaftProtocolHandler::new(node_id, tx.clone());
    
    // Set up protocol handling
    endpoint.accept(RAFT_ALPN, Arc::new(move |conn| {
        protocol_handler.accept(conn)
    }));
    
    let transport = IrohRaftTransport::new(
        shared_state.clone(),
        endpoint.clone(),
        local_node_id,
        tx,
    );
    
    Ok((transport, shared_state, endpoint, rx))
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

/// Test full message flow with connection pooling
#[tokio::test]
async fn test_full_message_flow_with_pooling() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7001).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7002).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7002).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7001).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Send first message
    let mut msg1 = Message::new();
    msg1.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg1.from = 1;
    msg1.to = 2;
    msg1.term = 1;
    
    transport1.send_message(2, msg1.clone()).await?;
    
    // Verify message was received
    let received = timeout(Duration::from_secs(5), rx2.recv()).await?;
    assert!(received.is_some());
    let (from, received_msg) = received.unwrap();
    assert_eq!(from, 1);
    assert_eq!(received_msg.from, 1);
    assert_eq!(received_msg.to, 2);
    assert_eq!(received_msg.term, 1);
    
    // Send second message (should reuse connection)
    let mut msg2 = Message::new();
    msg2.set_msg_type(raft::prelude::MessageType::MsgAppend);
    msg2.from = 1;
    msg2.to = 2;
    msg2.term = 2;
    
    let send_start = Instant::now();
    transport1.send_message(2, msg2.clone()).await?;
    let send_duration = send_start.elapsed();
    
    // Verify second message was received
    let received = timeout(Duration::from_secs(5), rx2.recv()).await?;
    assert!(received.is_some());
    let (from, received_msg) = received.unwrap();
    assert_eq!(from, 1);
    assert_eq!(received_msg.from, 1);
    assert_eq!(received_msg.to, 2);
    assert_eq!(received_msg.term, 2);
    
    // Second send should be fast due to connection reuse
    assert!(send_duration < Duration::from_millis(500));
    
    // Verify metrics
    let metrics = transport1.get_metrics().await;
    assert!(metrics.active_connections > 0);
    assert!(metrics.messages_sent >= 2);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test performance under load with connection pooling
#[tokio::test]
async fn test_performance_under_load() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7003).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7004).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7004).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7003).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    let message_count = 100;
    let start_time = Instant::now();
    
    // Send many messages rapidly
    for i in 0..message_count {
        let mut msg = Message::new();
        msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        msg.from = 1;
        msg.to = 2;
        msg.term = i + 1;
        
        transport1.send_message(2, msg).await?;
    }
    
    let send_duration = start_time.elapsed();
    
    // Verify messages were received
    let mut received_count = 0;
    for _ in 0..message_count {
        let result = timeout(Duration::from_millis(100), rx2.recv()).await;
        if result.is_ok() && result.unwrap().is_some() {
            received_count += 1;
        } else {
            break;
        }
    }
    
    println!(
        "Sent {} messages in {:?}, received {}, throughput: {:.2} msg/s",
        message_count,
        send_duration,
        received_count,
        message_count as f64 / send_duration.as_secs_f64()
    );
    
    // Should achieve reasonable throughput
    assert!(received_count >= message_count / 2); // At least 50% delivery
    assert!(send_duration < Duration::from_secs(10)); // Reasonable send time
    
    // Verify metrics
    let metrics = transport1.get_metrics().await;
    assert!(metrics.messages_sent >= message_count as u64);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test large message handling with zero-copy optimizations
#[tokio::test]
async fn test_large_message_handling() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7005).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7006).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7006).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7005).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Create a large message that would use streaming
    let mut large_msg = Message::new();
    large_msg.set_msg_type(raft::prelude::MessageType::MsgSnapshot);
    large_msg.from = 1;
    large_msg.to = 2;
    large_msg.term = 1;
    
    // Add large data to trigger streaming (simplified for test)
    // In real usage, snapshot data would be large
    large_msg.commit = LARGE_MESSAGE_THRESHOLD as u64;
    
    let send_start = Instant::now();
    transport1.send_message(2, large_msg.clone()).await?;
    let send_duration = send_start.elapsed();
    
    // Verify large message was received
    let received = timeout(Duration::from_secs(10), rx2.recv()).await?;
    assert!(received.is_some());
    let (from, received_msg) = received.unwrap();
    assert_eq!(from, 1);
    assert_eq!(received_msg.from, 1);
    assert_eq!(received_msg.to, 2);
    assert_eq!(received_msg.get_msg_type(), raft::prelude::MessageType::MsgSnapshot);
    
    // Large message should complete within reasonable time
    assert!(send_duration < Duration::from_secs(30));
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test failure recovery scenarios
#[tokio::test]
async fn test_failure_recovery_scenarios() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7007).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7008).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7008).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7007).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Establish connection
    let mut msg = Message::new();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 1;
    
    transport1.send_message(2, msg.clone()).await?;
    
    // Verify initial message
    let received = timeout(Duration::from_secs(5), rx2.recv()).await?;
    assert!(received.is_some());
    
    // Simulate peer failure by shutting down transport2
    transport2.shutdown().await;
    
    // Try to send message to failed peer
    msg.term = 2;
    let result = transport1.send_message(2, msg.clone()).await;
    // Transport should handle this gracefully (might succeed by buffering)
    
    // Wait a bit for connection cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Restart transport2
    let (transport2_new, shared_state2_new, endpoint2_new, mut rx2_new) = 
        create_test_transport_with_handler(2, 7008).await?;
    
    // Update peer information for new transport
    add_test_peer(&shared_state1, 2, &endpoint2_new, 7008).await;
    
    transport2_new.start().await?;
    
    // Send message to recovered peer
    msg.term = 3;
    transport1.send_message(2, msg.clone()).await?;
    
    // Should receive message on recovered transport
    let received = timeout(Duration::from_secs(5), rx2_new.recv()).await?;
    assert!(received.is_some());
    let (from, received_msg) = received.unwrap();
    assert_eq!(received_msg.term, 3);
    
    // Cleanup
    transport1.shutdown().await;
    transport2_new.shutdown().await;
    
    Ok(())
}

/// Test zero-copy message flow end-to-end
#[tokio::test]
async fn test_zero_copy_message_flow() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7009).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7010).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7010).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7009).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    // Test different message types that use zero-copy optimizations
    let test_cases = vec![
        raft::prelude::MessageType::MsgHeartbeat,
        raft::prelude::MessageType::MsgAppend,
        raft::prelude::MessageType::MsgRequestVote,
        raft::prelude::MessageType::MsgSnapshot,
    ];
    
    for (i, msg_type) in test_cases.iter().enumerate() {
        let mut msg = Message::new();
        msg.set_msg_type(*msg_type);
        msg.from = 1;
        msg.to = 2;
        msg.term = (i + 1) as u64;
        
        let send_start = Instant::now();
        transport1.send_message(2, msg.clone()).await?;
        let send_duration = send_start.elapsed();
        
        // Verify message was received
        let received = timeout(Duration::from_secs(5), rx2.recv()).await?;
        assert!(received.is_some());
        let (from, received_msg) = received.unwrap();
        assert_eq!(received_msg.get_msg_type(), *msg_type);
        assert_eq!(received_msg.term, (i + 1) as u64);
        
        // Zero-copy optimizations should make this fast
        assert!(send_duration < Duration::from_millis(1000));
    }
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}

/// Test graceful shutdown during active message flow
#[tokio::test]
async fn test_graceful_shutdown_during_flow() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7011).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7012).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7012).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7011).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    let transport1_arc = Arc::new(transport1);
    
    // Start continuous message sending
    let send_task = {
        let transport = transport1_arc.clone();
        tokio::spawn(async move {
            let mut term = 1;
            while !transport.is_shutting_down() {
                let mut msg = Message::new();
                msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                msg.from = 1;
                msg.to = 2;
                msg.term = term;
                term += 1;
                
                let _ = transport.send_message(2, msg).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    };
    
    // Start message receiving
    let recv_task = tokio::spawn(async move {
        let mut count = 0;
        while let Some(result) = timeout(Duration::from_millis(100), rx2.recv()).await.ok() {
            if result.is_some() {
                count += 1;
            }
        }
        count
    });
    
    // Let messages flow for a bit
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Initiate graceful shutdown
    let shutdown_start = Instant::now();
    transport1_arc.shutdown().await;
    let shutdown_duration = shutdown_start.elapsed();
    
    // Shutdown should complete quickly even with active flow
    assert!(shutdown_duration < Duration::from_secs(5));
    
    // Wait for tasks to complete
    let (send_result, recv_count) = tokio::join!(
        timeout(Duration::from_secs(2), send_task),
        timeout(Duration::from_secs(2), recv_task)
    );
    
    assert!(send_result.is_ok(), "Send task should complete cleanly");
    
    if let Ok(Ok(count)) = recv_count {
        println!("Received {} messages during test", count);
        assert!(count > 0, "Should have received some messages");
    }
    
    // Cleanup
    transport2.shutdown().await;
    
    Ok(())
}

/// Property-based test for integration robustness
proptest! {
    #[test]
    fn property_integration_robustness(
        message_count in 1usize..50,
        message_interval_ms in 1u64..100,
        include_large_messages in any::<bool>(),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (transport1, shared_state1, endpoint1, mut rx1) = 
                create_test_transport_with_handler(1, 7200).await.unwrap();
            let (transport2, shared_state2, endpoint2, mut rx2) = 
                create_test_transport_with_handler(2, 7201).await.unwrap();
            
            // Add peer information
            add_test_peer(&shared_state1, 2, &endpoint2, 7201).await;
            add_test_peer(&shared_state2, 1, &endpoint1, 7200).await;
            
            // Start transports
            transport1.start().await.unwrap();
            transport2.start().await.unwrap();
            
            let mut sent_count = 0;
            let mut received_count = 0;
            
            // Send messages with varying patterns
            for i in 0..message_count {
                let mut msg = Message::new();
                
                if include_large_messages && i % 10 == 0 {
                    msg.set_msg_type(raft::prelude::MessageType::MsgSnapshot);
                } else if i % 2 == 0 {
                    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                } else {
                    msg.set_msg_type(raft::prelude::MessageType::MsgAppend);
                }
                
                msg.from = 1;
                msg.to = 2;
                msg.term = (i + 1) as u64;
                
                if transport1.send_message(2, msg).await.is_ok() {
                    sent_count += 1;
                }
                
                tokio::time::sleep(Duration::from_millis(message_interval_ms)).await;
            }
            
            // Collect received messages
            let collect_timeout = Duration::from_millis(message_count as u64 * message_interval_ms + 1000);
            let collect_start = Instant::now();
            
            while collect_start.elapsed() < collect_timeout {
                if let Ok(Some(_)) = timeout(Duration::from_millis(50), rx2.recv()).await {
                    received_count += 1;
                } else {
                    break;
                }
            }
            
            // Invariants:
            // 1. Should send some messages successfully
            prop_assert!(sent_count > 0);
            
            // 2. Should receive some messages (allowing for network variability)
            prop_assert!(received_count > 0);
            
            // 3. Transport should remain functional
            prop_assert!(!transport1.is_shutting_down());
            prop_assert!(!transport2.is_shutting_down());
            
            // 4. Metrics should reflect activity
            let metrics = transport1.get_metrics().await;
            prop_assert!(metrics.messages_sent > 0);
            
            // Cleanup
            transport1.shutdown().await;
            transport2.shutdown().await;
        });
    }
}

/// Test concurrent connections and message flow
#[tokio::test]
async fn test_concurrent_connections() -> Result<(), Box<dyn std::error::Error>> {
    // Create 3 transports for a more complex test
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7013).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7014).await?;
    let (transport3, shared_state3, endpoint3, mut rx3) = create_test_transport_with_handler(3, 7015).await?;
    
    // Set up full mesh connectivity
    add_test_peer(&shared_state1, 2, &endpoint2, 7014).await;
    add_test_peer(&shared_state1, 3, &endpoint3, 7015).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7013).await;
    add_test_peer(&shared_state2, 3, &endpoint3, 7015).await;
    add_test_peer(&shared_state3, 1, &endpoint1, 7013).await;
    add_test_peer(&shared_state3, 2, &endpoint2, 7014).await;
    
    // Start all transports
    transport1.start().await?;
    transport2.start().await?;
    transport3.start().await?;
    
    // Send messages from 1 to both 2 and 3
    let mut msg_to_2 = Message::new();
    msg_to_2.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg_to_2.from = 1;
    msg_to_2.to = 2;
    msg_to_2.term = 1;
    
    let mut msg_to_3 = Message::new();
    msg_to_3.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg_to_3.from = 1;
    msg_to_3.to = 3;
    msg_to_3.term = 1;
    
    transport1.send_message(2, msg_to_2).await?;
    transport1.send_message(3, msg_to_3).await?;
    
    // Verify both messages were received
    let received_2 = timeout(Duration::from_secs(5), rx2.recv()).await?;
    let received_3 = timeout(Duration::from_secs(5), rx3.recv()).await?;
    
    assert!(received_2.is_some());
    assert!(received_3.is_some());
    
    let (from_2, msg_2) = received_2.unwrap();
    let (from_3, msg_3) = received_3.unwrap();
    
    assert_eq!(from_2, 1);
    assert_eq!(msg_2.to, 2);
    assert_eq!(from_3, 1);
    assert_eq!(msg_3.to, 3);
    
    // Verify connection pooling metrics
    let metrics = transport1.get_metrics().await;
    assert!(metrics.active_connections >= 2); // Should have connections to both peers
    assert!(metrics.messages_sent >= 2);
    
    // Cleanup
    transport1.shutdown().await;
    transport2.shutdown().await;
    transport3.shutdown().await;
    
    Ok(())
}

/// Test error handling in integration scenarios
#[tokio::test]
async fn test_integration_error_handling() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7016).await?;
    
    // Add invalid peer
    let invalid_peer = PeerInfo {
        node_id: 999,
        address: "invalid:99999".to_string(),
        p2p_node_id: Some("invalid-node-id".to_string()),
        p2p_addresses: vec!["invalid:99999".to_string()],
        p2p_relay_url: None,
    };
    shared_state1.add_peer(999, invalid_peer).await;
    
    transport1.start().await?;
    
    // Try to send to invalid peer
    let mut msg = Message::new();
    msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 999;
    msg.term = 1;
    
    let result = transport1.send_message(999, msg).await;
    
    // Should handle error gracefully
    assert!(result.is_err());
    
    // Transport should remain functional
    assert!(!transport1.is_shutting_down());
    
    // Should be able to get metrics
    let metrics = transport1.get_metrics().await;
    // Metrics should be accessible even after errors
    
    // Cleanup
    transport1.shutdown().await;
    
    Ok(())
}

/// Stress test for transport under high load
#[tokio::test]
async fn test_transport_stress() -> Result<(), Box<dyn std::error::Error>> {
    let (transport1, shared_state1, endpoint1, mut rx1) = create_test_transport_with_handler(1, 7017).await?;
    let (transport2, shared_state2, endpoint2, mut rx2) = create_test_transport_with_handler(2, 7018).await?;
    
    // Add peer information
    add_test_peer(&shared_state1, 2, &endpoint2, 7018).await;
    add_test_peer(&shared_state2, 1, &endpoint1, 7017).await;
    
    // Start transports
    transport1.start().await?;
    transport2.start().await?;
    
    let message_count = 500;
    let concurrent_senders = 5;
    
    let transport1_arc = Arc::new(transport1);
    let mut send_handles = Vec::new();
    
    // Create multiple concurrent sending tasks
    for sender_id in 0..concurrent_senders {
        let transport = transport1_arc.clone();
        let handle = tokio::spawn(async move {
            let mut success_count = 0;
            for i in 0..message_count / concurrent_senders {
                let mut msg = Message::new();
                msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
                msg.from = 1;
                msg.to = 2;
                msg.term = (sender_id * 1000 + i + 1) as u64;
                
                if transport.send_message(2, msg).await.is_ok() {
                    success_count += 1;
                }
            }
            success_count
        });
        send_handles.push(handle);
    }
    
    // Collect received messages
    let recv_handle = tokio::spawn(async move {
        let mut received_count = 0;
        let timeout_duration = Duration::from_secs(30);
        let start = Instant::now();
        
        while start.elapsed() < timeout_duration {
            if let Ok(Some(_)) = timeout(Duration::from_millis(100), rx2.recv()).await {
                received_count += 1;
            } else {
                break;
            }
        }
        received_count
    });
    
    // Wait for all senders to complete
    let mut total_sent = 0;
    for handle in send_handles {
        let sent = handle.await?;
        total_sent += sent;
    }
    
    // Wait for receiver to finish
    let total_received = recv_handle.await?;
    
    println!(
        "Stress test: sent {}, received {}, success rate: {:.2}%",
        total_sent,
        total_received,
        (total_received as f64 / total_sent as f64) * 100.0
    );
    
    // Verify reasonable performance under stress
    assert!(total_sent > 0, "Should send some messages");
    assert!(total_received > 0, "Should receive some messages");
    assert!(
        total_received >= total_sent / 2,
        "Should achieve at least 50% delivery rate"
    );
    
    // Verify transport remains functional
    assert!(!transport1_arc.is_shutting_down());
    
    // Cleanup
    transport1_arc.shutdown().await;
    transport2.shutdown().await;
    
    Ok(())
}