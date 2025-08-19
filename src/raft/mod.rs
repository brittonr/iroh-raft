//! Generic Raft consensus implementation for iroh-raft
//!
//! This module provides a generic, application-agnostic Raft consensus implementation
//! that any application can use by implementing the StateMachine trait.
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                 Generic Raft Architecture                       │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Application Layer      │  Generic Layer      │  Storage Layer  │
//! │  ┌─────────────────┐    │  ┌─────────────────┐ │  ┌───────────┐ │
//! │  │ Custom Commands │    │  │ Generic         │ │  │ Persistent│ │
//! │  │ • App Logic     │ ──▶│  │ Proposals       │─│──│ Logs      │ │
//! │  │ • Domain Types  │    │  │ • Metadata      │ │  │ • Entries │ │
//! │  │ • Business Rules│    │  │ • Batching      │ │  │ • State   │ │
//! │  └─────────────────┘    │  └─────────────────┘ │  └───────────┘ │
//! │                         │                       │               │
//! │  ┌─────────────────┐    │  ┌─────────────────┐ │  ┌───────────┐ │
//! │  │ StateMachine    │    │  │ Raft Consensus  │ │  │ Snapshots │ │
//! │  │ • apply_command │ ◀──│  │ • Leader Election│ │  │ • State   │ │
//! │  │ • create_snapshot│    │  │ • Log Replication│ │  │ Capture   │ │
//! │  │ • restore_snapshot│   │  │ • Commitment    │ │  │ • Install │ │
//! │  └─────────────────┘    │  └─────────────────┘ │  └───────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Core Components
//!
//! ### 1. Generic State Machine (`generic_state_machine`)
//! The foundational trait that applications implement:
//! - **Command Application**: `apply_command()` for deterministic state changes
//! - **Snapshot Creation**: `create_snapshot()` for efficient state transfer
//! - **Snapshot Restoration**: `restore_from_snapshot()` for catching up nodes
//! - **State Access**: `get_current_state()` for read operations
//!
//! ### 2. Example Implementation (`kv_example`)
//! A complete key-value store demonstrating the StateMachine trait:
//! - **Commands**: Set, Delete, Clear operations
//! - **State**: HashMap with metadata (operation count, version, timestamp)
//! - **Snapshots**: Full state serialization and restoration
//! - **Tests**: Comprehensive test suite showing usage patterns
//!
//! ### 3. Generic Proposals (`proposals`)
//! Application-agnostic proposal system:
//! - **Generic Commands**: Wraps any application command type
//! - **Metadata**: Tracking, correlation, priority, and originator information
//! - **Batching**: Efficient multi-operation processing
//! - **Builder Pattern**: Easy proposal construction
//!
//! ## Key Features
//!
//! ### 1. Type Safety
//! - **Generic Types**: Applications define their own Command and State types
//! - **Compile-time Guarantees**: SerDe constraints ensure serializable types
//! - **Trait Bounds**: Enforces proper Debug, Clone, and Serialize implementations
//!
//! ### 2. Flexibility
//! - **Any Command Type**: Support for enums, structs, or any serializable type
//! - **Any State Type**: From simple values to complex nested structures
//! - **Custom Logic**: Applications implement their own business rules
//!
//! ### 3. Performance
//! - **Efficient Serialization**: Uses bincode for fast binary serialization
//! - **Batch Processing**: Multiple commands in single Raft consensus round
//! - **Lazy Snapshots**: Only created when needed for catching up nodes
//!
//! ## Usage Examples
//!
//! ### Implementing a Custom State Machine
//! ```rust,no_run
//! use iroh_raft::raft::StateMachine;
//! use iroh_raft::error::RaftError;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Debug, Serialize, Deserialize)]
//! pub enum MyCommand {
//!     Increment { amount: u64 },
//!     Reset,
//! }
//!
//! #[derive(Clone, Debug, Serialize, Deserialize)]
//! pub struct MyState {
//!     pub counter: u64,
//! }
//!
//! pub struct MyStateMachine {
//!     state: MyState,
//! }
//!
//! impl StateMachine for MyStateMachine {
//!     type Command = MyCommand;
//!     type State = MyState;
//!
//!     async fn apply_command(&mut self, command: Self::Command) -> Result<(), RaftError> {
//!         match command {
//!             MyCommand::Increment { amount } => {
//!                 self.state.counter += amount;
//!             }
//!             MyCommand::Reset => {
//!                 self.state.counter = 0;
//!             }
//!         }
//!         Ok(())
//!     }
//!
//!     async fn create_snapshot(&self) -> Result<Self::State, RaftError> {
//!         Ok(self.state.clone())
//!     }
//!
//!     async fn restore_from_snapshot(&mut self, snapshot: Self::State) -> Result<(), RaftError> {
//!         self.state = snapshot;
//!         Ok(())
//!     }
//!
//!     fn get_current_state(&self) -> &Self::State {
//!         &self.state
//!     }
//! }
//! ```
//!
//! ### Using the Key-Value Store Example
//! ```rust,no_run
//! use iroh_raft::raft::{ExampleRaftStateMachine, KvCommand, StateMachine};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut state_machine = ExampleRaftStateMachine::new_kv_store();
//!     
//!     // Apply commands
//!     state_machine.inner_mut().apply_command(KvCommand::Set {
//!         key: "user:123".to_string(),
//!         value: "Alice".to_string(),
//!     }).await?;
//!     
//!     // Check state
//!     let state = state_machine.inner().get_current_state();
//!     println!("Value: {:?}", state.get("user:123"));
//!     
//!     Ok(())
//! }
//! ```

// Core generic components
pub mod generic_state_machine;
pub mod kv_example;
pub mod messages;
pub mod proposals;
pub mod state_machine;

// Re-export commonly used types for convenience
pub use self::state_machine::{
    CommandEncoder, ExampleRaftStateMachine, GenericRaftStateMachine, 
    KvCommand, KvState, KeyValueStore, StateMachine
};

// Re-export generic proposals
pub use self::proposals::{
    BatchProposal, BatchProposalBuilder, GenericProposal, ProposalMetadata
};

/// Version information for the generic Raft implementation
pub const GENERIC_RAFT_VERSION: &str = "0.1.0";

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_generic_raft_integration() {
        // Test creating and using the example state machine
        let mut sm = ExampleRaftStateMachine::new_kv_store();
        
        // Test command application
        sm.inner_mut().apply_command(KvCommand::Set {
            key: "test".to_string(),
            value: "value".to_string(),
        }).await.unwrap();
        
        // Test state access
        let state = sm.inner().get_current_state();
        assert_eq!(state.get("test"), Some(&"value".to_string()));
        assert_eq!(state.len(), 1);
        
        // Test snapshot creation and restoration
        let snapshot_data = sm.create_snapshot().await.unwrap();
        assert!(!snapshot_data.is_empty());
        
        let mut new_sm = ExampleRaftStateMachine::new_kv_store();
        new_sm.restore_from_snapshot(&snapshot_data).await.unwrap();
        
        let restored_state = new_sm.inner().get_current_state();
        assert_eq!(restored_state.get("test"), Some(&"value".to_string()));
        assert_eq!(restored_state.len(), 1);
    }

    #[tokio::test] 
    async fn test_generic_proposals() {
        let command = KvCommand::Set {
            key: "key1".to_string(),
            value: "value1".to_string(),
        };

        // Test single proposal
        let proposal = GenericProposal::new(command.clone())
            .with_originator("test_node".to_string())
            .with_priority(5);
            
        assert_eq!(proposal.metadata.priority, 5);
        assert_eq!(proposal.metadata.originator, Some("test_node".to_string()));

        // Test batch proposal
        let batch = BatchProposalBuilder::new()
            .add_command(command.clone())
            .add_command(KvCommand::Delete { key: "key1".to_string() })
            .with_originator("batch_creator".to_string())
            .build();
            
        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert_eq!(batch.batch_metadata.originator, Some("batch_creator".to_string()));
    }
}