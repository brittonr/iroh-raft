//! # iroh-raft
//!
//! A distributed consensus library combining:
//! - **Raft consensus algorithm** for distributed agreement
//! - **Iroh P2P networking** for secure, encrypted peer-to-peer communication
//! - **Redb storage backend** for persistent state management
//!
//! ## Features
//!
//! - Production-ready Raft implementation with leader election and log replication
//! - Built-in P2P transport with QUIC encryption via Iroh
//! - Persistent storage with efficient snapshots and log compaction
//! - Pluggable state machine interface for custom applications
//! - Comprehensive metrics and observability support
//!
//! ## Example
//!
//! ```rust,no_run
//! use iroh_raft::{ConfigBuilder, ConfigError};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), ConfigError> {
//!     // Create configuration using the builder pattern
//!     let config = ConfigBuilder::new()
//!         .node_id(1)
//!         .data_dir("/tmp/raft-node-1")
//!         .bind_address("127.0.0.1:8080")
//!         .add_peer("127.0.0.1:8081")
//!         .election_timeout(Duration::from_millis(5000))
//!         .heartbeat_interval(Duration::from_millis(1000))
//!         .log_level("info")
//!         .build()?;
//!
//!     println!("Configuration created: node_id={:?}", config.node.id);
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![deny(unsafe_code)]

// Core modules
pub mod config;
pub mod error;
pub mod types;

// Core modules
pub mod storage;
/// P2P transport implementations for Raft consensus
/// 
/// This module provides transport layers for Raft consensus communication
/// over peer-to-peer networks, primarily using the Iroh networking stack.
pub mod transport;

pub mod cluster;
pub mod cluster_api;
pub mod admin;

// Other modules are under development and will be added incrementally
// pub mod node;
pub mod raft;
// pub mod discovery;

// Performance and reliability modules
pub mod actor_patterns;
pub mod backpressure;
pub mod circuit_breaker;
pub mod timeout_manager;
pub mod lock_free;

// Management modules (Iroh-native P2P)
#[cfg(feature = "management-api")]
pub mod management;

// Optional modules
#[cfg(feature = "metrics-otel")]
pub mod metrics;

#[cfg(feature = "test-helpers")]
pub mod test_helpers;

// Re-export key types from dependencies
pub use raft as raft_lib;
pub use raft_proto;
pub use redb;

// Public API exports
pub use crate::config::{Config, ConfigBuilder, ConfigError, ConfigResult};
pub use crate::error::{RaftError as Error, Result};
pub use crate::types::{NodeId, ProposalData};

// High-level cluster API exports
pub use crate::cluster::{
    RaftCluster, ClusterStatus, ClusterInfo, HealthStatus, Health, ClusterMetrics,
    MemberConfig, MemberInfo, NodeRole, AdminOps, ClusterStateMachine
};

// Cluster API exports (simplified high-level interface)
pub use crate::cluster_api::{
    // StateMachine as ClusterStateMachine, ClusterBuilder, ClusterHealth,
    HealthCheck, CheckStatus, NodeInfo, NodeStatus, NodeState, OperationResult
};

// Administrative operations exports
pub use crate::admin::{
    AdminOps as AdminOperations, HealthChecker, MaintenanceOps,
    ClusterMonitor, MaintenanceNeeded
};

// State machine API exports
pub use crate::raft::{
    ExampleRaftStateMachine, GenericRaftStateMachine, KvCommand, KvState, KeyValueStore,
    StateMachine
};

// Core transport and storage exports
pub use crate::storage::RaftStorage;
pub use crate::transport::RaftProtocolHandler;

// Feature-gated exports
#[cfg(feature = "test-helpers")]
pub use crate::test_helpers::{TestCluster, TestNode, TempDir, NodeStatus as TestNodeStatus, ClusterStatus as TestClusterStatus};

#[cfg(feature = "metrics-otel")]
pub use crate::metrics::{MetricsRegistry, LatencyTimer};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::config::{Config, ConfigBuilder, ConfigError, ConfigResult};
    pub use crate::error::{RaftError as Error, Result};
    pub use crate::types::{NodeId, ProposalData};

    // High-level cluster API
    pub use crate::cluster::{
        RaftCluster, ClusterStatus, ClusterInfo, HealthStatus, Health,
        MemberConfig, NodeRole, AdminOps
    };

    // State machine API
    pub use crate::raft::{
        StateMachine, KeyValueStore, KvCommand, KvQuery, KvQueryResponse
    };

    // Administrative operations
    pub use crate::admin::{
        AdminOps as AdminOperations, HealthChecker, MaintenanceOps
    };

    // Re-export key Raft types
    pub use raft::prelude::*;
}
