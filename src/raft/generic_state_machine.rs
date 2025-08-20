//! Generic state machine trait for iroh-raft
//!
//! This module provides a generic, application-agnostic interface that any application
//! can implement to create their own state machine for distributed consensus.
//! The state machine handles the application of Raft log entries to maintain
//! consistent state across all nodes in the cluster.

use crate::error::RaftError;
use raft::prelude::Entry;
use std::future::Future;
// Removed unused imports

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
pub trait StateMachine: Send + Sync + 'static {
    /// Command type that can be applied to the state machine
    type Command: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    type State: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync;
    /// Response type returned by commands
    type Response: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    /// Query type for read-only operations
    type Query: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    /// Query response type
    type QueryResponse: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    
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
    /// - `Ok(response)` containing the command result
    fn apply_command(&mut self, command: Self::Command) -> impl Future<Output = Result<Self::Response, RaftError>> + Send + '_;

    /// Create a snapshot of the current state for efficient state transfer.
    /// 
    /// Snapshots are used when nodes join the cluster or fall behind and need
    /// to catch up. The snapshot should contain all information necessary to
    /// reconstruct the current state.
    /// 
    /// # Returns
    /// - `Ok(state)` containing the current state snapshot
    /// - `Err(error)` if there was an error creating the snapshot
    fn create_snapshot(&self) -> impl Future<Output = Result<Self::State, RaftError>> + Send + '_;

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
    fn restore_from_snapshot(&mut self, snapshot: Self::State) -> impl Future<Output = Result<(), RaftError>> + Send + '_;

    /// Get a read-only view of the current state.
    /// 
    /// This method allows applications to query the current state without
    /// modifying it. Useful for implementing read operations.
    /// 
    /// # Returns
    /// A reference to the current state
    fn get_current_state(&self) -> &Self::State;

    /// Execute a read-only query on the state machine.
    /// 
    /// This method allows applications to perform read-only operations
    /// without going through the Raft consensus process.
    /// 
    /// # Parameters
    /// - `query`: The query to execute
    /// 
    /// # Returns
    /// - `Ok(response)` containing the query result
    /// - `Err(error)` if there was an error executing the query
    fn execute_query(&self, query: Self::Query) -> impl Future<Output = Result<Self::QueryResponse, RaftError>> + Send + '_;
}

/// Generic wrapper that implements the actual Raft state machine processing
/// using an application-provided StateMachine implementation.
/// 
/// This handles the low-level details of Raft log entry processing, serialization,
/// and error handling, while delegating the actual state changes to the
/// application-specific state machine.
pub struct GenericRaftStateMachine<T>
where
    T: StateMachine,
{
    inner: T,
}

impl<T> GenericRaftStateMachine<T>
where
    T: StateMachine,
{
    /// Create a new generic Raft state machine wrapper around an application state machine
    pub fn new(state_machine: T) -> Self {
        Self {
            inner: state_machine,
        }
    }

    /// Apply a Raft log entry to the state machine
    /// 
    /// This method handles the deserialization of the entry data and delegates
    /// to the application's StateMachine implementation.
    pub async fn apply_entry(&mut self, entry: &Entry) -> Result<T::Response, RaftError> {
        // Skip empty entries
        if entry.data.is_empty() {
            return Err(RaftError::InvalidInput {
                parameter: "entry".to_string(),
                message: "Entry data is empty".to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        // Deserialize the command from the entry data
        let command: T::Command = bincode::deserialize(&entry.data)
            .map_err(RaftError::from)?;

        // Apply the command using the application state machine
        self.inner.apply_command(command).await
    }

    /// Create a snapshot of the current state
    pub async fn create_snapshot(&self) -> Result<Vec<u8>, RaftError> {
        let state = self.inner.create_snapshot().await?;
        
        bincode::serialize(&state)
            .map_err(RaftError::from)
    }

    /// Restore state from a snapshot
    pub async fn restore_from_snapshot(&mut self, snapshot_data: &[u8]) -> Result<(), RaftError> {
        let state: T::State = bincode::deserialize(snapshot_data)
            .map_err(RaftError::from)?;

        self.inner.restore_from_snapshot(state).await
    }

    /// Get access to the underlying state machine
    pub fn inner(&self) -> &T {
        &self.inner
    }


    /// Execute a read-only query on the state machine
    /// 
    /// This method executes queries without going through Raft consensus
    pub async fn execute_query(&self, query: T::Query) -> Result<T::QueryResponse, RaftError> {
        self.inner.execute_query(query).await
    }

    /// Get mutable access to the underlying state machine
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

// Implement StateMachine for GenericRaftStateMachine
impl<T> StateMachine for GenericRaftStateMachine<T>
where
    T: StateMachine,
{
    type Command = T::Command;
    type State = T::State;
    type Response = T::Response;
    type Query = T::Query;
    type QueryResponse = T::QueryResponse;

    async fn apply_command(&mut self, command: Self::Command) -> Result<Self::Response, RaftError> {
        self.inner.apply_command(command).await
    }

    async fn create_snapshot(&self) -> Result<Self::State, RaftError> {
        self.inner.create_snapshot().await
    }

    async fn restore_from_snapshot(&mut self, snapshot: Self::State) -> Result<(), RaftError> {
        self.inner.restore_from_snapshot(snapshot).await
    }

    fn get_current_state(&self) -> &Self::State {
        self.inner.get_current_state()
    }

    async fn execute_query(&self, query: Self::Query) -> Result<Self::QueryResponse, RaftError> {
        self.inner.execute_query(query).await
    }
}

/// Helper trait to create command entries for submission to Raft
pub trait CommandEncoder {
    /// Command type that can be encoded and submitted to Raft
    type Command: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    
    /// Encode a command into bytes for submission to Raft
    fn encode_command(command: Self::Command) -> Result<Vec<u8>, RaftError> {
        bincode::serialize(&command)
            .map_err(RaftError::from)
    }
}

// Implement CommandEncoder for any valid StateMachine type
impl<T> CommandEncoder for T
where
    T: StateMachine,
{
    type Command = T::Command;
}
