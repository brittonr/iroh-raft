//! Generic state machine implementation for iroh-raft
//!
//! This module re-exports the generic state machine components and provides
//! a backward-compatible interface for existing applications.
//!
//! Applications should use the generic StateMachine trait to implement their
//! own distributed state machines with Raft consensus.

pub use super::generic_state_machine::{
    CommandEncoder, GenericRaftStateMachine, StateMachine, StateMachineResult,
};

// Re-export the key-value store example as a reference implementation
pub use super::kv_example::{KvCommand, KvMetadata, KvState, KeyValueStore};

// Re-export generic proposal types
pub use super::proposals::{
    BatchProposal, BatchProposalBuilder, GenericProposal, ProposalMetadata,
};

/// Type alias for a generic Raft state machine using the key-value store example
/// 
/// This provides a concrete instantiation of the generic state machine that
/// applications can use as a starting point or for testing purposes.
pub type ExampleRaftStateMachine = GenericRaftStateMachine<KeyValueStore, KvCommand, KvState>;

impl ExampleRaftStateMachine {
    /// Create a new example Raft state machine using the key-value store
    pub fn new_kv_store() -> Self {
        GenericRaftStateMachine::new(KeyValueStore::new())
    }

    /// Create a new example Raft state machine with initial state
    pub fn new_kv_store_with_state(state: KvState) -> Self {
        GenericRaftStateMachine::new(KeyValueStore::with_state(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_example_state_machine_creation() {
        let sm = ExampleRaftStateMachine::new_kv_store();
        let state = sm.inner().get_current_state();
        
        assert_eq!(state.len(), 0);
        assert!(state.is_empty());
        assert_eq!(state.metadata.operation_count, 0);
    }

    #[tokio::test]
    async fn test_example_state_machine_with_initial_state() {
        let mut initial_state = KvState::new();
        initial_state.metadata.operation_count = 5;
        initial_state.metadata.version = 3;
        
        let sm = ExampleRaftStateMachine::new_kv_store_with_state(initial_state);
        let state = sm.inner().get_current_state();
        
        assert_eq!(state.metadata.operation_count, 5);
        assert_eq!(state.metadata.version, 3);
    }
}