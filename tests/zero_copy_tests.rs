//! Zero-copy message tests for iroh-raft transport layer
//!
//! This module provides comprehensive tests for zero-copy message handling features
//! including COW behavior, streaming, large message handling, and edge cases.

#![cfg(test)]

use iroh_raft::transport::protocol::{
    ZeroCopyMessage, ZeroCopyRpcRequest, MessageType, MessageHeader,
    serialize_raft_message_fast, deserialize_raft_message_fast,
    serialize_payload_zero_copy, deserialize_zero_copy,
    write_message_zero_copy, read_zero_copy_message,
    raft_utils, LARGE_MESSAGE_THRESHOLD,
};
use bytes::{Bytes, BytesMut};
use raft::prelude::{Message, MessageType as RaftMessageType};
use std::borrow::Cow;
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use proptest::prelude::*;

/// Test zero-copy message creation with borrowed data
#[test]
fn test_zero_copy_message_borrowed() {
    let payload_data = b"test payload data";
    let request_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    
    let message = ZeroCopyMessage::new_borrowed(
        MessageType::Request,
        Some(request_id),
        payload_data,
    );
    
    assert_eq!(message.msg_type, MessageType::Request);
    assert_eq!(message.request_id, Some(request_id));
    assert_eq!(message.payload_bytes(), payload_data);
    assert_eq!(message.payload_len(), payload_data.len());
    assert!(!message.is_large_message());
    
    // Verify it's actually borrowed
    match &message.payload {
        Cow::Borrowed(_) => (),
        Cow::Owned(_) => panic!("Expected borrowed payload"),
    }
}

/// Test zero-copy message creation with owned data
#[test]
fn test_zero_copy_message_owned() {
    let payload_data = Bytes::from("test payload data");
    let message = ZeroCopyMessage::new_owned(
        MessageType::Response,
        None,
        payload_data.clone(),
    );
    
    assert_eq!(message.msg_type, MessageType::Response);
    assert_eq!(message.request_id, None);
    assert_eq!(message.payload_bytes(), payload_data.as_ref());
    assert_eq!(message.payload_len(), payload_data.len());
    
    // Verify it's owned
    match &message.payload {
        Cow::Borrowed(_) => panic!("Expected owned payload"),
        Cow::Owned(_) => (),
    }
}

/// Test COW (Copy-on-Write) behavior
#[test]
fn test_cow_behavior() {
    let original_data = b"original data";
    let message = ZeroCopyMessage::new_borrowed(
        MessageType::RaftMessage,
        None,
        original_data,
    );
    
    // Initially borrowed
    assert!(matches!(message.payload, Cow::Borrowed(_)));
    
    // Convert to owned
    let owned_message = message.into_owned();
    assert!(matches!(owned_message.payload, Cow::Owned(_)));
    assert_eq!(owned_message.payload_bytes(), original_data);
}

/// Test large message detection
#[test]
fn test_large_message_detection() {
    // Small message
    let small_payload = vec![0u8; 1024]; // 1KB
    let small_message = ZeroCopyMessage::new_borrowed(
        MessageType::Request,
        None,
        &small_payload,
    );
    assert!(!small_message.is_large_message());
    
    // Large message
    let large_payload = vec![0u8; LARGE_MESSAGE_THRESHOLD + 1]; // > 64KB
    let large_message = ZeroCopyMessage::new_borrowed(
        MessageType::Request,
        None,
        &large_payload,
    );
    assert!(large_message.is_large_message());
}

/// Test zero-copy RPC request creation
#[test]
fn test_zero_copy_rpc_request() {
    let payload_data = b"rpc payload";
    
    // Test borrowed payload
    let rpc_borrowed = ZeroCopyRpcRequest::new_borrowed(
        "raft",
        "raft.message",
        payload_data,
    );
    
    assert_eq!(rpc_borrowed.service, "raft");
    assert_eq!(rpc_borrowed.method, "raft.message");
    assert_eq!(rpc_borrowed.payload.as_ref(), payload_data);
    
    // Test owned payload
    let rpc_owned = ZeroCopyRpcRequest::new_owned(
        "test",
        "test.method",
        payload_data.to_vec(),
    );
    
    assert_eq!(rpc_owned.service, "test");
    assert_eq!(rpc_owned.method, "test.method");
    assert_eq!(rpc_owned.payload.as_ref(), payload_data);
    
    // Test conversion to standard RPC request
    let standard = rpc_borrowed.to_standard();
    assert_eq!(standard.service, "raft");
    assert_eq!(standard.method, "raft.message");
    assert_eq!(standard.payload, payload_data);
}

/// Test fast Raft message serialization/deserialization
#[tokio::test]
async fn test_fast_raft_message_serialization() -> Result<(), Box<dyn std::error::Error>> {
    let mut raft_msg = Message::default();
    raft_msg.set_msg_type(RaftMessageType::MsgHeartbeat);
    raft_msg.from = 1;
    raft_msg.to = 2;
    raft_msg.term = 42;
    raft_msg.index = 100;
    raft_msg.log_term = 41;
    
    // Test fast serialization
    let serialized = serialize_raft_message_fast(&raft_msg)?;
    assert!(!serialized.is_empty());
    
    // Test fast deserialization
    let deserialized = deserialize_raft_message_fast(&serialized)?;
    
    // Verify fields match
    assert_eq!(deserialized.get_msg_type(), raft_msg.get_msg_type());
    assert_eq!(deserialized.from, raft_msg.from);
    assert_eq!(deserialized.to, raft_msg.to);
    assert_eq!(deserialized.term, raft_msg.term);
    assert_eq!(deserialized.index, raft_msg.index);
    assert_eq!(deserialized.log_term, raft_msg.log_term);
    
    Ok(())
}

/// Test zero-copy serialization with pre-allocated buffers
#[tokio::test]
async fn test_zero_copy_serialization() -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct TestData {
        id: u64,
        name: String,
        values: Vec<i32>,
    }
    
    let test_data = TestData {
        id: 123,
        name: "test".to_string(),
        values: vec![1, 2, 3, 4, 5],
    };
    
    // Test zero-copy serialization into pre-allocated buffer
    let mut buf = BytesMut::with_capacity(1024);
    serialize_payload_zero_copy(&test_data, &mut buf)?;
    
    assert!(!buf.is_empty());
    
    // Test zero-copy deserialization
    let bytes = buf.freeze();
    let deserialized: TestData = deserialize_zero_copy(&bytes)?;
    
    assert_eq!(deserialized, test_data);
    
    Ok(())
}

/// Test streaming writes and reads for large messages
#[tokio::test]
async fn test_streaming_large_messages() -> Result<(), Box<dyn std::error::Error>> {
    // Create a large message (> 64KB)
    let large_payload = vec![0x42u8; LARGE_MESSAGE_THRESHOLD + 1000];
    let message = ZeroCopyMessage::new_borrowed(
        MessageType::RaftMessage,
        Some([1; 16]),
        &large_payload,
    );
    
    assert!(message.is_large_message());
    
    // Test streaming write
    let mut buffer = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buffer);
        write_message_zero_copy(&mut cursor, &message).await?;
    }
    
    assert!(!buffer.is_empty());
    
    // Test streaming read
    let mut read_cursor = Cursor::new(&buffer);
    let read_message = read_zero_copy_message(&mut read_cursor).await?;
    
    assert_eq!(read_message.msg_type, message.msg_type);
    assert_eq!(read_message.request_id, message.request_id);
    assert_eq!(read_message.payload_bytes(), message.payload_bytes());
    
    Ok(())
}

/// Test utility functions for Raft operations
#[tokio::test]
async fn test_raft_utils() -> Result<(), Box<dyn std::error::Error>> {
    // Test heartbeat message creation
    let heartbeat = raft_utils::create_heartbeat_message();
    assert_eq!(heartbeat.msg_type, MessageType::Heartbeat);
    assert!(heartbeat.payload.is_empty());
    assert!(raft_utils::is_heartbeat_message(&heartbeat));
    assert!(!raft_utils::should_stream_message(&heartbeat));
    
    // Test Raft RPC request creation
    let mut raft_msg = Message::default();
    raft_msg.set_msg_type(RaftMessageType::MsgAppend);
    raft_msg.from = 1;
    raft_msg.to = 2;
    raft_msg.term = 5;
    
    let rpc_request = raft_utils::create_raft_rpc_request(&raft_msg, Some([2; 16]))?;
    assert_eq!(rpc_request.msg_type, MessageType::Request);
    assert_eq!(rpc_request.request_id, Some([2; 16]));
    assert!(!rpc_request.payload.is_empty());
    
    Ok(())
}

/// Test edge cases with malformed data
#[tokio::test]
async fn test_malformed_data_handling() -> Result<(), Box<dyn std::error::Error>> {
    // Test deserializing random bytes as Raft message
    let random_bytes = vec![0x1a, 0x2b, 0x3c, 0x4d, 0x5e];
    let result = deserialize_raft_message_fast(&random_bytes);
    assert!(result.is_err());
    
    // Test deserializing empty bytes
    let empty_result = deserialize_raft_message_fast(&[]);
    assert!(empty_result.is_err());
    
    // Test creating message with empty payload
    let empty_message = ZeroCopyMessage::new_borrowed(
        MessageType::Request,
        None,
        &[],
    );
    assert_eq!(empty_message.payload_len(), 0);
    assert!(!empty_message.is_large_message());
    
    Ok(())
}

/// Test message header serialization
#[tokio::test]
async fn test_message_header_handling() -> Result<(), Box<dyn std::error::Error>> {
    let header = MessageHeader {
        msg_type: MessageType::Response,
        request_id: Some([0xFF; 16]),
        payload_len: 12345,
    };
    
    // Test header serialization
    let mut header_buf = BytesMut::new();
    iroh_raft::transport::protocol::serialize_header_zero_copy(&header, &mut header_buf)?;
    
    assert!(!header_buf.is_empty());
    
    // Test deserialization
    let deserialized_header: MessageHeader = 
        iroh_raft::transport::protocol::deserialize_payload(&header_buf)?;
    
    assert_eq!(deserialized_header.msg_type, header.msg_type);
    assert_eq!(deserialized_header.request_id, header.request_id);
    assert_eq!(deserialized_header.payload_len, header.payload_len);
    
    Ok(())
}

/// Property-based test for zero-copy message roundtrip
proptest! {
    #[test]
    fn property_zero_copy_message_roundtrip(
        msg_type_val in 0u8..5,
        has_request_id in any::<bool>(),
        payload_size in 0usize..10000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let msg_type = match msg_type_val {
                0 => MessageType::Request,
                1 => MessageType::Response,
                2 => MessageType::RaftMessage,
                3 => MessageType::Heartbeat,
                _ => MessageType::Error,
            };
            
            let request_id = if has_request_id {
                Some([payload_size as u8; 16])
            } else {
                None
            };
            
            let payload = vec![0x42u8; payload_size];
            
            // Create zero-copy message
            let original = ZeroCopyMessage::new_borrowed(msg_type, request_id, &payload);
            
            // Serialize and deserialize
            let mut buffer = Vec::new();
            {
                let mut cursor = Cursor::new(&mut buffer);
                write_message_zero_copy(&mut cursor, &original).await.unwrap();
            }
            
            let mut read_cursor = Cursor::new(&buffer);
            let roundtrip = read_zero_copy_message(&mut read_cursor).await.unwrap();
            
            // Verify roundtrip
            prop_assert_eq!(roundtrip.msg_type, original.msg_type);
            prop_assert_eq!(roundtrip.request_id, original.request_id);
            prop_assert_eq!(roundtrip.payload_bytes(), original.payload_bytes());
            
            Ok(())
        })?;
    }
}

/// Property-based test for Raft message fast serialization
proptest! {
    #[test]
    fn property_raft_message_fast_serialization(
        from in 1u64..1000,
        to in 1u64..1000,
        term in 1u64..1000,
        msg_type_val in 0u8..10,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let msg_type = match msg_type_val {
                0 => RaftMessageType::MsgHeartbeat,
                1 => RaftMessageType::MsgHeartbeatResponse,
                2 => RaftMessageType::MsgAppend,
                3 => RaftMessageType::MsgAppendResponse,
                4 => RaftMessageType::MsgRequestVote,
                5 => RaftMessageType::MsgRequestVoteResponse,
                6 => RaftMessageType::MsgSnapshot,
                7 => RaftMessageType::MsgUnreachable,
                8 => RaftMessageType::MsgSnapStatus,
                _ => RaftMessageType::MsgCheckQuorum,
            };
            
            let mut raft_msg = Message::default();
            raft_msg.set_msg_type(msg_type);
            raft_msg.from = from;
            raft_msg.to = to;
            raft_msg.term = term;
            
            // Fast serialization roundtrip
            let serialized = serialize_raft_message_fast(&raft_msg).unwrap();
            let deserialized = deserialize_raft_message_fast(&serialized).unwrap();
            
            // Verify core fields
            prop_assert_eq!(deserialized.get_msg_type(), raft_msg.get_msg_type());
            prop_assert_eq!(deserialized.from, raft_msg.from);
            prop_assert_eq!(deserialized.to, raft_msg.to);
            prop_assert_eq!(deserialized.term, raft_msg.term);
            
            Ok(())
        })?;
    }
}

/// Test memory efficiency of zero-copy approach
#[test]
fn test_memory_efficiency() {
    let large_data = vec![0u8; 1_000_000]; // 1MB
    
    // Test borrowed message (should not copy the data)
    let borrowed_msg = ZeroCopyMessage::new_borrowed(
        MessageType::RaftMessage,
        None,
        &large_data,
    );
    
    // Payload should reference the original data
    assert_eq!(borrowed_msg.payload_bytes().len(), large_data.len());
    assert_eq!(borrowed_msg.payload_bytes().as_ptr(), large_data.as_ptr());
    
    // Test that cloning a borrowed message is efficient
    let cloned_msg = borrowed_msg.clone();
    match (&borrowed_msg.payload, &cloned_msg.payload) {
        (Cow::Borrowed(orig), Cow::Borrowed(cloned)) => {
            assert_eq!(orig.as_ptr(), cloned.as_ptr());
        }
        _ => panic!("Expected both to be borrowed"),
    }
}

/// Test payload size calculations
#[test]
fn test_payload_size_calculations() {
    let test_cases = vec![
        (0, false),
        (100, false),
        (LARGE_MESSAGE_THRESHOLD - 1, false),
        (LARGE_MESSAGE_THRESHOLD, false),
        (LARGE_MESSAGE_THRESHOLD + 1, true),
        (LARGE_MESSAGE_THRESHOLD * 2, true),
    ];
    
    for (size, should_be_large) in test_cases {
        let payload = vec![0u8; size];
        let message = ZeroCopyMessage::new_borrowed(
            MessageType::Request,
            None,
            &payload,
        );
        
        assert_eq!(message.payload_len(), size);
        assert_eq!(message.is_large_message(), should_be_large);
        assert!(message.total_size > size as u32); // Should include header overhead
    }
}

/// Test zero-copy message with different payload types
#[test]
fn test_different_payload_types() {
    // Test with string data
    let string_data = "Hello, zero-copy world!";
    let string_msg = ZeroCopyMessage::new_borrowed(
        MessageType::Request,
        None,
        string_data.as_bytes(),
    );
    assert_eq!(
        std::str::from_utf8(string_msg.payload_bytes()).unwrap(),
        string_data
    );
    
    // Test with structured data (serialized)
    let structured_data = serde_json::to_vec(&serde_json::json!({
        "type": "test",
        "value": 42,
        "nested": {
            "array": [1, 2, 3]
        }
    })).unwrap();
    
    let structured_msg = ZeroCopyMessage::new_borrowed(
        MessageType::Response,
        Some([1; 16]),
        &structured_data,
    );
    
    // Should be able to deserialize back
    let parsed: serde_json::Value = serde_json::from_slice(structured_msg.payload_bytes()).unwrap();
    assert_eq!(parsed["type"], "test");
    assert_eq!(parsed["value"], 42);
}

/// Benchmark-style test to compare zero-copy vs traditional approach
#[tokio::test]
async fn test_performance_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let test_data = vec![0x42u8; 10_000]; // 10KB test data
    let iterations = 100;
    
    // Test zero-copy approach
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let msg = ZeroCopyMessage::new_borrowed(
            MessageType::RaftMessage,
            None,
            &test_data,
        );
        // Simulate some processing
        let _len = msg.payload_len();
        let _is_large = msg.is_large_message();
    }
    let zero_copy_duration = start.elapsed();
    
    // Test traditional approach (with cloning)
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let _cloned_data = test_data.clone(); // Simulate copying
        let _len = test_data.len();
        let _is_large = test_data.len() > LARGE_MESSAGE_THRESHOLD;
    }
    let traditional_duration = start.elapsed();
    
    println!(
        "Zero-copy: {:?}, Traditional: {:?}, Ratio: {:.2}x",
        zero_copy_duration,
        traditional_duration,
        traditional_duration.as_nanos() as f64 / zero_copy_duration.as_nanos() as f64
    );
    
    // Zero-copy should be at least as fast (usually much faster for large data)
    assert!(zero_copy_duration <= traditional_duration * 2);
    
    Ok(())
}

/// Test error cases in streaming
#[tokio::test]
async fn test_streaming_error_cases() -> Result<(), Box<dyn std::error::Error>> {
    // Test reading from truncated stream
    let partial_data = vec![0x01, 0x02, 0x03]; // Too short to be a valid message
    let mut cursor = Cursor::new(partial_data);
    
    let result = read_zero_copy_message(&mut cursor).await;
    assert!(result.is_err());
    
    // Test with oversized header
    let mut oversized_header = vec![0xFF; 4]; // Header length = MAX_U32
    oversized_header.extend_from_slice(&vec![0x00; 1000]);
    let mut cursor = Cursor::new(oversized_header);
    
    let result = read_zero_copy_message(&mut cursor).await;
    assert!(result.is_err());
    
    Ok(())
}