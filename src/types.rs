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
    KeyValue { 
        /// The operation type
        op: KeyValueOp, 
        /// The key to operate on
        key: String, 
        /// The value (for set operations)
        value: Option<String> 
    },
    /// Raw bytes
    Raw(Vec<u8>),
    /// Text data
    Text(String),
    /// No operation (used for testing)
    Noop,
}

/// Key-value operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyValueOp {
    /// Set a key to a value
    Set,
    /// Get the value of a key
    Get,
    /// Delete a key
    Delete,
}

impl ProposalData {
    /// Create a key-value set operation
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::KeyValue {
            op: KeyValueOp::Set,
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// Create a key-value get operation
    pub fn get(key: impl Into<String>) -> Self {
        Self::KeyValue {
            op: KeyValueOp::Get,
            key: key.into(),
            value: None,
        }
    }

    /// Create a key-value delete operation
    pub fn delete(key: impl Into<String>) -> Self {
        Self::KeyValue {
            op: KeyValueOp::Delete,
            key: key.into(),
            value: None,
        }
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
            ProposalData::KeyValue { op, key, value } => {
                match op {
                    KeyValueOp::Set => write!(f, "SET {} = {:?}", key, value),
                    KeyValueOp::Get => write!(f, "GET {}", key),
                    KeyValueOp::Delete => write!(f, "DELETE {}", key),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_data() {
        let set_op = ProposalData::set("key1", "value1");
        assert_eq!(format!("{}", set_op), "SET key1 = Some(\"value1\")");

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