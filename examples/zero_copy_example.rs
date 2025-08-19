//! Example demonstrating zero-copy message handling optimizations
//! 
//! This example shows how to use the new zero-copy message structures
//! for high-performance Raft message processing.

use bytes::Bytes;
use iroh_raft::transport::protocol::{
    ZeroCopyMessage, MessageType, raft_utils,
    serialize_raft_message_fast, deserialize_raft_message_fast,
};
use raft::prelude::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Zero-Copy Message Handling Example");
    println!("==================================");

    // Create a sample Raft message
    let mut raft_msg = Message::new();
    raft_msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
    raft_msg.from = 1;
    raft_msg.to = 2;
    raft_msg.term = 1;

    println!("\n1. Original Raft message:");
    println!("   Type: {:?}", raft_msg.get_msg_type());
    println!("   From: {}", raft_msg.from);
    println!("   To: {}", raft_msg.to);
    println!("   Term: {}", raft_msg.term);

    // Demonstrate fast serialization
    println!("\n2. Fast serialization:");
    let serialized = serialize_raft_message_fast(&raft_msg)?;
    println!("   Serialized size: {} bytes", serialized.len());

    // Demonstrate fast deserialization
    println!("\n3. Fast deserialization:");
    let deserialized = deserialize_raft_message_fast(&serialized)?;
    println!("   Deserialized successfully");
    println!("   Type: {:?}", deserialized.get_msg_type());
    println!("   From: {}", deserialized.from);
    println!("   To: {}", deserialized.to);

    // Demonstrate zero-copy message creation
    println!("\n4. Zero-copy message creation:");
    
    // With borrowed data
    let borrowed_data = b"hello world";
    let zero_copy_msg = ZeroCopyMessage::new_borrowed(
        MessageType::RaftMessage,
        Some([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
        borrowed_data,
    );
    println!("   Borrowed message payload length: {}", zero_copy_msg.payload_len());
    println!("   Is large message: {}", zero_copy_msg.is_large_message());

    // With owned Bytes
    let owned_data = Bytes::from("zero copy is efficient");
    let zero_copy_msg_owned = ZeroCopyMessage::new_owned(
        MessageType::Request,
        None,
        owned_data,
    );
    println!("   Owned message payload length: {}", zero_copy_msg_owned.payload_len());

    // Demonstrate utility functions
    println!("\n5. Utility functions:");
    
    let heartbeat_msg = raft_utils::create_heartbeat_message();
    println!("   Heartbeat message created");
    println!("   Is heartbeat: {}", raft_utils::is_heartbeat_message(&heartbeat_msg));
    println!("   Should stream: {}", raft_utils::should_stream_message(&heartbeat_msg));

    // Create an RPC request for the Raft message
    let rpc_msg = raft_utils::create_raft_rpc_request(&raft_msg, None)?;
    println!("   RPC request created for Raft message");
    println!("   RPC payload length: {}", rpc_msg.payload_len());

    println!("\n✅ Zero-copy example completed successfully!");
    println!("\nKey benefits demonstrated:");
    println!("• Minimal allocations with Cow<[u8]>");
    println!("• Efficient serialization with pre-allocated buffers");
    println!("• Fast Raft message processing with prost");
    println!("• Streaming support for large messages");
    println!("• Optimized utility functions for common operations");

    Ok(())
}