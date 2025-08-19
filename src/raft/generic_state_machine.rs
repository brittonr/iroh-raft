//! Generic state machine trait for iroh-raft
//!
//! This module provides a generic, application-agnostic interface that any application
//! can implement to create their own state machine for distributed consensus.
//! The state machine handles the application of Raft log entries to maintain
//! consistent state across all nodes in the cluster.

use crate::error::RaftError;
use raft::prelude::Entry;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Result type for state machine operations
pub type StateMachineResult<T> = Result<T, RaftError>;

/// Generic trait that applications must implement to define their state machine behavior.
/// 
/// The state machine is responsible for:
/// - Applying log entries to update the application state
/// - Creating snapshots of the current state for efficient state transfer
/// - Restoring state from snapshots when nodes join the cluster
/// 
/// Applications define their own `Command` and `State` types to represent
/// operations and the current state of their system.
pub trait StateMachine<Command, State>
where
    Command: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
    State: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    /// Apply a command from a Raft log entry to update the state machine.
    /// 
    /// This method is called for each committed log entry in order.
    /// The implementation should be deterministic - the same command
    /// applied to the same state should always produce the same result.
    /// 
    /// # Parameters
    /// - `command`: The command to apply, deserialized from the log entry
    /// 
    /// # Returns
    /// - `Ok(())` if the command was applied successfully
    /// - `Err(error)` if there was an error applying the command
    async fn apply_command(&mut self, command: Command) -> StateMachineResult<()>;

    /// Create a snapshot of the current state for efficient state transfer.
    /// 
    /// Snapshots are used when nodes join the cluster or fall behind and need
    /// to catch up. The snapshot should contain all information necessary to
    /// reconstruct the current state.
    /// 
    /// # Returns
    /// - `Ok(state)` containing the current state snapshot
    /// - `Err(error)` if there was an error creating the snapshot
    async fn create_snapshot(&self) -> StateMachineResult<State>;

    /// Restore the state machine from a snapshot.
    /// 
    /// This replaces the current state with the state from the snapshot.
    /// This method is called when a node needs to catch up using a snapshot
    /// from another node.
    /// 
    /// # Parameters
    /// - `snapshot`: The state to restore from
    /// 
    /// # Returns
    /// - `Ok(())` if the state was restored successfully  
    /// - `Err(error)` if there was an error restoring from the snapshot
    async fn restore_from_snapshot(&mut self, snapshot: State) -> StateMachineResult<()>;

    /// Get a read-only view of the current state.
    /// 
    /// This method allows applications to query the current state without
    /// modifying it. Useful for implementing read operations.
    /// 
    /// # Returns
    /// A reference to the current state
    fn get_current_state(&self) -> &State;
}

/// Generic wrapper that implements the actual Raft state machine processing
/// using an application-provided StateMachine implementation.
/// 
/// This handles the low-level details of Raft log entry processing, serialization,
/// and error handling, while delegating the actual state changes to the
/// application-specific state machine.
pub struct GenericRaftStateMachine<T, Command, State>
where
    T: StateMachine<Command, State>,
    Command: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
    State: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    inner: T,
    _phantom_command: std::marker::PhantomData<Command>,
    _phantom_state: std::marker::PhantomData<State>,
}

impl<T, Command, State> GenericRaftStateMachine<T, Command, State>
where
    T: StateMachine<Command, State>,
    Command: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
    State: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    /// Create a new generic Raft state machine wrapper around an application state machine
    pub fn new(state_machine: T) -> Self {
        Self {
            inner: state_machine,
            _phantom_command: std::marker::PhantomData,
            _phantom_state: std::marker::PhantomData,
        }
    }

    /// Apply a Raft log entry to the state machine
    /// 
    /// This method handles the deserialization of the entry data and delegates
    /// to the application's StateMachine implementation.
    pub async fn apply_entry(&mut self, entry: &Entry) -> StateMachineResult<()> {
        // Skip empty entries
        if entry.data.is_empty() {
            return Ok(());
        }

        // Deserialize the command from the entry data
        let command: Command = bincode::deserialize(&entry.data)
            .map_err(|e| RaftError::Serialization {
                message_type: "log entry command".to_string(),
                source: Box::new(e),
            })?;

        // Apply the command using the application state machine
        self.inner.apply_command(command).await
    }

    /// Create a snapshot of the current state
    pub async fn create_snapshot(&self) -> StateMachineResult<Vec<u8>> {
        let state = self.inner.create_snapshot().await?;
        
        bincode::serialize(&state)
            .map_err(|e| RaftError::Serialization {
                message_type: "state snapshot".to_string(),
                source: Box::new(e),
            })
    }

    /// Restore state from a snapshot
    pub async fn restore_from_snapshot(&mut self, snapshot_data: &[u8]) -> StateMachineResult<()> {
        let state: State = bincode::deserialize(snapshot_data)
            .map_err(|e| RaftError::Serialization {
                message_type: "state snapshot".to_string(),
                source: Box::new(e),
            })?;

        self.inner.restore_from_snapshot(state).await
    }

    /// Get access to the underlying state machine
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get mutable access to the underlying state machine
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

/// Helper trait to create command entries for submission to Raft
pub trait CommandEncoder<Command>
where
    Command: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    /// Encode a command into bytes for submission to Raft
    fn encode_command(command: Command) -> StateMachineResult<Vec<u8>> {
        bincode::serialize(&command)
            .map_err(|e| RaftError::Serialization {
                message_type: "command".to_string(),
                source: Box::new(e),
            })
    }
}

// Implement CommandEncoder for any valid Command type
impl<Command> CommandEncoder<Command> for Command
where
    Command: Clone + Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
}