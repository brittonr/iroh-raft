//! Simple Iroh transport implementation for Raft
//!
//! This module provides a simplified transport layer that handles
//! P2P communication for Raft messages using Iroh networking.

use crate::error::{RaftError as Error, Result};
use crate::types::NodeConfig;

use iroh::{Endpoint, NodeId as IrohNodeId};
use raft::prelude::Message;
// use serde::{Deserialize, Serialize}; // Not needed for simplified version
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// Message envelope for transport
#[derive(Debug, Clone)]
pub enum TransportMessage {
    /// Raft protocol message
    Raft { 
        /// Source node ID
        from: u64, 
        /// Raft consensus message
        message: Message 
    },
}

/// Simplified Iroh transport for Raft messages
pub struct IrohTransport {
    /// Local node configuration
    config: NodeConfig,
    
    /// Iroh endpoint for P2P communication
    endpoint: Option<Endpoint>,
    
    /// Known peers mapping node_id -> iroh_node_id
    peers: Arc<RwLock<HashMap<u64, IrohNodeId>>>,
    
    /// Message sender for outgoing messages
    outgoing_tx: mpsc::UnboundedSender<(u64, Message)>,
    
    /// Message receiver for outgoing messages
    outgoing_rx: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<(u64, Message)>>>>,
    
    /// Running flag
    running: Arc<RwLock<bool>>,
}

impl IrohTransport {
    /// Create a new Iroh transport
    pub async fn new(config: &NodeConfig) -> Result<Self> {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        
        Ok(Self {
            config: config.clone(),
            endpoint: None,
            peers: Arc::new(RwLock::new(HashMap::new())),
            outgoing_tx,
            outgoing_rx: Arc::new(tokio::sync::Mutex::new(Some(outgoing_rx))),
            running: Arc::new(RwLock::new(false)),
        })
    }
    
    /// Start the transport
    pub async fn start(&self) -> Result<()> {
        info!("Starting Iroh transport for node {:?}", self.config.id);
        
        // Create Iroh endpoint with a random secret key
        let secret_key = iroh::SecretKey::generate(&mut rand::thread_rng());
        let endpoint = Endpoint::builder()
            .secret_key(secret_key)
            .discovery_n0()
            .bind()
            .await
            .map_err(|e| Error::transport_error(&format!("create iroh endpoint: {}", e)))?;
        
        // Store the endpoint
        {
            // This is a simplified approach - in a real implementation you'd need
            // proper mutable access to the endpoint
            warn!("Iroh transport started but endpoint storage not implemented");
        }
        
        // Initialize known peers
        // TODO: Add peer configuration to NodeConfig
        // let mut peers = self.peers.write().await;
        // for peer in &self.config.peers {
        //     // For now, generate a dummy Iroh node ID from the peer ID
        //     // In a real implementation, you'd discover or configure these properly
        //     let iroh_id = IrohNodeId::from([peer.id as u8; 32]);
        //     peers.insert(peer.id, iroh_id);
        // }
        
        // Mark as running
        *self.running.write().await = true;
        
        info!("Iroh transport started successfully");
        Ok(())
    }
    
    /// Stop the transport
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping Iroh transport for node {:?}", self.config.id);
        
        // Mark as not running
        *self.running.write().await = false;
        
        // TODO: Close endpoint and connections
        
        info!("Iroh transport stopped");
        Ok(())
    }
    
    /// Send a Raft message to a peer
    pub async fn send_message(&self, message: Message) -> Result<()> {
        let to = message.to;
        
        // Check if we're running
        if !*self.running.read().await {
            warn!("Attempt to send message while transport is stopped");
            return Ok(()); // Silently ignore for now
        }
        
        // Check if we know this peer
        let peers = self.peers.read().await;
        if !peers.contains_key(&to) {
            warn!("Unknown peer {}, cannot send message", to);
            return Ok(()); // Silently ignore unknown peers
        }
        
        // For now, just log the message - in a real implementation
        // this would serialize and send over Iroh
        debug!("Sending Raft message to peer {}: type={:?}", to, message.msg_type());
        
        // TODO: Implement actual message sending via Iroh
        // This would involve:
        // 1. Serializing the message
        // 2. Opening a connection to the peer
        // 3. Sending the serialized data
        // 4. Handling connection failures
        
        Ok(())
    }
    
    /// Add a new peer
    pub async fn add_peer(&self, node_id: u64, iroh_id: IrohNodeId) -> Result<()> {
        info!("Adding peer {} with Iroh ID {}", node_id, iroh_id);
        
        let mut peers = self.peers.write().await;
        peers.insert(node_id, iroh_id);
        
        Ok(())
    }
    
    /// Remove a peer
    pub async fn remove_peer(&self, node_id: u64) -> Result<()> {
        info!("Removing peer {}", node_id);
        
        let mut peers = self.peers.write().await;
        peers.remove(&node_id);
        
        Ok(())
    }
    
    /// Get the list of connected peers
    pub async fn connected_peers(&self) -> Vec<u64> {
        self.peers.read().await.keys().copied().collect()
    }
    
    /// Check if transport is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

/// Simple mock transport for testing
#[cfg(feature = "test-helpers")]
pub struct MockTransport {
    node_id: u64,
    sent_messages: Arc<RwLock<Vec<Message>>>,
    message_handler: Option<Box<dyn Fn(Message) -> Result<()> + Send + Sync>>,
}

#[cfg(feature = "test-helpers")]
impl MockTransport {
    /// Create a new mock transport
    pub fn new(node_id: u64) -> Self {
        Self {
            node_id,
            sent_messages: Arc::new(RwLock::new(Vec::new())),
            message_handler: None,
        }
    }
    
    /// Set a message handler for incoming messages
    pub fn with_message_handler<F>(mut self, handler: F) -> Self 
    where 
        F: Fn(Message) -> Result<()> + Send + Sync + 'static 
    {
        self.message_handler = Some(Box::new(handler));
        self
    }
    
    /// Send a message (stores it for inspection)
    pub async fn send_message(&self, message: Message) -> Result<()> {
        self.sent_messages.write().await.push(message);
        Ok(())
    }
    
    /// Get sent messages for testing
    pub async fn sent_messages(&self) -> Vec<Message> {
        self.sent_messages.read().await.clone()
    }
    
    /// Clear sent messages
    pub async fn clear_sent_messages(&self) {
        self.sent_messages.write().await.clear();
    }
    
    /// Simulate receiving a message
    pub async fn receive_message(&self, message: Message) -> Result<()> {
        if let Some(ref handler) = self.message_handler {
            handler(message)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigBuilder};

    #[tokio::test]
    async fn test_transport_creation() -> Result<()> {
        let config = ConfigBuilder::new()
            .node_id(1)
            .data_dir("/tmp/test-node-1")
            .build()?;

        let transport = IrohTransport::new(&config.node).await?;
        assert!(!transport.is_running().await);

        Ok(())
    }

    #[tokio::test]
    async fn test_transport_lifecycle() -> Result<()> {
        let config = ConfigBuilder::new()
            .node_id(1)
            .data_dir("/tmp/test-node-1")
            .build()?;

        let transport = IrohTransport::new(&config.node).await?;
        
        // Start transport
        transport.start().await?;
        assert!(transport.is_running().await);
        
        // Add a peer manually for testing
        let secret_key = iroh::SecretKey::generate(&mut rand::thread_rng());
        let peer_iroh_id = secret_key.public();
        let result = transport.add_peer(2, peer_iroh_id).await;
        assert!(result.is_ok(), "add_peer failed: {:?}", result);
        
        // Check peers
        let peers = transport.connected_peers().await;
        println!("Connected peers: {:?}", peers);
        assert_eq!(peers.len(), 1, "Expected 1 peer, found {}: {:?}", peers.len(), peers);
        assert!(peers.contains(&2));
        
        // Stop transport
        transport.stop().await?;
        assert!(!transport.is_running().await);

        Ok(())
    }

    #[cfg(feature = "test-helpers")]
    #[tokio::test]
    async fn test_mock_transport() -> Result<()> {
        let transport = MockTransport::new(1);
        
        // Create a test message
        let mut message = Message::default();
        message.set_from(1);
        message.set_to(2);
        message.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
        
        // Send message
        transport.send_message(message.clone()).await?;
        
        // Check sent messages
        let sent = transport.sent_messages().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].from, 1);
        assert_eq!(sent[0].to, 2);
        
        Ok(())
    }
}