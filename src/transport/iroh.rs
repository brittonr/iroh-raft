//! Iroh transport adapter for Raft consensus
//!
//! This module provides an efficient transport layer for Raft messages over Iroh P2P,
//! with integrated OpenTelemetry metrics and graceful shutdown.

use iroh::endpoint::Connection;
use iroh::{Endpoint, NodeId};
use raft::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock, Notify};
use tokio::task::JoinHandle;

use crate::error::{RaftError, Result};
#[cfg(feature = "metrics-otel")]
use crate::metrics::{MetricsRegistry, LatencyTimer};
#[cfg(feature = "metrics-otel")]
use opentelemetry::{global, metrics::Counter, KeyValue};
use crate::transport::shared::SharedNodeState;
use crate::transport::protocol::{
    generate_request_id, read_message, write_message, MessageType,
};

/// Metrics for Iroh Raft transport
#[derive(Debug, Clone)]
pub struct IrohRaftMetrics {
    /// Number of currently active peer connections
    pub active_connections: usize,
    /// Total number of messages sent since transport started
    pub messages_sent: u64,
    /// Total number of messages received since transport started
    pub messages_received: u64,
}

#[cfg(feature = "metrics-otel")]
lazy_static::lazy_static! {
    static ref RAFT_MESSAGES_SENT: Counter<u64> = {
        global::meter("iroh-raft")
            .u64_counter("raft.messages.sent")
            .with_description("Number of Raft messages sent over Iroh transport")
            .build()
    };
    static ref RAFT_MESSAGES_RECEIVED: Counter<u64> = {
        global::meter("iroh-raft")
            .u64_counter("raft.messages.received")
            .with_description("Number of Raft messages received over Iroh transport")
            .build()
    };
}

/// Raft-specific message types for optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RaftMessagePriority {
    /// Election messages (RequestVote, Vote) - highest priority
    Election,
    /// Heartbeat messages - high priority
    Heartbeat,
    /// Log append messages - normal priority
    LogAppend,
    /// Snapshot messages - low priority (bulk transfer)
    Snapshot,
}

impl RaftMessagePriority {
    fn from_raft_message(msg: &Message) -> Self {
        match msg.msg_type() {
            raft::eraftpb::MessageType::MsgRequestVote | raft::eraftpb::MessageType::MsgRequestVoteResponse => Self::Election,
            raft::eraftpb::MessageType::MsgHeartbeat | raft::eraftpb::MessageType::MsgHeartbeatResponse => Self::Heartbeat,
            raft::eraftpb::MessageType::MsgSnapshot => Self::Snapshot,
            _ => Self::LogAppend,
        }
    }

    fn as_u8(&self) -> u8 {
        match self {
            Self::Election => 0,
            Self::Heartbeat => 1,
            Self::LogAppend => 2,
            Self::Snapshot => 3,
        }
    }
}

/// Buffered Raft message with metadata
struct BufferedRaftMessage {
    message: Message,
    priority: RaftMessagePriority,
    timestamp: Instant,
}

/// Connection state for a peer
struct PeerConnection {
    node_id: u64,
    iroh_node_id: NodeId,
    connection: Connection,
    last_activity: Instant,
}

/// Iroh transport adapter for Raft
pub struct IrohRaftTransport {
    /// Reference to shared node state
    node: Arc<SharedNodeState>,
    /// Iroh endpoint for P2P communication
    endpoint: Endpoint,
    /// Our Iroh node ID
    local_node_id: NodeId,
    /// Active connections to peers
    connections: Arc<RwLock<HashMap<u64, PeerConnection>>>,
    /// Message buffer for batching and retry
    message_buffer: Arc<Mutex<HashMap<u64, VecDeque<BufferedRaftMessage>>>>,
    /// Channel to send incoming Raft messages to the Raft manager
    raft_rx_tx: mpsc::UnboundedSender<(u64, Message)>,
    /// Background task handles
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Shutdown channel
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    /// Shutdown notification for graceful termination
    shutdown_notify: Arc<Notify>,
    /// Metrics registry for OpenTelemetry integration
    #[cfg(feature = "metrics-otel")]
    metrics: Option<MetricsRegistry>,
    /// Local metrics tracking
    messages_sent: Arc<Mutex<HashMap<String, u64>>>,
    messages_received: Arc<Mutex<HashMap<String, u64>>>,
}

impl IrohRaftTransport {
    /// Create a new Iroh Raft transport
    pub fn new(
        node: Arc<SharedNodeState>,
        endpoint: Endpoint,
        local_node_id: NodeId,
        raft_rx_tx: mpsc::UnboundedSender<(u64, Message)>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let shutdown_notify = Arc::new(Notify::new());
        
        #[cfg(feature = "metrics-otel")]
        let metrics = MetricsRegistry::new("iroh-raft-transport").ok();

        Self {
            node,
            endpoint,
            local_node_id,
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_buffer: Arc::new(Mutex::new(HashMap::new())),
            raft_rx_tx,
            tasks: Mutex::new(Vec::new()),
            shutdown_tx,
            shutdown_rx,
            shutdown_notify,
            #[cfg(feature = "metrics-otel")]
            metrics,
            messages_sent: Arc::new(Mutex::new(HashMap::new())),
            messages_received: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Create with metrics registry
    #[cfg(feature = "metrics-otel")]
    pub fn with_metrics(
        node: Arc<SharedNodeState>,
        endpoint: Endpoint,
        local_node_id: NodeId,
        raft_rx_tx: mpsc::UnboundedSender<(u64, Message)>,
        metrics: MetricsRegistry,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let shutdown_notify = Arc::new(Notify::new());

        Self {
            node,
            endpoint,
            local_node_id,
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_buffer: Arc::new(Mutex::new(HashMap::new())),
            raft_rx_tx,
            tasks: Mutex::new(Vec::new()),
            shutdown_tx,
            shutdown_rx,
            shutdown_notify,
            metrics: Some(metrics),
            messages_sent: Arc::new(Mutex::new(HashMap::new())),
            messages_received: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start the transport (process outgoing messages)
    pub async fn start(&self) -> Result<()> {
        // Start message batch processor for outgoing messages
        let batch_task = self.spawn_batch_processor();

        // Start connection health checker
        let health_task = self.spawn_health_checker();

        // Store task handles
        let mut tasks = self.tasks.lock().await;
        tasks.push(batch_task);
        tasks.push(health_task);

        tracing::info!("Iroh Raft transport started (outgoing connections only)");
        Ok(())
    }

    /// Send a Raft message to a peer
    pub async fn send_message(&self, to: u64, message: Message) -> Result<()> {
        if self.is_shutting_down() {
            return Err(RaftError::Internal {
                message: "Transport is shutting down".to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        tracing::debug!("Sending {:?} message to node {}", message.msg_type(), to);
        
        let priority = RaftMessagePriority::from_raft_message(&message);

        // For high-priority messages, try to send immediately
        if matches!(
            priority,
            RaftMessagePriority::Election | RaftMessagePriority::Heartbeat | RaftMessagePriority::LogAppend
        ) {
            if let Err(e) = self.try_send_immediate(to, &message, priority).await {
                tracing::warn!("Immediate send failed for node {}: {}, buffering", to, e);
                self.buffer_message(to, message, priority).await?;
            }
        } else {
            // For lower priority messages, always buffer for batching
            self.buffer_message(to, message, priority).await?;
        }

        Ok(())
    }

    /// Try to send a message immediately
    async fn try_send_immediate(
        &self,
        to: u64,
        message: &Message,
        _priority: RaftMessagePriority,
    ) -> Result<()> {
        #[cfg(feature = "metrics-otel")]
        let timer = self.metrics.as_ref().map(|_| LatencyTimer::start("message_send"));

        // Get or create connection
        let conn = self.get_or_create_connection(to).await?;

        // Serialize Raft message
        let msg_data = crate::raft::messages::serialize_message(message)?;

        // Create RPC request for the raft service
        let request_id = generate_request_id();
        let request = crate::transport::protocol::RpcRequest {
            service: "raft".to_string(),
            method: "raft.message".to_string(),
            payload: msg_data,
        };
        
        // Serialize RPC request
        let request_bytes = crate::transport::protocol::serialize_payload(&request)?;

        // Open bidirectional stream for RPC
        let (mut send, mut recv) = conn.connection.open_bi().await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to open bi stream: {}", e),
                backtrace: snafu::Backtrace::new(),
            })?;

        // Send RPC request
        write_message(&mut send, MessageType::Request, Some(request_id), &request_bytes).await?;
        
        // Finish the send stream to signal we're done sending
        send.finish().map_err(|e| RaftError::Internal {
            message: format!("Failed to finish send stream: {}", e),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        // Wait for response to confirm delivery
        let _ = read_message(&mut recv).await;

        // Update metrics
        self.record_raft_message_sent(message.msg_type());
        
        #[cfg(feature = "metrics-otel")]
        if let (Some(metrics), Some(timer)) = (&self.metrics, timer) {
            metrics.transport_metrics().record_message_sent(
                &format!("{:?}", message.msg_type()),
                &to.to_string()
            );
            metrics.transport_metrics().record_message_latency(
                &format!("{:?}", message.msg_type()),
                timer.elapsed()
            );
        }

        Ok(())
    }

    /// Buffer a message for batched sending
    async fn buffer_message(
        &self,
        to: u64,
        message: Message,
        priority: RaftMessagePriority,
    ) -> Result<()> {
        let mut buffer = self.message_buffer.lock().await;
        let queue = buffer.entry(to).or_insert_with(VecDeque::new);

        // Add message to queue based on priority
        let buffered = BufferedRaftMessage {
            message,
            priority,
            timestamp: Instant::now(),
        };

        // Insert based on priority (higher priority at front)
        let insert_pos = queue
            .iter()
            .position(|m| m.priority.as_u8() > priority.as_u8())
            .unwrap_or(queue.len());
        queue.insert(insert_pos, buffered);

        // Limit buffer size
        const MAX_BUFFERED: usize = 1000;
        while queue.len() > MAX_BUFFERED {
            queue.pop_back();
        }

        Ok(())
    }

    /// Get or create a connection to a peer
    async fn get_or_create_connection(&self, peer_id: u64) -> Result<PeerConnection> {
        // Check existing connections
        {
            let connections = self.connections.read().await;
            if let Some(conn) = connections.get(&peer_id) {
                // Check if connection is still recent (less than 1 hour old and 5 min idle)
                if conn.last_activity.elapsed() < Duration::from_secs(300) {
                    return Ok(PeerConnection {
                        node_id: conn.node_id,
                        iroh_node_id: conn.iroh_node_id,
                        connection: conn.connection.clone(),
                        last_activity: conn.last_activity,
                    });
                }
            }
        }

        // Create new connection
        let peer_info = self.node.get_peer(peer_id).await
            .ok_or_else(|| RaftError::ClusterError {
                message: format!("Unknown peer {}", peer_id),
                backtrace: snafu::Backtrace::new(),
            })?;

        let node_registry = self.node.node_registry();
        let node_addr = match node_registry.get_node_addr(peer_id).await {
            Some(addr) => addr,
            None => {
                // Fallback to peer info
                if let Some(p2p_node_id_str) = &peer_info.p2p_node_id {
                    let iroh_node_id = p2p_node_id_str.parse::<iroh::NodeId>()
                        .map_err(|e| RaftError::Internal {
                            message: format!("Invalid Iroh node ID: {}", e),
                            backtrace: snafu::Backtrace::new(),
                        })?;
                    
                    let mut node_addr = iroh::NodeAddr::new(iroh_node_id);
                    
                    // Add addresses and relay if available
                    let addrs: Vec<std::net::SocketAddr> = peer_info.p2p_addresses
                        .iter()
                        .filter_map(|a| a.parse().ok())
                        .collect();
                    if !addrs.is_empty() {
                        node_addr = node_addr.with_direct_addresses(addrs);
                    }
                    
                    if let Some(relay) = &peer_info.p2p_relay_url {
                        if let Ok(relay_url) = relay.parse() {
                            node_addr = node_addr.with_relay_url(relay_url);
                        }
                    }
                    
                    node_addr
                } else {
                    return Err(RaftError::ClusterError {
                        message: format!("Peer {} has no P2P node ID", peer_id),
                        backtrace: snafu::Backtrace::new(),
                    });
                }
            }
        };

        let iroh_node_id = node_addr.node_id;

        // Connect with timeout
        let connection = tokio::time::timeout(
            Duration::from_secs(10),
            self.endpoint.connect(node_addr, crate::transport::BLIXARD_ALPN)
        ).await
        .map_err(|_| RaftError::Timeout {
            operation: format!("connect to peer {}", peer_id),
            duration: Duration::from_secs(10),
            backtrace: snafu::Backtrace::new(),
        })?
        .map_err(|e| RaftError::Internal {
            message: format!("Connection error to peer {}: {}", peer_id, e),
            backtrace: snafu::Backtrace::new(),
        })?;

        // Record connection established in metrics
        #[cfg(feature = "metrics-otel")]
        if let Some(metrics) = &self.metrics {
            metrics.transport_metrics().record_connection_established(&peer_id.to_string());
        }

        // Store connection
        let peer_conn = PeerConnection {
            node_id: peer_id,
            iroh_node_id,
            connection: connection.clone(),
            last_activity: Instant::now(),
        };

        {
            let mut connections = self.connections.write().await;
            connections.insert(peer_id, peer_conn);
        }

        Ok(PeerConnection {
            node_id: peer_id,
            iroh_node_id,
            connection,
            last_activity: Instant::now(),
        })
    }

    /// Spawn task to process message batches
    fn spawn_batch_processor(&self) -> JoinHandle<()> {
        let message_buffer = self.message_buffer.clone();
        let connections = self.connections.clone();
        let node = self.node.clone();
        let messages_sent = self.messages_sent.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();
        let endpoint = self.endpoint.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            tracing::info!("Raft transport batch processor shutting down");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        Self::process_message_batches(
                            &message_buffer,
                            &connections,
                            &node,
                            &messages_sent,
                            &endpoint,
                        ).await;
                    }
                }
            }
        })
    }

    /// Spawn task to check connection health
    fn spawn_health_checker(&self) -> JoinHandle<()> {
        let connections = self.connections.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            tracing::info!("Raft transport health checker shutting down");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        Self::check_connection_health(&connections).await;
                    }
                }
            }
        })
    }

    /// Process batched messages
    async fn process_message_batches(
        message_buffer: &Arc<Mutex<HashMap<u64, VecDeque<BufferedRaftMessage>>>>,
        connections: &Arc<RwLock<HashMap<u64, PeerConnection>>>,
        node: &Arc<SharedNodeState>,
        messages_sent: &Arc<Mutex<HashMap<String, u64>>>,
        endpoint: &Endpoint,
    ) {
        let mut buffer = message_buffer.lock().await;

        for (peer_id, messages) in buffer.iter_mut() {
            if messages.is_empty() {
                continue;
            }

            // Try to get connection
            let connection = {
                let connections = connections.read().await;
                match connections.get(peer_id) {
                    Some(conn) => conn.connection.clone(),
                    None => {
                        // Try to create connection for this peer
                        if let Some(peer_info) = node.get_peer(*peer_id).await {
                            if let Some(p2p_node_id_str) = &peer_info.p2p_node_id {
                                if let Ok(iroh_node_id) = p2p_node_id_str.parse::<NodeId>() {
                                    match endpoint.connect(iroh_node_id, crate::transport::BLIXARD_ALPN).await {
                                        Ok(conn) => conn,
                                        Err(_) => continue,
                                    }
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                }
            };

            // Send messages
            let mut sent_count = 0;
            while let Some(buffered) = messages.pop_front() {
                // Skip old messages
                if buffered.timestamp.elapsed() > Duration::from_secs(30) {
                    continue;
                }

                if let Err(_) = Self::send_single_message(&connection, &buffered.message).await {
                    // Put message back if send failed
                    messages.push_front(buffered);
                    break;
                }

                // Record metrics
                {
                    let mut sent = messages_sent.lock().await;
                    let key = format!("{:?}", buffered.message.msg_type());
                    *sent.entry(key).or_insert(0) += 1;
                }

                sent_count += 1;
                if sent_count >= 100 {
                    break; // Batch size limit
                }
            }
        }
    }

    /// Send a single message over a connection
    async fn send_single_message(connection: &Connection, message: &Message) -> Result<()> {
        let msg_data = crate::raft::messages::serialize_message(message)?;
        let request_id = generate_request_id();
        let request = crate::transport::protocol::RpcRequest {
            service: "raft".to_string(),
            method: "raft.message".to_string(),
            payload: msg_data,
        };
        
        let request_bytes = crate::transport::protocol::serialize_payload(&request)?;
        let (mut send, mut recv) = connection.open_bi().await
            .map_err(|e| RaftError::Internal {
                message: format!("Failed to open bi stream: {}", e),
                backtrace: snafu::Backtrace::new(),
            })?;
        
        write_message(&mut send, MessageType::Request, Some(request_id), &request_bytes).await?;
        send.finish().map_err(|e| RaftError::Internal {
            message: format!("Failed to finish send stream: {}", e),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        let _ = read_message(&mut recv).await;
        Ok(())
    }

    /// Check health of all connections
    async fn check_connection_health(connections: &Arc<RwLock<HashMap<u64, PeerConnection>>>) {
        let mut to_remove = Vec::new();

        {
            let connections = connections.read().await;
            for (peer_id, conn) in connections.iter() {
                if conn.last_activity.elapsed() > Duration::from_secs(300) {
                    to_remove.push(*peer_id);
                }
            }
        }

        if !to_remove.is_empty() {
            let mut connections = connections.write().await;
            for peer_id in to_remove {
                connections.remove(&peer_id);
                tracing::info!("Removed stale connection to peer {}", peer_id);
            }
        }
    }

    /// Record sent Raft message metrics
    fn record_raft_message_sent(&self, msg_type: raft::eraftpb::MessageType) {
        if let Ok(mut sent) = self.messages_sent.try_lock() {
            let key = format!("Raft_{:?}", msg_type);
            *sent.entry(key).or_insert(0) += 1;
        }
        
        #[cfg(feature = "metrics-otel")]
        RAFT_MESSAGES_SENT.add(
            1,
            &[
                KeyValue::new("transport_type", "iroh"),
                KeyValue::new("message_type", format!("Raft_{:?}", msg_type)),
            ],
        );
    }

    /// Get current transport connection and performance metrics
    /// 
    /// Returns a snapshot of transport metrics including active connections,
    /// message throughput statistics, and connection pool performance data.
    /// 
    /// # Returns
    /// 
    /// `IrohRaftMetrics` containing:
    /// - `active_connections`: Number of currently established peer connections
    /// - `messages_sent`: Total number of messages sent since transport startup
    /// - `messages_received`: Total number of messages received since transport startup
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// use iroh_raft::transport::iroh::IrohRaftTransport;
    /// 
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let transport = IrohRaftTransport::new(/* config */).await?;
    /// 
    /// // Get current metrics
    /// let metrics = transport.get_metrics().await;
    /// println!("Active connections: {}", metrics.active_connections);
    /// println!("Messages sent: {}", metrics.messages_sent);
    /// println!("Messages received: {}", metrics.messages_received);
    /// # Ok(())
    /// # }
    /// ```
    /// 
    /// # Performance
    /// 
    /// This method acquires read locks on internal connection state, so it's
    /// relatively lightweight but should not be called at very high frequency.
    /// Consider caching the results if called frequently.
    pub async fn get_metrics(&self) -> IrohRaftMetrics {
        let connections = self.connections.read().await;
        let active_connections = connections.len();
        
        let messages_sent = {
            let sent = self.messages_sent.lock().await;
            sent.values().sum()
        };
        
        let messages_received = {
            let received = self.messages_received.lock().await;
            received.values().sum()
        };
        
        IrohRaftMetrics {
            active_connections,
            messages_sent,
            messages_received,
        }
    }

    /// Shutdown the transport gracefully
    /// 
    /// This method performs a coordinated shutdown of all transport components:
    /// - Signals all background tasks to terminate
    /// - Waits for running tasks to complete cleanly
    /// - Closes all active peer connections with proper notifications
    /// - Records final metrics and connection statistics
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// use iroh_raft::transport::iroh::IrohRaftTransport;
    /// 
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let transport = IrohRaftTransport::new(/* config */).await?;
    /// 
    /// // Later, when shutting down
    /// transport.shutdown().await;
    /// println!("Transport shutdown complete");
    /// # Ok(())
    /// # }
    /// ```
    /// 
    /// # Behavior
    /// 
    /// - **Non-blocking**: Returns immediately after initiating shutdown
    /// - **Graceful**: Allows in-flight operations to complete
    /// - **Clean**: Properly closes resources and connections
    /// - **Metrics**: Records shutdown statistics for observability
    pub async fn shutdown(&self) {
        tracing::info!("Shutting down Iroh Raft transport");

        // Signal shutdown to all tasks
        let _ = self.shutdown_tx.send(true);
        self.shutdown_notify.notify_waiters();

        // Wait for all background tasks to complete
        let mut tasks = self.tasks.lock().await;
        for task in tasks.drain(..) {
            if let Err(e) = task.await {
                tracing::warn!("Task failed during shutdown: {:?}", e);
            }
        }

        // Gracefully close all connections and record metrics
        let mut connections = self.connections.write().await;
        let connection_count = connections.len();
        
        for (peer_id, conn) in connections.drain() {
            #[cfg(feature = "metrics-otel")]
            {
                let duration = conn.last_activity.elapsed();
                if let Some(metrics) = &self.metrics {
                    metrics.transport_metrics().record_connection_closed(&peer_id.to_string(), duration);
                }
            }
            
            #[cfg(not(feature = "metrics-otel"))]
            let _ = peer_id;
            
            // Close the connection gracefully
            conn.connection.close(0u32.into(), b"shutdown");
        }
        
        tracing::info!(
            "Iroh Raft transport shutdown complete. Closed {} connections",
            connection_count
        );
    }
    
    /// Wait for shutdown signal
    pub async fn wait_for_shutdown(&self) {
        self.shutdown_notify.notified().await;
    }
    
    /// Check if transport is shutting down
    pub fn is_shutting_down(&self) -> bool {
        *self.shutdown_rx.borrow()
    }
}

/// Extension trait to integrate with existing PeerConnector
impl IrohRaftTransport {
    /// Send a Raft message using this transport (compatible with PeerConnector interface)
    pub async fn send_raft_message(
        &self,
        to: u64,
        message: raft::prelude::Message,
    ) -> Result<()> {
        self.send_message(to, message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_priority() {
        let mut vote_msg = Message::default();
        vote_msg.set_msg_type(raft::eraftpb::MessageType::MsgRequestVote);
        assert_eq!(
            RaftMessagePriority::from_raft_message(&vote_msg),
            RaftMessagePriority::Election
        );

        let mut heartbeat_msg = Message::default();
        heartbeat_msg.set_msg_type(raft::eraftpb::MessageType::MsgHeartbeat);
        assert_eq!(
            RaftMessagePriority::from_raft_message(&heartbeat_msg),
            RaftMessagePriority::Heartbeat
        );

        let mut append_msg = Message::default();
        append_msg.set_msg_type(raft::eraftpb::MessageType::MsgAppend);
        assert_eq!(
            RaftMessagePriority::from_raft_message(&append_msg),
            RaftMessagePriority::LogAppend
        );

        let mut snapshot_msg = Message::default();
        snapshot_msg.set_msg_type(raft::eraftpb::MessageType::MsgSnapshot);
        assert_eq!(
            RaftMessagePriority::from_raft_message(&snapshot_msg),
            RaftMessagePriority::Snapshot
        );
    }

    #[test]
    fn test_priority_ordering() {
        assert!(RaftMessagePriority::Election.as_u8() < RaftMessagePriority::Heartbeat.as_u8());
        assert!(RaftMessagePriority::Heartbeat.as_u8() < RaftMessagePriority::LogAppend.as_u8());
        assert!(RaftMessagePriority::LogAppend.as_u8() < RaftMessagePriority::Snapshot.as_u8());
    }
}