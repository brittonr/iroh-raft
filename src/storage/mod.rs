//! Storage implementations for Raft consensus
//! 
//! This module provides storage backends and utilities for Raft consensus,
//! including redb-based persistent storage and codec utilities for 
//! serialization/deserialization of Raft data structures.

pub mod codec;
pub mod simple;

// Re-export commonly used items
pub use codec::{
    serialize_entry, deserialize_entry, 
    serialize_hard_state, deserialize_hard_state, 
    serialize_conf_state, deserialize_conf_state, 
    serialize_snapshot, deserialize_snapshot
};
pub use simple::RaftStorage;