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
//! use iroh_raft::error::{RaftError, Result};
//!
//! fn example_operation() -> Result<String> {
//!     // This would be a real Raft operation
//!     Err(RaftError::NotLeader { 
//!         operation: "propose_entry".to_string(),
//!         current_leader: Some(2)
//!     })
//! }
//! ```

use std::time::Duration;
use thiserror::Error;

/// Comprehensive error type for iroh-raft consensus operations
///
/// This error type covers all failure modes in the distributed consensus system,
/// from low-level storage operations to high-level Raft protocol violations.
#[derive(Error, Debug)]
pub enum RaftError {
    // === Raft Protocol Errors ===
    
    /// Node is not the current leader for write operations
    #[error("Not leader for operation '{operation}', current leader: {current_leader:?}")]
    NotLeader {
        /// The operation that was attempted on a non-leader
        operation: String,
        /// The current leader node ID, if known
        current_leader: Option<u64>,
    },

    /// Raft consensus error from the underlying raft library
    #[error("Raft consensus error in '{operation}': {source}")]
    Consensus {
        /// The operation that triggered the consensus error
        operation: String,
        /// The underlying Raft library error
        #[source]
        source: raft::Error,
    },

    /// Node is not initialized or ready for operations
    #[error("Node {node_id} not ready for operation '{operation}': {reason}")]
    NotReady {
        /// The ID of the node that is not ready
        node_id: u64,
        /// The operation that was attempted
        operation: String,
        /// The reason why the node is not ready
        reason: String,
    },

    /// Invalid node ID or node not found in cluster configuration
    #[error("Node {node_id} not found in cluster configuration")]
    NodeNotFound { 
        /// The node ID that was not found
        node_id: u64 
    },

    /// Proposal was rejected by the Raft state machine
    #[error("Proposal rejected for operation '{operation}': {reason}")]
    ProposalRejected {
        /// The operation that was rejected
        operation: String,
        /// The reason for rejection
        reason: String,
    },

    /// Configuration change failed (add/remove node)
    #[error("Configuration change failed: {operation} - {reason}")]
    ConfigurationChange {
        /// The configuration change operation that failed
        operation: String,
        /// The reason for the failure
        reason: String,
    },

    /// Snapshot operation failed
    #[error("Snapshot {operation} failed: {details}")]
    Snapshot {
        /// The snapshot operation that failed (create, apply, restore)
        operation: String,
        /// Additional details about the failure
        details: String,
    },

    // === Storage Errors ===

    /// Database operation failed
    #[error("Storage operation '{operation}' failed")]
    Storage {
        /// The storage operation that failed
        operation: String,
        /// The underlying error that caused the failure
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Redb database error
    #[error("Database error in '{operation}': {source}")]
    Database {
        /// The database operation that failed
        operation: String,
        /// The underlying redb error
        #[source]
        source: redb::Error,
    },

    /// Storage transaction failed
    #[error("Transaction '{operation}' failed: {reason}")]
    Transaction {
        /// The transaction operation that failed
        operation: String,
        /// The reason for transaction failure
        reason: String,
    },

    /// Storage corruption or inconsistency detected
    #[error("Storage corruption detected in {component}: {details}")]
    StorageCorruption {
        /// The storage component where corruption was detected
        component: String,
        /// Details about the corruption
        details: String,
    },

    /// Insufficient disk space for storage operations
    #[error("Insufficient storage space for operation '{operation}': need {needed} bytes, available {available} bytes")]
    InsufficientSpace {
        /// The operation that needs more space
        operation: String,
        /// Number of bytes needed
        needed: u64,
        /// Number of bytes available
        available: u64,
    },

    // === Transport/Networking Errors ===

    /// P2P transport error
    #[error("P2P transport error: {details}")]
    Transport { 
        /// Details about the transport error
        details: String 
    },

    /// Failed to establish connection to peer
    #[error("Connection failed to peer {peer_id}: {reason}")]
    ConnectionFailed {
        /// The peer ID that couldn't be connected to
        peer_id: String,
        /// The reason for connection failure
        reason: String,
    },

    /// Network partition or connectivity issues
    #[error("Network partition detected: {details}")]
    NetworkPartition { 
        /// Details about the network partition
        details: String 
    },

    /// Message serialization/deserialization failed
    #[error("Serialization error for {message_type}: {source}")]
    Serialization {
        /// The type of message that failed to serialize/deserialize
        message_type: String,
        /// The underlying serialization error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Protocol version mismatch between nodes
    #[error("Protocol version mismatch with peer {peer_id}: local={local_version}, remote={remote_version}")]
    ProtocolMismatch {
        /// The peer ID with the mismatched protocol version
        peer_id: String,
        /// This node's protocol version
        local_version: String,
        /// The remote node's protocol version
        remote_version: String,
    },

    /// Network timeout during operation
    #[error("Network timeout for operation '{operation}' with peer {peer_id} after {duration:?}")]
    NetworkTimeout {
        /// The operation that timed out
        operation: String,
        /// The peer ID that the operation timed out with
        peer_id: String,
        /// How long the operation waited before timing out
        duration: Duration,
    },

    // === Configuration Errors ===

    /// Invalid configuration provided
    #[error("Invalid configuration for {component}: {message}")]
    InvalidConfiguration {
        /// The component with invalid configuration
        component: String,
        /// Details about what makes the configuration invalid
        message: String,
    },

    /// Required configuration missing
    #[error("Missing required configuration: {parameter} in {component}")]
    MissingConfiguration {
        /// The missing configuration parameter
        parameter: String,
        /// The component that is missing the configuration
        component: String,
    },

    /// Configuration validation failed
    #[error("Configuration validation failed for {component}: {details}")]
    ConfigurationValidation {
        /// The component whose configuration validation failed
        component: String,
        /// Details about the validation failure
        details: String,
    },

    // === Resource Management Errors ===

    /// Resource exhaustion (memory, file descriptors, etc.)
    #[error("Resource exhausted: {resource_type} - {details}")]
    ResourceExhausted {
        /// The type of resource that was exhausted
        resource_type: String,
        /// Additional details about the exhaustion
        details: String,
    },

    /// Operation timeout
    #[error("Operation '{operation}' timed out after {duration:?}")]
    Timeout {
        /// The operation that timed out
        operation: String,
        /// How long the operation waited before timing out
        duration: Duration,
    },

    /// Service unavailable or shutting down
    #[error("Service unavailable: {service} - {reason}")]
    ServiceUnavailable {
        /// The service that is unavailable
        service: String,
        /// The reason why the service is unavailable
        reason: String,
    },

    // === Security Errors ===

    /// Authentication failure
    #[error("Authentication failed for peer {peer_id}: {reason}")]
    AuthenticationFailed {
        /// The peer ID that failed authentication
        peer_id: String,
        /// The reason for authentication failure
        reason: String,
    },

    /// Authorization failure
    #[error("Authorization denied for operation '{operation}' by peer {peer_id}")]
    AuthorizationDenied {
        /// The operation that was denied authorization
        operation: String,
        /// The peer ID that was denied authorization
        peer_id: String,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error in {operation}: {details}")]
    Cryptographic {
        /// The cryptographic operation that failed
        operation: String,
        /// Details about the cryptographic failure
        details: String,
    },

    // === Internal/Programming Errors ===

    /// Internal invariant violated - indicates a bug
    #[error("Internal invariant violated: {details}")]
    InvariantViolation { 
        /// Details about what invariant was violated
        details: String 
    },

    /// Feature not implemented yet
    #[error("Feature not implemented: {feature}")]
    NotImplemented { 
        /// The feature that is not yet implemented
        feature: String 
    },

    /// Invalid state transition or operation
    #[error("Invalid state transition: from {from_state} to {to_state} in {component}")]
    InvalidState {
        /// The state being transitioned from
        from_state: String,
        /// The state being transitioned to
        to_state: String,
        /// The component attempting the state transition
        component: String,
    },

    /// Poisoned lock or synchronization primitive
    #[error("Lock poisoned during operation '{operation}': {details}")]
    LockPoisoned {
        /// The operation that encountered the poisoned lock
        operation: String,
        /// Details about the lock poisoning
        details: String,
    },

    // === Input/Validation Errors ===

    /// Invalid input parameters
    #[error("Invalid input for {parameter}: {message}")]
    InvalidInput {
        /// The parameter that has invalid input
        parameter: String,
        /// Description of why the input is invalid
        message: String,
    },

    /// Data validation failed
    #[error("Validation failed for {field}: {reason}")]
    Validation {
        /// The field that failed validation
        field: String,
        /// The reason for validation failure
        reason: String,
    },

    /// Constraint violation (e.g., unique key, foreign key)
    #[error("Constraint violation in {operation}: {constraint} - {details}")]
    ConstraintViolation {
        /// The operation that violated the constraint
        operation: String,
        /// The constraint that was violated
        constraint: String,
        /// Additional details about the violation
        details: String,
    },

    // === General System Errors ===

    /// I/O operation failed
    #[error("I/O error during {operation}: {source}")]
    Io {
        /// The operation that encountered the I/O error
        operation: String,
        /// The underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// System resource error (e.g., out of memory)
    #[error("System error during {operation}: {details}")]
    System {
        /// The operation that encountered the system error
        operation: String,
        /// Details about the system error
        details: String,
    },

    /// Multiple errors occurred
    #[error("Multiple errors in {context}: {}", format_error_list(.errors))]
    Multiple {
        /// The context in which the multiple errors occurred
        context: String,
        /// The list of errors that occurred
        errors: Vec<RaftError>,
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

            RaftError::Multiple { .. } => "multiple",
        }
    }

    /// Create a storage error with context
    pub fn storage_error(operation: &str, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        RaftError::Storage {
            operation: operation.to_string(),
            source: Box::new(source),
        }
    }

    /// Create a transport error with context
    pub fn transport_error(details: &str) -> Self {
        RaftError::Transport {
            details: details.to_string(),
        }
    }

    /// Create a consensus error with context
    pub fn consensus_error(operation: &str, source: raft::Error) -> Self {
        RaftError::Consensus {
            operation: operation.to_string(),
            source,
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
        }
    }
}

impl From<redb::Error> for RaftError {
    fn from(err: redb::Error) -> Self {
        RaftError::Database {
            operation: "unknown".to_string(),
            source: err,
        }
    }
}

impl From<raft::Error> for RaftError {
    fn from(err: raft::Error) -> Self {
        RaftError::Consensus {
            operation: "unknown".to_string(),
            source: err,
        }
    }
}

impl From<serde_json::Error> for RaftError {
    fn from(err: serde_json::Error) -> Self {
        RaftError::Serialization {
            message_type: "json".to_string(),
            source: Box::new(err),
        }
    }
}

impl From<bincode::Error> for RaftError {
    fn from(err: bincode::Error) -> Self {
        RaftError::Serialization {
            message_type: "binary".to_string(),
            source: Box::new(err),
        }
    }
}

impl From<postcard::Error> for RaftError {
    fn from(err: postcard::Error) -> Self {
        RaftError::Serialization {
            message_type: "postcard".to_string(),
            source: Box::new(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_error_categorization() {
        let raft_err = RaftError::NotLeader {
            operation: "propose".to_string(),
            current_leader: Some(2),
        };
        assert_eq!(raft_err.category(), "raft");
        assert!(!raft_err.is_transient());
        assert!(!raft_err.should_step_down());

        let transport_err = RaftError::NetworkTimeout {
            operation: "append_entries".to_string(),
            peer_id: "peer1".to_string(),
            duration: Duration::from_secs(5),
        };
        assert_eq!(transport_err.category(), "transport");
        assert!(transport_err.is_transient());
        assert!(!transport_err.should_step_down());

        let storage_err = RaftError::StorageCorruption {
            component: "log".to_string(),
            details: "checksum mismatch".to_string(),
        };
        assert_eq!(storage_err.category(), "storage");
        assert!(!storage_err.is_transient());
        assert!(storage_err.should_step_down());
    }

    #[test]
    fn test_multiple_errors() {
        let errors = vec![
            RaftError::NetworkTimeout {
                operation: "test1".to_string(),
                peer_id: "peer1".to_string(),
                duration: Duration::from_secs(1),
            },
            RaftError::ConnectionFailed {
                peer_id: "peer2".to_string(),
                reason: "timeout".to_string(),
            },
        ];

        let multi_err = RaftError::Multiple {
            context: "cluster_operation".to_string(),
            errors,
        };

        assert_eq!(multi_err.category(), "multiple");
        assert!(multi_err.is_transient()); // All sub-errors are transient
        assert!(!multi_err.should_step_down()); // No sub-errors require step-down
    }

    #[test]
    fn test_error_constructors() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let raft_err = RaftError::storage_error("read_log", io_err);
        
        match raft_err {
            RaftError::Storage { operation, .. } => {
                assert_eq!(operation, "read_log");
            }
            _ => panic!("Expected Storage error"),
        }

        let transport_err = RaftError::transport_error("connection refused");
        match transport_err {
            RaftError::Transport { details } => {
                assert_eq!(details, "connection refused");
            }
            _ => panic!("Expected Transport error"),
        }
    }

    #[test]
    fn test_error_display() {
        let err = RaftError::NotLeader {
            operation: "propose".to_string(),
            current_leader: Some(2),
        };
        let display = format!("{}", err);
        assert!(display.contains("Not leader"));
        assert!(display.contains("propose"));
        assert!(display.contains("2"));

        let multi_err = RaftError::Multiple {
            context: "test".to_string(),
            errors: vec![
                RaftError::InvalidInput {
                    parameter: "data".to_string(),
                    message: "empty".to_string(),
                },
                RaftError::Timeout {
                    operation: "commit".to_string(),
                    duration: Duration::from_secs(30),
                },
            ],
        };
        let display = format!("{}", multi_err);
        assert!(display.contains("Multiple errors"));
        assert!(display.contains("1."));
        assert!(display.contains("2."));
    }
}