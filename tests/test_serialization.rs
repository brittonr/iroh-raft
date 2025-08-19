//! Tests for Raft message and snapshot serialization

use iroh_raft::storage::codec::{serialize_message, deserialize_message, serialize_snapshot, deserialize_snapshot};
use raft::prelude::{Message, MessageType, Snapshot, SnapshotMetadata};

#[test]
fn test_message_serialization_roundtrip() {
    // Create a test message
    let mut msg = Message::default();
    msg.set_msg_type(MessageType::MsgHeartbeat);
    msg.from = 1;
    msg.to = 2;
    msg.term = 5;
    msg.log_term = 3;
    msg.index = 100;
    msg.commit = 50;
    
    // Serialize
    let encoded = serialize_message(&msg).expect("Failed to serialize message");
    assert!(!encoded.is_empty(), "Encoded message should not be empty");
    
    // Deserialize
    let decoded = deserialize_message(&encoded).expect("Failed to deserialize message");
    
    // Verify fields match
    assert_eq!(decoded.get_msg_type(), MessageType::MsgHeartbeat);
    assert_eq!(decoded.from, 1);
    assert_eq!(decoded.to, 2);
    assert_eq!(decoded.term, 5);
    assert_eq!(decoded.log_term, 3);
    assert_eq!(decoded.index, 100);
    assert_eq!(decoded.commit, 50);
}

#[test]
fn test_snapshot_serialization_roundtrip() {
    // Create a test snapshot
    let mut snapshot = Snapshot::default();
    let mut metadata = SnapshotMetadata::default();
    metadata.index = 1000;
    metadata.term = 10;
    metadata.set_conf_state(raft::prelude::ConfState {
        voters: vec![1, 2, 3],
        learners: vec![4, 5],
        ..Default::default()
    });
    snapshot.set_metadata(metadata);
    snapshot.data = vec![1, 2, 3, 4, 5];
    
    // Serialize
    let encoded = serialize_snapshot(&snapshot).expect("Failed to serialize snapshot");
    assert!(!encoded.is_empty(), "Encoded snapshot should not be empty");
    
    // Deserialize
    let decoded = deserialize_snapshot(&encoded).expect("Failed to deserialize snapshot");
    
    // Verify fields match
    assert_eq!(decoded.get_metadata().index, 1000);
    assert_eq!(decoded.get_metadata().term, 10);
    assert_eq!(decoded.get_metadata().get_conf_state().voters, vec![1, 2, 3]);
    assert_eq!(decoded.get_metadata().get_conf_state().learners, vec![4, 5]);
    assert_eq!(decoded.data, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_empty_message_serialization() {
    let msg = Message::default();
    
    // Should still serialize successfully
    let encoded = serialize_message(&msg).expect("Failed to serialize empty message");
    let decoded = deserialize_message(&encoded).expect("Failed to deserialize empty message");
    
    // Empty message should have default values
    assert_eq!(decoded.get_msg_type(), MessageType::MsgHup);
    assert_eq!(decoded.from, 0);
    assert_eq!(decoded.to, 0);
}

#[test]
fn test_invalid_data_deserialization() {
    let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
    
    // Should return an error for invalid data
    assert!(deserialize_message(&invalid_data).is_err());
    assert!(deserialize_snapshot(&invalid_data).is_err());
}