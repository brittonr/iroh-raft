//! Core types and data structures for iroh-raft
//!
//! This module defines the fundamental types used throughout the iroh-raft library
//! including node identifiers, proposal data, and node configuration.

// Re-export node configuration for convenience
pub use crate::config::NodeConfig;

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a Raft node
pub type NodeId = u64;

/// Data that can be proposed to the Raft cluster
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalData {
    /// Key-value store operation
    KeyValue(KeyValueOp),
    /// Raw bytes
    Raw(Vec<u8>),
    /// Text data
    Text(String),
    /// No operation (used for testing)
    Noop,
}


impl ProposalData {
    /// Create a key-value set operation
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::KeyValue(KeyValueOp::Set {
            key: key.into(),
            value: value.into(),
        })
    }

    /// Create a key-value get operation
    pub fn get(key: impl Into<String>) -> Self {
        Self::KeyValue(KeyValueOp::Get {
            key: key.into(),
        })
    }

    /// Create a key-value delete operation
    pub fn delete(key: impl Into<String>) -> Self {
        Self::KeyValue(KeyValueOp::Delete {
            key: key.into(),
        })
    }

    /// Create a text proposal
    pub fn text(data: impl Into<String>) -> Self {
        Self::Text(data.into())
    }

    /// Create a raw bytes proposal
    pub fn raw(data: Vec<u8>) -> Self {
        Self::Raw(data)
    }

    /// Create a no-op proposal
    pub fn noop() -> Self {
        Self::Noop
    }
}

impl fmt::Display for ProposalData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposalData::KeyValue(op) => {
                match op {
                    KeyValueOp::Set { key, value } => write!(f, "SET {} = {}", key, value),
                    KeyValueOp::Get { key } => write!(f, "GET {}", key),
                    KeyValueOp::Delete { key } => write!(f, "DELETE {}", key),
                    KeyValueOp::List { prefix } => write!(f, "LIST {:?}", prefix),
                }
            }
            ProposalData::Raw(bytes) => write!(f, "RAW({} bytes)", bytes.len()),
            ProposalData::Text(text) => write!(f, "TEXT({})", text),
            ProposalData::Noop => write!(f, "NOOP"),
        }
    }
}

/// Virtual machine identifier
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct VmId(pub Uuid);

impl VmId {
    /// Create a new random VM ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create VM ID from bytes
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }
}

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vm-{}", self.0)
    }
}

impl Default for VmId {
    fn default() -> Self {
        Self::new()
    }
}

/// Virtual machine status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VmStatus {
    /// VM is being created
    Creating,
    /// VM is running
    Running,
    /// VM is stopped
    Stopped,
    /// VM is paused
    Paused,
    /// VM creation or operation failed
    Failed(String),
}

impl fmt::Display for VmStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmStatus::Creating => write!(f, "creating"),
            VmStatus::Running => write!(f, "running"),
            VmStatus::Stopped => write!(f, "stopped"),
            VmStatus::Paused => write!(f, "paused"),
            VmStatus::Failed(reason) => write!(f, "failed: {}", reason),
        }
    }
}

impl Default for VmStatus {
    fn default() -> Self {
        VmStatus::Creating
    }
}

/// Node role in the Raft cluster
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    /// Follower node
    Follower,
    /// Candidate node (during election)
    Candidate,
    /// Leader node
    Leader,
}

impl fmt::Display for NodeRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeRole::Follower => write!(f, "follower"),
            NodeRole::Candidate => write!(f, "candidate"),
            NodeRole::Leader => write!(f, "leader"),
        }
    }
}

impl Default for NodeRole {
    fn default() -> Self {
        NodeRole::Follower
    }
}

/// Current status of a Raft node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeStatus {
    /// The node ID
    pub node_id: NodeId,
    /// Current role in the cluster
    pub role: NodeRole,
    /// Current term
    pub term: u64,
    /// Current leader ID (if known)
    pub leader_id: Option<NodeId>,
    /// Last log index
    pub last_log_index: u64,
    /// Committed log index
    pub committed_index: u64,
    /// Applied log index
    pub applied_index: u64,
    /// List of connected peer IDs
    pub connected_peers: Vec<NodeId>,
    /// Node uptime in seconds
    pub uptime_seconds: u64,
}

impl NodeStatus {
    /// Create a new node status
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            role: NodeRole::Follower,
            term: 0,
            leader_id: None,
            last_log_index: 0,
            committed_index: 0,
            applied_index: 0,
            connected_peers: Vec::new(),
            uptime_seconds: 0,
        }
    }
    
    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        self.role == NodeRole::Leader
    }
    
    /// Check if this node is a follower
    pub fn is_follower(&self) -> bool {
        self.role == NodeRole::Follower
    }
    
    /// Check if this node is a candidate
    pub fn is_candidate(&self) -> bool {
        self.role == NodeRole::Candidate
    }
}

impl fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f, 
            "Node {} [{}] term={} leader={:?} log={}/{}/{} peers={}",
            self.node_id,
            self.role,
            self.term,
            self.leader_id,
            self.last_log_index,
            self.committed_index,
            self.applied_index,
            self.connected_peers.len()
        )
    }
}

/// Updated key-value operations with the correct structure expected in the codebase
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyValueOp {
    /// Set a key to a value
    Set { key: String, value: String },
    /// Get the value of a key
    Get { key: String },
    /// Delete a key
    Delete { key: String },
    /// List keys with optional prefix
    List { prefix: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_data() {
        let set_op = ProposalData::set("key1", "value1");
        assert_eq!(format!("{}", set_op), "SET key1 = value1");

        let get_op = ProposalData::get("key1");
        assert_eq!(format!("{}", get_op), "GET key1");

        let delete_op = ProposalData::delete("key1");
        assert_eq!(format!("{}", delete_op), "DELETE key1");

        let text_op = ProposalData::text("hello world");
        assert_eq!(format!("{}", text_op), "TEXT(hello world)");

        let raw_op = ProposalData::raw(vec![1, 2, 3, 4]);
        assert_eq!(format!("{}", raw_op), "RAW(4 bytes)");

        let noop = ProposalData::noop();
        assert_eq!(format!("{}", noop), "NOOP");
    }

    #[test]
    fn test_vm_id() {
        let id1 = VmId::new();
        let id2 = VmId::new();
        
        // IDs should be different
        assert_ne!(id1, id2);
        
        // Should display with vm- prefix
        assert!(format!("{}", id1).starts_with("vm-"));
        
        // Should be serializable
        let json = serde_json::to_string(&id1).unwrap();
        let deserialized: VmId = serde_json::from_str(&json).unwrap();
        assert_eq!(id1, deserialized);
    }

    #[test]
    fn test_vm_status() {
        assert_eq!(format!("{}", VmStatus::Creating), "creating");
        assert_eq!(format!("{}", VmStatus::Running), "running");
        assert_eq!(format!("{}", VmStatus::Stopped), "stopped");
        assert_eq!(format!("{}", VmStatus::Paused), "paused");
        assert_eq!(format!("{}", VmStatus::Failed("timeout".to_string())), "failed: timeout");
    }

    #[test]
    fn test_serialization() {
        let proposal = ProposalData::set("test_key", "test_value");
        
        // Test JSON serialization
        let json = serde_json::to_string(&proposal).unwrap();
        let deserialized: ProposalData = serde_json::from_str(&json).unwrap();
        assert_eq!(proposal, deserialized);
        
        // Test binary serialization
        let bytes = bincode::serialize(&proposal).unwrap();
        let deserialized: ProposalData = bincode::deserialize(&bytes).unwrap();
        assert_eq!(proposal, deserialized);
    }
}