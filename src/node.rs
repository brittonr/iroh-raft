//! High-level Raft node implementation
//!
//! This module provides the main RaftNode struct that users interact with to build
//! distributed systems using Raft consensus, Iroh P2P transport, and Redb storage.

use crate::error::{RaftError as Error, Result};
use crate::storage::RaftStorage;
use crate::transport::{IrohTransport, TransportMessage};
use crate::types::{NodeConfig, NodeId, NodeRole, NodeStatus, ProposalData};

use async_trait::async_trait;
use parking_lot::RwLock;
use raft::prelude::{Config as RaftConfig, Message, RawNode, Snapshot, SnapshotMetadata};
use raft::{GetEntriesContext, StateRole, Storage};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, instrument, warn};

/// High-level interface for custom state machines used by RaftNode
/// 
/// NOTE: This is different from the generic StateMachine trait in the raft module.
/// This trait is specific to the RaftNode implementation and works with ProposalData.
#[async_trait]
pub trait NodeStateMachine: Send + Sync + 'static {
    /// Apply a proposal to the state machine
    async fn apply(&mut self, index: u64, data: &ProposalData) -> Result<Vec<u8>>;
    
    /// Take a snapshot of the current state
    async fn snapshot(&self) -> Result<Vec<u8>>;
    
    /// Restore state from a snapshot
    async fn restore(&mut self, data: &[u8]) -> Result<()>;
}

/// Simple key-value state machine implementation
pub struct KeyValueStateMachine {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl KeyValueStateMachine {
    /// Create a new key-value state machine
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.read().get(key).cloned()
    }
    
    /// Get all keys with optional prefix
    pub fn list(&self, prefix: Option<&str>) -> Vec<String> {
        let data = self.data.read();
        if let Some(prefix) = prefix {
            data.keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            data.keys().cloned().collect()
        }
    }
    
    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.data.read().len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.read().is_empty()
    }
}

#[async_trait]
impl NodeStateMachine for KeyValueStateMachine {
    async fn apply(&mut self, _index: u64, data: &ProposalData) -> Result<Vec<u8>> {
        match data {
            ProposalData::KeyValue(op) => {
                use crate::types::KeyValueOp;
                let mut store = self.data.write();
                
                match op {
                    KeyValueOp::Set { key, value } => {
                        store.insert(key.clone(), value.clone());
                        Ok(b"ok".to_vec())
                    }
                    KeyValueOp::Delete { key } => {
                        let existed = store.remove(key).is_some();
                        Ok(if existed { b"deleted" } else { b"not_found" }.to_vec())
                    }
                    KeyValueOp::Get { key } => {
                        let value = store.get(key).cloned().unwrap_or_default();
                        Ok(value.into_bytes())
                    }
                    KeyValueOp::List { prefix } => {
                        let keys: Vec<String> = if let Some(prefix) = prefix {
                            store.keys()
                                .filter(|k| k.starts_with(prefix))
                                .cloned()
                                .collect()
                        } else {
                            store.keys().cloned().collect()
                        };
                        Ok(keys.join("\n").into_bytes())
                    }
                }
            }
            _ => {
                // For other proposal types, just acknowledge
                Ok(b"ack".to_vec())
            }
        }
    }
    
    async fn snapshot(&self) -> Result<Vec<u8>> {
        let data = self.data.read();
        bincode::serialize(&*data).map_err(|e| Error::serialization("snapshot state machine", e))
    }
    
    async fn restore(&mut self, data: &[u8]) -> Result<()> {
        let restored: HashMap<String, String> = bincode::deserialize(data)
            .map_err(|e| Error::serialization("restore state machine", e))?;
        
        *self.data.write() = restored;
        Ok(())
    }
}

impl Default for KeyValueStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating RaftNode instances
pub struct RaftNodeBuilder<S> {
    config: NodeConfig,
    state_machine: Option<S>,
}

impl RaftNodeBuilder<KeyValueStateMachine> {
    /// Create a new builder with default key-value state machine
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            state_machine: None,
        }
    }
}

impl<S: NodeStateMachine> RaftNodeBuilder<S> {
    /// Create a builder with a custom state machine
    pub fn with_state_machine(config: NodeConfig, state_machine: S) -> RaftNodeBuilder<S> {
        RaftNodeBuilder {
            config,
            state_machine: Some(state_machine),
        }
    }
    
    /// Build the RaftNode
    pub async fn build(mut self) -> Result<RaftNode<S>>
    where
        S: Default,
    {
        let state_machine = self.state_machine.take().unwrap_or_default();
        RaftNode::new(self.config, state_machine).await
    }
}

/// Main Raft node that coordinates consensus, transport, and storage
pub struct RaftNode<S: NodeStateMachine> {
    /// Node configuration
    config: NodeConfig,
    
    /// Raft consensus core
    raft: Arc<Mutex<RawNode<RaftStorage>>>,
    
    /// Transport layer for P2P communication
    transport: Arc<IrohTransport>,
    
    /// Storage backend
    storage: Arc<RaftStorage>,
    
    /// Application state machine
    state_machine: Arc<Mutex<S>>,
    
    /// Current node status
    status: Arc<RwLock<NodeStatus>>,
    
    /// Start time for uptime calculation
    start_time: Instant,
    
    /// Message channels
    message_tx: mpsc::UnboundedSender<TransportMessage>,
    message_rx: Arc<Mutex<mpsc::UnboundedReceiver<TransportMessage>>>,
    
    /// Proposal responses
    pending_proposals: Arc<RwLock<HashMap<Vec<u8>, oneshot::Sender<Result<Vec<u8>>>>>>,
    
    /// Shutdown signal
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    shutdown_rx: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

impl<S: NodeStateMachine> RaftNode<S> {
    /// Create a new RaftNode with the given configuration and state machine
    pub async fn new(config: NodeConfig, state_machine: S) -> Result<Self> {
        // Validate configuration
        config.validate()?;
        
        // Create storage
        let storage = Arc::new(
            RaftStorage::new(&config.data_dir)
                .await
                .map_err(|e| Error::storage("create storage", e))?
        );
        
        // Create transport
        let transport = Arc::new(
            IrohTransport::new(&config)
                .await
                .map_err(|e| Error::transport("create transport", e))?
        );
        
        // Create Raft configuration
        let raft_config = RaftConfig {
            id: config.id,
            election_tick: (config.raft.election_timeout_ms / config.raft.heartbeat_interval_ms) as usize,
            heartbeat_tick: 1,
            applied: storage.applied_index()?,
            max_size_per_msg: config.raft.max_bytes_per_msg as u64,
            max_inflight_msgs: config.raft.max_entries_per_msg,
            ..Default::default()
        };
        
        // Create logger for Raft
        let logger = slog::Logger::root(slog::Discard, slog::o!());
        
        // Create Raft node
        let raft_node = RawNode::new(&raft_config, storage.clone(), &logger)
            .map_err(|e| Error::raft("create raft node", e))?;
        
        // Create message channel
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        // Initialize status
        let status = NodeStatus {
            node_id: config.id,
            role: NodeRole::Follower,
            term: 0,
            leader_id: None,
            last_log_index: 0,
            committed_index: 0,
            applied_index: 0,
            connected_peers: Vec::new(),
            uptime_seconds: 0,
        };
        
        let node = Self {
            config,
            raft: Arc::new(Mutex::new(raft_node)),
            transport,
            storage,
            state_machine: Arc::new(Mutex::new(state_machine)),
            status: Arc::new(RwLock::new(status)),
            start_time: Instant::now(),
            message_tx,
            message_rx: Arc::new(Mutex::new(message_rx)),
            pending_proposals: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
            shutdown_rx: Arc::new(Mutex::new(Some(shutdown_rx))),
        };
        
        Ok(node)
    }
    
    /// Start the Raft node
    #[instrument(skip(self), fields(node_id = self.config.id))]
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Raft node {}", self.config.id);
        
        // Start transport
        self.transport.start().await?;
        
        // If this is a bootstrap node, start as single-node cluster
        if self.config.bootstrap {
            self.bootstrap().await?;
        } else if let Some(join_addr) = &self.config.join_address {
            // Join existing cluster
            self.join_cluster(join_addr).await?;
        }
        
        // Start main loop
        let node = Arc::new(self.clone_for_task());
        tokio::spawn(Self::run_main_loop(node));
        
        Ok(())
    }
    
    /// Stop the Raft node
    #[instrument(skip(self), fields(node_id = self.config.id))]
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping Raft node {}", self.config.id);
        
        // Send shutdown signal
        if let Some(shutdown_tx) = self.shutdown_tx.lock().await.take() {
            let _ = shutdown_tx.send(());
        }
        
        // Stop transport
        self.transport.stop().await?;
        
        info!("Raft node {} stopped", self.config.id);
        Ok(())
    }
    
    /// Submit a proposal to the Raft cluster
    #[instrument(skip(self, data), fields(node_id = self.config.id))]
    pub async fn propose(&self, data: ProposalData) -> Result<Vec<u8>> {
        let proposal_id = self.generate_proposal_id();
        let serialized = bincode::serialize(&data)
            .map_err(|e| Error::serialization("serialize proposal", e))?;
        
        // Create response channel
        let (response_tx, response_rx) = oneshot::channel();
        
        // Store pending proposal
        self.pending_proposals.write().insert(proposal_id.clone(), response_tx);
        
        // Submit to Raft
        {
            let mut raft = self.raft.lock().await;
            
            // Check if we're the leader
            if raft.raft.state != StateRole::Leader {
                self.pending_proposals.write().remove(&proposal_id);
                return Err(Error::not_leader(
                    "propose",
                    self.config.id,
                    if raft.raft.leader_id == 0 { None } else { Some(raft.raft.leader_id) }
                ));
            }
            
            // Propose the data
            raft.propose(proposal_id, serialized)
                .map_err(|e| Error::raft("propose data", e))?;
        }
        
        // Wait for response with timeout
        let response = timeout(
            Duration::from_millis(self.config.transport.request_timeout_ms),
            response_rx
        ).await
        .map_err(|_| Error::timeout("proposal", self.config.transport.request_timeout_ms))?
        .map_err(|_| Error::internal("proposal channel closed"))?;
        
        response
    }
    
    /// Get the current status of the node
    pub fn status(&self) -> NodeStatus {
        let mut status = self.status.read().clone();
        status.uptime_seconds = self.start_time.elapsed().as_secs();
        status
    }
    
    /// Check if this node is the current leader
    pub fn is_leader(&self) -> bool {
        self.status.read().role == NodeRole::Leader
    }
    
    /// Get the current leader ID if known
    pub fn leader_id(&self) -> Option<NodeId> {
        self.status.read().leader_id
    }
    
    /// Add a new peer to the cluster
    #[instrument(skip(self), fields(node_id = self.config.id))]
    pub async fn add_peer(&self, peer_id: NodeId, address: String) -> Result<()> {
        info!("Adding peer {} at {}", peer_id, address);
        
        // Create configuration change
        let mut conf_change = raft::prelude::ConfChange::default();
        conf_change.set_change_type(raft::prelude::ConfChangeType::AddNode);
        conf_change.set_node_id(peer_id);
        conf_change.set_context(address.into_bytes());
        
        // Apply configuration change
        let mut raft = self.raft.lock().await;
        if raft.raft.state != StateRole::Leader {
            return Err(Error::not_leader(
                "add peer",
                self.config.id,
                if raft.raft.leader_id == 0 { None } else { Some(raft.raft.leader_id) }
            ));
        }
        
        raft.propose_conf_change(vec![], conf_change)
            .map_err(|e| Error::raft("propose conf change", e))?;
        
        Ok(())
    }
    
    /// Remove a peer from the cluster
    #[instrument(skip(self), fields(node_id = self.config.id))]
    pub async fn remove_peer(&self, peer_id: NodeId) -> Result<()> {
        info!("Removing peer {}", peer_id);
        
        // Create configuration change
        let mut conf_change = raft::prelude::ConfChange::default();
        conf_change.set_change_type(raft::prelude::ConfChangeType::RemoveNode);
        conf_change.set_node_id(peer_id);
        
        // Apply configuration change
        let mut raft = self.raft.lock().await;
        if raft.raft.state != StateRole::Leader {
            return Err(Error::not_leader(
                "remove peer",
                self.config.id,
                if raft.raft.leader_id == 0 { None } else { Some(raft.raft.leader_id) }
            ));
        }
        
        raft.propose_conf_change(vec![], conf_change)
            .map_err(|e| Error::raft("propose conf change", e))?;
        
        Ok(())
    }
    
    /// Get direct access to the state machine (for read operations)
    pub async fn state_machine(&self) -> tokio::sync::MutexGuard<'_, S> {
        self.state_machine.lock().await
    }
    
    // Private helper methods
    
    async fn bootstrap(&self) -> Result<()> {
        info!("Bootstrapping single-node cluster");
        
        let mut raft = self.raft.lock().await;
        raft.campaign()
            .map_err(|e| Error::raft("campaign for leadership", e))?;
        
        Ok(())
    }
    
    async fn join_cluster(&self, _join_addr: &str) -> Result<()> {
        info!("Joining existing cluster");
        // TODO: Implement cluster joining logic
        // This would involve connecting to an existing node and requesting to join
        Ok(())
    }
    
    fn generate_proposal_id(&self) -> Vec<u8> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("{}_{}", self.config.id, id).into_bytes()
    }
    
    fn clone_for_task(&self) -> RaftNodeHandle<S> {
        RaftNodeHandle {
            config: self.config.clone(),
            raft: Arc::clone(&self.raft),
            transport: Arc::clone(&self.transport),
            storage: Arc::clone(&self.storage),
            state_machine: Arc::clone(&self.state_machine),
            status: Arc::clone(&self.status),
            message_rx: Arc::clone(&self.message_rx),
            pending_proposals: Arc::clone(&self.pending_proposals),
            shutdown_rx: Arc::clone(&self.shutdown_rx),
        }
    }
    
    async fn run_main_loop(node: RaftNodeHandle<S>) {
        let mut tick_timer = interval(Duration::from_millis(node.config.raft.heartbeat_interval_ms));
        let mut message_rx = node.message_rx.lock().await;
        let mut shutdown_rx = node.shutdown_rx.lock().await.take();
        
        info!("Starting main loop for node {}", node.config.id);
        
        loop {
            tokio::select! {
                _ = tick_timer.tick() => {
                    if let Err(e) = node.handle_tick().await {
                        error!("Error handling tick: {}", e);
                    }
                },
                
                Some(msg) = message_rx.recv() => {
                    if let Err(e) = node.handle_message(msg).await {
                        error!("Error handling message: {}", e);
                    }
                },
                
                _ = async {
                    if let Some(ref mut rx) = shutdown_rx {
                        rx.await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    info!("Shutdown signal received");
                    break;
                },
            }
        }
        
        info!("Main loop ended for node {}", node.config.id);
    }
}

// Helper struct for the background task
#[derive(Clone)]
struct RaftNodeHandle<S: NodeStateMachine> {
    config: NodeConfig,
    raft: Arc<Mutex<RawNode<RaftStorage>>>,
    transport: Arc<IrohTransport>,
    storage: Arc<RaftStorage>,
    state_machine: Arc<Mutex<S>>,
    status: Arc<RwLock<NodeStatus>>,
    message_rx: Arc<Mutex<mpsc::UnboundedReceiver<TransportMessage>>>,
    pending_proposals: Arc<RwLock<HashMap<Vec<u8>, oneshot::Sender<Result<Vec<u8>>>>>>,
    shutdown_rx: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

impl<S: NodeStateMachine> RaftNodeHandle<S> {
    async fn handle_tick(&self) -> Result<()> {
        let mut raft = self.raft.lock().await;
        raft.tick();
        
        if raft.has_ready() {
            self.handle_ready(&mut raft).await?;
        }
        
        Ok(())
    }
    
    async fn handle_message(&self, msg: TransportMessage) -> Result<()> {
        match msg {
            TransportMessage::Raft { from, message } => {
                let mut raft = self.raft.lock().await;
                raft.step(message)
                    .map_err(|e| Error::raft("step message", e))?;
                
                if raft.has_ready() {
                    self.handle_ready(&mut raft).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn handle_ready(&self, raft: &mut RawNode<RaftStorage>) -> Result<()> {
        let mut ready = raft.ready();
        
        // Save hard state
        if let Some(hs) = ready.hs() {
            self.storage.save_hard_state(hs)
                .map_err(|e| Error::storage("save hard state", e))?;
        }
        
        // Save entries
        if !ready.entries().is_empty() {
            self.storage.append(ready.entries())
                .map_err(|e| Error::storage("append entries", e))?;
        }
        
        // Send messages
        for msg in ready.take_messages() {
            if let Err(e) = self.transport.send_message(msg).await {
                warn!("Failed to send message: {}", e);
            }
        }
        
        // Apply committed entries
        let committed_entries = ready.take_committed_entries();
        for entry in committed_entries {
            self.apply_entry(&entry).await?;
        }
        
        // Handle snapshots
        if !ready.snapshot().is_empty() {
            let snapshot = ready.snapshot();
            self.apply_snapshot(snapshot).await?;
        }
        
        // Advance the Raft state machine
        let mut light_ready = raft.advance(ready);
        
        // Handle light ready messages
        for msg in light_ready.take_messages() {
            if let Err(e) = self.transport.send_message(msg).await {
                warn!("Failed to send light ready message: {}", e);
            }
        }
        
        // Update node status
        self.update_status(raft).await;
        
        Ok(())
    }
    
    async fn apply_entry(&self, entry: &raft::prelude::Entry) -> Result<()> {
        if entry.data.is_empty() {
            return Ok(());
        }
        
        // Deserialize proposal
        let proposal: ProposalData = bincode::deserialize(&entry.data)
            .map_err(|e| Error::serialization("deserialize entry data", e))?;
        
        // Apply to state machine
        let mut state_machine = self.state_machine.lock().await;
        let result = state_machine.apply(entry.index, &proposal).await;
        
        // Send response to pending proposal if it exists
        if !entry.context.is_empty() {
            let mut pending = self.pending_proposals.write();
            if let Some(response_tx) = pending.remove(&entry.context) {
                let _ = response_tx.send(result);
            }
        }
        
        Ok(())
    }
    
    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        // Restore state machine from snapshot
        let mut state_machine = self.state_machine.lock().await;
        state_machine.restore(&snapshot.data).await?;
        
        // Apply snapshot to storage
        self.storage.apply_snapshot(snapshot.clone())
            .map_err(|e| Error::storage("apply snapshot", e))?;
        
        Ok(())
    }
    
    async fn update_status(&self, raft: &RawNode<RaftStorage>) {
        let role = match raft.raft.state {
            StateRole::Follower => NodeRole::Follower,
            StateRole::Candidate => NodeRole::Candidate,
            StateRole::Leader => NodeRole::Leader,
            StateRole::PreCandidate => NodeRole::Candidate,
        };
        
        let leader_id = if raft.raft.leader_id == 0 {
            None
        } else {
            Some(raft.raft.leader_id)
        };
        
        let mut status = self.status.write();
        status.role = role;
        status.term = raft.raft.term;
        status.leader_id = leader_id;
        status.last_log_index = raft.raft.raft_log.last_index();
        status.committed_index = raft.raft.raft_log.committed;
        status.applied_index = raft.raft.raft_log.applied;
    }
}

impl<S: NodeStateMachine> std::fmt::Debug for RaftNode<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RaftNode")
            .field("node_id", &self.config.id)
            .field("role", &self.status.read().role)
            .finish()
    }
}