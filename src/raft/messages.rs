//! Raft message types and routing
//!
//! This module defines the message types used for Raft communication
//! and provides utilities for message routing and handling.

use crate::error::Result;
use raft::prelude::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::types::ProposalData;

/// Main message type for Raft communication
#[derive(Debug)]
pub enum RaftMessage {
    /// Raft protocol messages (heartbeats, votes, etc.)
    Raft(Message),
    /// Application-specific proposals
    Propose(RaftProposal),
    /// Configuration changes (add/remove nodes)
    ConfChange(RaftConfChange),
}

/// Configuration change request
#[derive(Debug)]
pub struct RaftConfChange {
    /// Type of configuration change (add or remove node)
    pub change_type: ConfChangeType,
    /// Raft node ID being added or removed
    pub node_id: u64,
    /// Network address of the node
    pub address: String,
    /// Optional Iroh P2P node ID
    pub p2p_node_id: Option<String>,
    /// List of P2P addresses for the node
    pub p2p_addresses: Vec<String>,
    /// Optional relay server URL for NAT traversal
    pub p2p_relay_url: Option<String>,
    /// Optional response channel for operation confirmation
    pub response_tx: Option<oneshot::Sender<Result<()>>>,
}

/// Type of configuration change
#[derive(Debug, Clone, Copy)]
pub enum ConfChangeType {
    /// Add a new node to the cluster
    AddNode,
    /// Remove an existing node from the cluster
    RemoveNode,
}

/// Configuration change context that includes P2P info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfChangeContext {
    /// Network address of the node
    pub address: String,
    /// Optional Iroh P2P node ID
    pub p2p_node_id: Option<String>,
    /// List of P2P addresses for the node
    pub p2p_addresses: Vec<String>,
    /// Optional relay server URL for NAT traversal
    pub p2p_relay_url: Option<String>,
}

/// Raft proposal wrapper
#[derive(Debug)]
pub struct RaftProposal {
    /// Unique proposal ID
    pub id: Vec<u8>,
    /// Proposal data
    pub data: ProposalData,
    /// Response channel for proposal result
    pub response_tx: Option<oneshot::Sender<Result<()>>>,
}

impl RaftProposal {
    /// Create a new proposal with a generated ID
    pub fn new(data: ProposalData) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string().into_bytes(),
            data,
            response_tx: None,
        }
    }

    /// Create a proposal with a response channel
    pub fn with_response(data: ProposalData) -> (Self, oneshot::Receiver<Result<()>>) {
        let (tx, rx) = oneshot::channel();
        let proposal = Self {
            id: uuid::Uuid::new_v4().to_string().into_bytes(),
            data,
            response_tx: Some(tx),
        };
        (proposal, rx)
    }
}

/// Message routing information
#[derive(Debug, Clone)]
pub struct MessageRoute {
    /// Source node ID
    pub from: u64,
    /// Destination node ID
    pub to: u64,
    /// Type of Raft message
    pub msg_type: MessageType,
}

/// High-level Raft message types
#[derive(Debug, Clone, Copy)]
pub enum MessageType {
    /// Vote request for leader election
    Vote,
    /// Heartbeat message to maintain leadership
    Heartbeat,
    /// Log append message for replication
    Append,
    /// Snapshot transfer message
    Snapshot,
    /// Response to vote request
    VoteResponse,
    /// Response to heartbeat message
    HeartbeatResponse,
    /// Response to log append message
    AppendResponse,
    /// Response to snapshot transfer
    SnapshotResponse,
}

impl From<&Message> for MessageType {
    fn from(msg: &Message) -> Self {
        use raft::prelude::MessageType as RaftMsgType;
        match msg.msg_type() {
            RaftMsgType::MsgHup => MessageType::Vote,
            RaftMsgType::MsgBeat => MessageType::Heartbeat,
            RaftMsgType::MsgPropose => MessageType::Append,
            RaftMsgType::MsgAppend => MessageType::Append,
            RaftMsgType::MsgAppendResponse => MessageType::AppendResponse,
            RaftMsgType::MsgRequestVote => MessageType::Vote,
            RaftMsgType::MsgRequestVoteResponse => MessageType::VoteResponse,
            RaftMsgType::MsgSnapshot => MessageType::Snapshot,
            RaftMsgType::MsgHeartbeat => MessageType::Heartbeat,
            RaftMsgType::MsgHeartbeatResponse => MessageType::HeartbeatResponse,
            _ => MessageType::Heartbeat, // Default for other types
        }
    }
}

/// Serialize a Raft message  
pub fn serialize_message(msg: &Message) -> Result<Vec<u8>> {
    // Use prost 0.11 for serialization (raft uses prost-codec with 0.11)
    use prost::Message as ProstMessage;
    
    let encoded = msg.encode_to_vec();
    Ok(encoded)
}

/// Deserialize a Raft message
pub fn deserialize_message(data: &[u8]) -> Result<Message> {
    // Use prost 0.11 for deserialization (raft uses prost-codec with 0.11)
    use prost::Message as ProstMessage;
    
    Message::decode(data).map_err(|e| crate::error::RaftError::Serialization {
        backtrace: snafu::Backtrace::new(),
        message_type: "Message".to_string(),
        source: Box::new(e),
    })
}

impl MessageRoute {
    /// Create routing information from a Raft message
    pub fn from_message(msg: &Message) -> Self {
        Self {
            from: msg.from,
            to: msg.to,
            msg_type: MessageType::from(msg),
        }
    }
}
