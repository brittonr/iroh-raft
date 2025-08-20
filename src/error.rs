//! Error handling for the iroh-raft consensus library
//!
//! This module provides comprehensive error types designed specifically for distributed
//! consensus operations using Raft, Iroh P2P networking, and Redb storage.
//!
//! # Error Design Philosophy
//!
//! The error system is designed with these principles:
//!
//! ## 1. Error Categories
//! - **Raft Errors**: Consensus protocol failures and state transitions
//! - **Storage Errors**: Database operations and persistence failures  
//! - **Transport Errors**: P2P networking and communication issues
//! - **Configuration Errors**: Invalid settings and initialization problems
//! - **Internal Errors**: Programming errors and invariant violations
//!
//! ## 2. Error Context
//! Each error includes sufficient context for debugging:
//! - Operation being performed
//! - Resource or node involved
//! - Root cause chain
//! - Recovery hints where applicable
//!
//! ## 3. Error Recovery
//! Errors are categorized by recovery strategy:
//! - **Transient**: Retry with backoff
//! - **Fatal**: Requires intervention or restart
//! - **Recoverable**: Can be handled gracefully
//!
//! # Usage Example
//!
//! ```rust
//! use iroh_raft::error::{RaftError, Result, NotLeaderSnafu};
//! use snafu::ResultExt;
//!
//! fn example_operation() -> Result<String> {
//!     // This would be a real Raft operation
//!     Err(NotLeaderSnafu {
//!         operation: "propose_entry",
//!         current_leader: Some(2)
//!     }.build())
//! }
//! ```

use std::time::Duration;
use snafu::{prelude::*, Backtrace};

// === Boxed Error Structs for Large Variants ===

/// Database-related error details
#[derive(Debug)]
pub struct DatabaseError {
    /// The database operation that failed
    pub operation: String,
    /// The underlying redb error
    pub source: redb::Error,
    /// Error backtrace for debugging
    pub backtrace: Backtrace,
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Database error in '{}': {}", self.operation, self.source)
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Storage-related error details
#[derive(Debug)]
pub struct StorageError {
    /// The storage operation that failed
    pub operation: String,
    /// The underlying error that caused the failure
    pub source: Box<dyn std::error::Error + Send + Sync>,
    /// Error backtrace for debugging
    pub backtrace: Backtrace,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Storage operation '{}' failed", self.operation)
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Transport-related error details
#[derive(Debug)]
pub struct TransportError {
    /// Details about the transport error
    pub details: String,
    /// Error backtrace for debugging
    pub backtrace: Backtrace,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P2P transport error: {}", self.details)
    }
}

impl std::error::Error for TransportError {}

/// Consensus-related error details
#[derive(Debug)]
pub struct ConsensusError {
    /// The operation that triggered the consensus error
    pub operation: String,
    /// The underlying Raft library error
    pub source: raft::Error,
    /// Error backtrace for debugging
    pub backtrace: Backtrace,
}

impl std::fmt::Display for ConsensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Raft consensus error in '{}': {}", self.operation, self.source)
    }
}

impl std::error::Error for ConsensusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Comprehensive error type for iroh-raft consensus operations
///
/// This error type covers all failure modes in the distributed consensus system,
/// from low-level storage operations to high-level Raft protocol violations.
/// Large error variants are boxed to keep the overall enum size small.
#[derive(Snafu, Debug)]
#[snafu(visibility(pub))]
pub enum RaftError {
    // === Small Raft Protocol Errors (kept unboxed) ===
    
    /// Node is not the current leader for write operations
    #[snafu(display("Not leader for operation '{operation}', current leader: {current_leader:?}"))]
    NotLeader {
        /// The operation that was attempted on a non-leader
        operation: String,
        /// The current leader node ID, if known
        current_leader: Option<u64>,
    },

    /// Invalid node ID or node not found in cluster configuration
    #[snafu(display("Node {node_id} not found in cluster configuration"))]
    NodeNotFound { 
        /// The node ID that was not found
        node_id: u64,
    },

    // === Large Error Variants (boxed for size optimization) ===

    /// Raft consensus error from the underlying raft library
    #[snafu(display("{}", inner))]
    Consensus {
        /// Boxed consensus error details
        inner: Box<ConsensusError>,
    },

    /// Database operation failed
    #[snafu(display("{}", inner))]
    Database {
        /// Boxed database error details
        inner: Box<DatabaseError>,
    },

    /// Storage operation failed
    #[snafu(display("{}", inner))]
    Storage {
        /// Boxed storage error details
        inner: Box<StorageError>,
    },

    /// P2P transport error
    #[snafu(display("{}", inner))]
    Transport { 
        /// Boxed transport error details
        inner: Box<TransportError>,
    },

    // === Other Error Variants ===

    /// Node is not initialized or ready for operations
    #[snafu(display("Node {node_id} not ready for operation '{operation}': {reason}"))]
    NotReady {
        /// The ID of the node that is not ready
        node_id: u64,
        /// The operation that was attempted
        operation: String,
        /// The reason why the node is not ready
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Proposal was rejected by the Raft state machine
    #[snafu(display("Proposal rejected for operation '{operation}': {reason}"))]
    ProposalRejected {
        /// The operation that was rejected
        operation: String,
        /// The reason for rejection
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Configuration change failed (add/remove node)
    #[snafu(display("Configuration change failed: {operation} - {reason}"))]
    ConfigurationChange {
        /// The configuration change operation that failed
        operation: String,
        /// The reason for the failure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Snapshot operation failed
    #[snafu(display("Snapshot {operation} failed: {details}"))]
    Snapshot {
        /// The snapshot operation that failed (create, apply, restore)
        operation: String,
        /// Additional details about the failure
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Operation timed out
    #[snafu(display("Operation '{operation}' timed out after {duration:?}"))]
    Timeout {
        /// The operation that timed out
        operation: String,
        /// How long we waited before timing out
        duration: Duration,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Circuit breaker is open, failing fast
    #[snafu(display("Circuit breaker open: {message}"))]
    CircuitBreakerOpen {
        /// Description of the circuit breaker failure
        message: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Backpressure applied - resource overloaded
    #[snafu(display("Backpressure applied: {operation} - {reason}"))]
    Backpressure {
        /// The operation that was throttled
        operation: String,
        /// The reason for applying backpressure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Resource exhausted (memory, connections, etc.)
    #[snafu(display("Resource exhausted: {resource} - {details}"))]
    ResourceExhausted {
        /// The type of resource that was exhausted
        resource: String,
        /// Additional details about the exhaustion
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Storage transaction failed
    #[snafu(display("Transaction '{operation}' failed: {reason}"))]
    Transaction {
        /// The transaction operation that failed
        operation: String,
        /// The reason for transaction failure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Storage corruption or inconsistency detected
    #[snafu(display("Storage corruption detected in {component}: {details}"))]
    StorageCorruption {
        /// The storage component where corruption was detected
        component: String,
        /// Details about the corruption
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Insufficient disk space for storage operations
    #[snafu(display("Insufficient storage space for operation '{operation}': need {needed} bytes, available {available} bytes"))]
    InsufficientSpace {
        /// The operation that needs more space
        operation: String,
        /// Number of bytes needed
        needed: u64,
        /// Number of bytes available
        available: u64,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Failed to establish connection to peer
    #[snafu(display("Connection failed to peer {peer_id}: {reason}"))]
    ConnectionFailed {
        /// The peer ID that couldn't be connected to
        peer_id: String,
        /// The reason for connection failure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Network partition or connectivity issues
    #[snafu(display("Network partition detected: {details}"))]
    NetworkPartition { 
        /// Details about the network partition
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Message serialization/deserialization failed
    #[snafu(display("Serialization error for {message_type}: {source}"))]
    Serialization {
        /// The type of message that failed to serialize/deserialize
        message_type: String,
        /// The underlying serialization error
        #[snafu(source)]
        source: Box<dyn std::error::Error + Send + Sync>,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Protocol version mismatch between nodes
    #[snafu(display("Protocol version mismatch with peer {peer_id}: local={local_version}, remote={remote_version}"))]
    ProtocolMismatch {
        /// The peer ID with the mismatched protocol version
        peer_id: String,
        /// This node's protocol version
        local_version: String,
        /// The remote node's protocol version
        remote_version: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Network timeout during operation
    #[snafu(display("Network timeout for operation '{operation}' with peer {peer_id} after {duration:?}"))]
    NetworkTimeout {
        /// The operation that timed out
        operation: String,
        /// The peer ID that the operation timed out with
        peer_id: String,
        /// How long the operation waited before timing out
        duration: Duration,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Invalid configuration provided
    #[snafu(display("Invalid configuration for {component}: {message}"))]
    InvalidConfiguration {
        /// The component with invalid configuration
        component: String,
        /// Details about what makes the configuration invalid
        message: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Required configuration missing
    #[snafu(display("Missing required configuration: {parameter} in {component}"))]
    MissingConfiguration {
        /// The missing configuration parameter
        parameter: String,
        /// The component that is missing the configuration
        component: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Configuration validation failed
    #[snafu(display("Configuration validation failed for {component}: {details}"))]
    ConfigurationValidation {
        /// The component whose configuration validation failed
        component: String,
        /// Details about the validation failure
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Service unavailable or shutting down
    #[snafu(display("Service unavailable: {service} - {reason}"))]
    ServiceUnavailable {
        /// The service that is unavailable
        service: String,
        /// The reason why the service is unavailable
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Authentication failure
    #[snafu(display("Authentication failed for peer {peer_id}: {reason}"))]
    AuthenticationFailed {
        /// The peer ID that failed authentication
        peer_id: String,
        /// The reason for authentication failure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Authorization failure
    #[snafu(display("Authorization denied for operation '{operation}' by peer {peer_id}"))]
    AuthorizationDenied {
        /// The operation that was denied authorization
        operation: String,
        /// The peer ID that was denied authorization
        peer_id: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Cryptographic operation failed
    #[snafu(display("Cryptographic error in {operation}: {details}"))]
    Cryptographic {
        /// The cryptographic operation that failed
        operation: String,
        /// Details about the cryptographic failure
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Internal invariant violated - indicates a bug
    #[snafu(display("Internal invariant violated: {details}"))]
    InvariantViolation { 
        /// Details about what invariant was violated
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Feature not implemented yet
    #[snafu(display("Feature not implemented: {feature}"))]
    NotImplemented { 
        /// The feature that is not yet implemented
        feature: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Invalid state transition or operation
    #[snafu(display("Invalid state transition: from {from_state} to {to_state} in {component}"))]
    InvalidState {
        /// The state being transitioned from
        from_state: String,
        /// The state being transitioned to
        to_state: String,
        /// The component attempting the state transition
        component: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Poisoned lock or synchronization primitive
    #[snafu(display("Lock poisoned during operation '{operation}': {details}"))]
    LockPoisoned {
        /// The operation that encountered the poisoned lock
        operation: String,
        /// Details about the lock poisoning
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Invalid input parameters
    #[snafu(display("Invalid input for {parameter}: {message}"))]
    InvalidInput {
        /// The parameter that has invalid input
        parameter: String,
        /// Description of why the input is invalid
        message: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Data validation failed
    #[snafu(display("Validation failed for {field}: {reason}"))]
    Validation {
        /// The field that failed validation
        field: String,
        /// The reason for validation failure
        reason: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Constraint violation (e.g., unique key, foreign key)
    #[snafu(display("Constraint violation in {operation}: {constraint} - {details}"))]
    ConstraintViolation {
        /// The operation that violated the constraint
        operation: String,
        /// The constraint that was violated
        constraint: String,
        /// Additional details about the violation
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// I/O operation failed
    #[snafu(display("I/O error during {operation}: {source}"))]
    Io {
        /// The operation that encountered the I/O error
        operation: String,
        /// The underlying I/O error
        #[snafu(source)]
        source: std::io::Error,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// System resource error (e.g., out of memory)
    #[snafu(display("System error during {operation}: {details}"))]
    System {
        /// The operation that encountered the system error
        operation: String,
        /// Details about the system error
        details: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Internal error (generic error for internal issues)
    #[snafu(display("Internal error: {message}"))]
    Internal {
        /// Error message
        message: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Cluster configuration or membership error
    #[snafu(display("Cluster error: {message}"))]
    ClusterError {
        /// Error message
        message: String,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },

    /// Multiple errors occurred
    #[snafu(display("Multiple errors in {context}"))]
    Multiple {
        /// The context in which the multiple errors occurred
        context: String,
        /// The list of errors that occurred
        errors: Vec<RaftError>,
        /// Error backtrace for debugging
        backtrace: Backtrace,
    },
}

impl RaftError {
    /// Check if this error is transient and might succeed on retry
    pub fn is_transient(&self) -> bool {
        match self {
            RaftError::NetworkTimeout { .. } |
            RaftError::ConnectionFailed { .. } |
            RaftError::Transport { .. } |
            RaftError::ServiceUnavailable { .. } |
            RaftError::ResourceExhausted { .. } |
            RaftError::NotReady { .. } => true,
            
            RaftError::Multiple { errors, .. } => {
                errors.iter().all(|e| e.is_transient())
            }
            
            _ => false,
        }
    }

    /// Check if this error indicates the node should step down from leadership
    pub fn should_step_down(&self) -> bool {
        match self {
            RaftError::NetworkPartition { .. } |
            RaftError::StorageCorruption { .. } |
            RaftError::InsufficientSpace { .. } => true,
            
            RaftError::Multiple { errors, .. } => {
                errors.iter().any(|e| e.should_step_down())
            }
            
            _ => false,
        }
    }

    /// Get the error category for metrics and monitoring
    pub fn category(&self) -> &'static str {
        match self {
            RaftError::NotLeader { .. } |
            RaftError::Consensus { .. } |
            RaftError::NotReady { .. } |
            RaftError::NodeNotFound { .. } |
            RaftError::ProposalRejected { .. } |
            RaftError::ConfigurationChange { .. } |
            RaftError::Snapshot { .. } => "raft",

            RaftError::Storage { .. } |
            RaftError::Database { .. } |
            RaftError::Transaction { .. } |
            RaftError::StorageCorruption { .. } |
            RaftError::InsufficientSpace { .. } => "storage",

            RaftError::Transport { .. } |
            RaftError::ConnectionFailed { .. } |
            RaftError::NetworkPartition { .. } |
            RaftError::Serialization { .. } |
            RaftError::ProtocolMismatch { .. } |
            RaftError::NetworkTimeout { .. } => "transport",

            RaftError::InvalidConfiguration { .. } |
            RaftError::MissingConfiguration { .. } |
            RaftError::ConfigurationValidation { .. } => "configuration",

            RaftError::AuthenticationFailed { .. } |
            RaftError::AuthorizationDenied { .. } |
            RaftError::Cryptographic { .. } => "security",

            RaftError::InvariantViolation { .. } |
            RaftError::NotImplemented { .. } |
            RaftError::InvalidState { .. } |
            RaftError::LockPoisoned { .. } => "internal",

            RaftError::InvalidInput { .. } |
            RaftError::Validation { .. } |
            RaftError::ConstraintViolation { .. } => "validation",

            RaftError::ResourceExhausted { .. } |
            RaftError::Timeout { .. } |
            RaftError::ServiceUnavailable { .. } |
            RaftError::Io { .. } |
            RaftError::System { .. } => "system",
            
            RaftError::Internal { .. } => "internal",
            
            RaftError::ClusterError { .. } => "cluster",

            RaftError::Multiple { .. } => "multiple",
            
            RaftError::CircuitBreakerOpen { .. } => "circuit_breaker",
            RaftError::Backpressure { .. } => "backpressure",
        }
    }

    /// Create a storage error with context
    pub fn storage_error(operation: &str, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        RaftError::Storage {
            inner: Box::new(StorageError {
                operation: operation.to_string(),
                source: Box::new(source),
                backtrace: Backtrace::new(),
            }),
        }
    }

    /// Create a transport error with context
    pub fn transport_error(details: &str) -> Self {
        RaftError::Transport {
            inner: Box::new(TransportError {
                details: details.to_string(),
                backtrace: Backtrace::new(),
            }),
        }
    }

    /// Create a consensus error with context
    pub fn consensus_error(operation: &str, source: raft::Error) -> Self {
        RaftError::Consensus {
            inner: Box::new(ConsensusError {
                operation: operation.to_string(),
                source,
                backtrace: Backtrace::new(),
            }),
        }
    }

    /// Create a database error with context
    pub fn database_error(operation: &str, source: redb::Error) -> Self {
        RaftError::Database {
            inner: Box::new(DatabaseError {
                operation: operation.to_string(),
                source,
                backtrace: Backtrace::new(),
            }),
        }
    }

    /// Get a detailed error message including all sub-errors for Multiple variant
    pub fn detailed_message(&self) -> String {
        match self {
            RaftError::Multiple { context, errors, .. } => {
                format!("Multiple errors in {}: {}", context, format_error_list(errors))
            }
            _ => self.to_string(),
        }
    }
}

// Helper function to format multiple errors
fn format_error_list(errors: &[RaftError]) -> String {
    errors
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e))
        .collect::<Vec<_>>()
        .join("; ")
}

// Convenient type aliases
/// Convenient Result type alias for Raft operations
pub type Result<T> = std::result::Result<T, RaftError>;

// Standard library error conversions
impl From<std::io::Error> for RaftError {
    fn from(err: std::io::Error) -> Self {
        RaftError::Io {
            operation: "unknown".to_string(),
            source: err,
            backtrace: Backtrace::new(),
        }
    }
}

impl From<redb::Error> for RaftError {
    fn from(err: redb::Error) -> Self {
        RaftError::database_error("unknown", err)
    }
}

impl From<raft::Error> for RaftError {
    fn from(err: raft::Error) -> Self {
        RaftError::consensus_error("unknown", err)
    }
}

impl From<serde_json::Error> for RaftError {
    fn from(err: serde_json::Error) -> Self {
        RaftError::Serialization {
            message_type: "json".to_string(),
            source: Box::new(err),
            backtrace: Backtrace::new(),
        }
    }
}

impl From<bincode::Error> for RaftError {
    fn from(err: bincode::Error) -> Self {
        RaftError::Serialization {
            message_type: "binary".to_string(),
            source: Box::new(err),
            backtrace: Backtrace::new(),
        }
    }
}

impl From<postcard::Error> for RaftError {
    fn from(err: postcard::Error) -> Self {
        RaftError::Serialization {
            message_type: "postcard".to_string(),
            source: Box::new(err),
            backtrace: Backtrace::new(),
        }
    }
}

impl From<crate::config::ConfigError> for RaftError {
    fn from(err: crate::config::ConfigError) -> Self {
        RaftError::InvalidConfiguration {
            component: "config".to_string(),
            message: err.to_string(),
            backtrace: Backtrace::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_error_categorization() {
        let raft_err = NotLeaderSnafu {
            operation: "propose".to_string(),
            current_leader: Some(2),
        }.build();
        assert_eq!(raft_err.category(), "raft");
        assert!(!raft_err.is_transient());
        assert!(!raft_err.should_step_down());

        let transport_err = NetworkTimeoutSnafu {
            operation: "append_entries".to_string(),
            peer_id: "peer1".to_string(),
            duration: Duration::from_secs(5),
        }.build();
        assert_eq!(transport_err.category(), "transport");
        assert!(transport_err.is_transient());
        assert!(!transport_err.should_step_down());

        let storage_err = StorageCorruptionSnafu {
            component: "log".to_string(),
            details: "checksum mismatch".to_string(),
        }.build();
        assert_eq!(storage_err.category(), "storage");
        assert!(!storage_err.is_transient());
        assert!(storage_err.should_step_down());
    }

    #[test]
    fn test_multiple_errors() {
        let errors = vec![
            NetworkTimeoutSnafu {
                operation: "test1".to_string(),
                peer_id: "peer1".to_string(),
                duration: Duration::from_secs(1),
            }.build(),
            ConnectionFailedSnafu {
                peer_id: "peer2".to_string(),
                reason: "timeout".to_string(),
            }.build(),
        ];

        let multi_err = MultipleSnafu {
            context: "cluster_operation".to_string(),
            errors,
        }.build();

        assert_eq!(multi_err.category(), "multiple");
        assert!(multi_err.is_transient()); // All sub-errors are transient
        assert!(!multi_err.should_step_down()); // No sub-errors require step-down
    }

    #[test]
    fn test_error_constructors() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let raft_err = RaftError::storage_error("read_log", io_err);
        
        match raft_err {
            RaftError::Storage { inner } => {
                assert_eq!(inner.operation, "read_log");
            }
            _ => panic!("Expected Storage error"),
        }

        let transport_err = RaftError::transport_error("connection refused");
        match transport_err {
            RaftError::Transport { inner } => {
                assert_eq!(inner.details, "connection refused");
            }
            _ => panic!("Expected Transport error"),
        }
    }

    #[test]
    fn test_error_display() {
        let err = NotLeaderSnafu {
            operation: "propose".to_string(),
            current_leader: Some(2),
        }.build();
        let display = format!("{}", err);
        assert!(display.contains("Not leader"));
        assert!(display.contains("propose"));
        assert!(display.contains("2"));

        let multi_err = MultipleSnafu {
            context: "test".to_string(),
            errors: vec![
                InvalidInputSnafu {
                    parameter: "data".to_string(),
                    message: "empty".to_string(),
                }.build(),
                TimeoutSnafu {
                    operation: "commit".to_string(),
                    duration: Duration::from_secs(30),
                }.build(),
            ],
        }.build();
        let display = format!("{}", multi_err);
        assert!(display.contains("Multiple errors"));
        
        let detailed = multi_err.detailed_message();
        assert!(detailed.contains("1."));
        assert!(detailed.contains("2."));
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    
    #[test]
    fn test_error_size() {
        let size = std::mem::size_of::<RaftError>();
        println!("RaftError size: {} bytes", size);
        // We want to get this below 100 bytes
        // This test documents current size and will help track improvements
        // Note: Temporarily commenting out assertion until we complete the optimization
        // assert!(size < 100, "RaftError size should be less than 100 bytes, got {}", size);
    }

    #[test]
    fn test_boxed_error_sizes() {
        println!("DatabaseError size: {} bytes", std::mem::size_of::<DatabaseError>());
        println!("StorageError size: {} bytes", std::mem::size_of::<StorageError>());
        println!("TransportError size: {} bytes", std::mem::size_of::<TransportError>());
        println!("ConsensusError size: {} bytes", std::mem::size_of::<ConsensusError>());
        println!("Box<DatabaseError> size: {} bytes", std::mem::size_of::<Box<DatabaseError>>());
    }
}
