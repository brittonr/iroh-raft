// pub mod adapter;  // Requires node_shared module
// pub mod endpoint_manager;  // May have dependencies
pub mod iroh;
pub mod protocol;
// pub mod service;  // Requires node_shared module
pub mod shared;
pub mod simple;

pub use protocol::{RAFT_ALPN, RaftProtocolHandler};
pub use simple::*;

/// Legacy ALPN constant for backward compatibility with older Blixard protocol implementations
pub const BLIXARD_ALPN: &[u8] = RAFT_ALPN;