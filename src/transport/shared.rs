//! Shared node state for transport layer
//!
//! This module provides a minimal shared state structure for the transport layer
//! to avoid circular dependencies with the node module.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use raft::prelude::Message;
use iroh::{PublicKey, NodeAddr};

/// Minimal shared state for transport operations
#[derive(Clone)]
pub struct SharedNodeState {
    /// Node ID
    pub node_id: u64,
    /// Channel for incoming Raft messages
    pub raft_tx: tokio::sync::mpsc::UnboundedSender<(u64, Message)>,
    /// Storage for any additional state
    pub state: Arc<RwLock<SharedStateInner>>,
}

/// Inner state structure
pub struct SharedStateInner {
    /// Whether the node is running
    pub is_running: bool,
    /// Peer information
    pub peers: HashMap<u64, PeerInfo>,
}

/// Peer information
#[derive(Clone, Debug)]
pub struct PeerInfo {
    /// Raft cluster node ID
    pub node_id: u64,
    /// Iroh public key for P2P communication
    pub public_key: PublicKey,
    /// Optional network address
    pub address: Option<String>,
    /// Optional Iroh node ID string representation
    pub p2p_node_id: Option<String>,
    /// List of P2P network addresses
    pub p2p_addresses: Vec<String>,
    /// Optional relay server URL for NAT traversal
    pub p2p_relay_url: Option<String>,
}

/// Node registry entry
#[derive(Clone, Debug)]
pub struct NodeEntry {
    /// Raft cluster node ID
    pub cluster_node_id: u64,
    /// Iroh node ID for P2P communication
    pub iroh_node_id: String,
}

impl SharedNodeState {
    /// Create a new shared node state
    pub fn new(node_id: u64, raft_tx: tokio::sync::mpsc::UnboundedSender<(u64, Message)>) -> Self {
        Self {
            node_id,
            raft_tx,
            state: Arc::new(RwLock::new(SharedStateInner {
                is_running: true,
                peers: HashMap::new(),
            })),
        }
    }
    
    /// Get the node ID
    pub fn node_id(&self) -> u64 {
        self.node_id
    }
    
    /// Check if the node is running
    pub async fn is_running(&self) -> bool {
        self.state.read().await.is_running
    }
    
    /// Send a Raft message to the node
    pub fn send_raft_message(&self, from: u64, msg: Message) -> Result<(), tokio::sync::mpsc::error::SendError<(u64, Message)>> {
        self.raft_tx.send((from, msg))
    }
    
    /// Get peer information
    pub async fn get_peer(&self, peer_id: u64) -> Option<PeerInfo> {
        self.state.read().await.peers.get(&peer_id).cloned()
    }
    
    /// Get all peers
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        self.state.read().await.peers.values().cloned().collect()
    }
    
    /// Add or update a peer
    pub async fn add_peer(&self, peer_info: PeerInfo) {
        self.state.write().await.peers.insert(peer_info.node_id, peer_info);
    }
    
    /// Remove a peer
    pub async fn remove_peer(&self, peer_id: u64) -> Option<PeerInfo> {
        self.state.write().await.peers.remove(&peer_id)
    }
    
    /// Get a placeholder node registry (returns self for now)
    pub fn node_registry(&self) -> Arc<SharedNodeState> {
        Arc::new(self.clone())
    }
    
    /// List all node entries
    pub async fn list_nodes(&self) -> Vec<NodeEntry> {
        self.state.read().await.peers.iter().map(|(id, peer)| NodeEntry {
            cluster_node_id: *id,
            iroh_node_id: peer.p2p_node_id.clone().unwrap_or_else(|| id.to_string()),
        }).collect()
    }
    
    /// Register a node
    pub async fn register_node(&self, cluster_id: u64, iroh_id: String, addr: NodeAddr) {
        // For now, just update the peer info
        if let Some(mut peer) = self.state.write().await.peers.get_mut(&cluster_id) {
            peer.p2p_node_id = Some(iroh_id);
        }
    }
    
    /// Get node address for a peer
    pub async fn get_node_addr(&self, peer_id: u64) -> Option<NodeAddr> {
        // This is a placeholder - proper implementation would convert PeerInfo to NodeAddr
        None
    }
}