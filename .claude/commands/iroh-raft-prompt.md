# Rust Systems Programming Prompt Generator

## Generated Rust Prompt

<system_context>
You are a Rust systems programming expert with deep knowledge of:
- Ownership, borrowing, lifetimes, and the borrow checker
- Zero-cost abstractions and performance optimization
- Memory safety without garbage collection
- Trait systems and type-level programming
- Tokio async runtime and ecosystem
- Error handling with snafu and anyhow
- Property-based testing with proptest
- Deterministic simulation testing with madsim
- TUI development with ratatui
- GUI development with Dioxus 0.6 (use_signal, no cx)
- Cargo ecosystem and best practices
</system_context>

<task_specification>
<primary_task>
Build a comprehensive Rust library crate implementing the Raft consensus protocol using Iroh P2P networking (v0.91+), following Iroh's protocol design patterns for distributed systems.
</primary_task>

<task_category>
Type: Library (Distributed Systems)
Complexity: Systems-level
Performance Critical: Yes
Requires Deterministic Testing: Yes
</task_category>
</task_specification>

<technical_requirements>
<rust_version>
Minimum Rust Version: 1.75+
Edition: 2021
</rust_version>

<dependencies>
```toml
[dependencies]
# Core async runtime
tokio = { version = "1.40", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# Raft consensus
raft = { version = "0.7", default-features = false, features = ["prost-codec"] }
raft-proto = { version = "0.7", default-features = false, features = ["prost-codec"] }

# Iroh P2P networking (CRITICAL: use v0.91+)
iroh = "0.91"

# Storage backend
redb = "2.2"

# Error handling
snafu = "0.8"  # For library error types
anyhow = "1.0" # For examples only

# Serialization
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
prost = "0.13"

# Logging and metrics
tracing = "0.1"
tracing-subscriber = "0.3"
opentelemetry = { version = "0.27", optional = true }

# Utilities
bytes = "1.8"
parking_lot = "0.12"
uuid = { version = "1.11", features = ["v4", "serde"] }
rand = "0.8"

[dev-dependencies]
# Property-based testing
proptest = "1.5"

# Deterministic simulation testing
madsim = "0.2"

# Benchmarking
criterion = "0.5"

# Testing utilities
pretty_assertions = "1.0"
tempfile = "3.14"
test-case = "3.0"
```
</dependencies>

<architecture>
- Pattern: Protocol Handler with State Machine
- Error Strategy: snafu for structured library errors
- Async Runtime: tokio with full features
- Testing Strategy: unit, integration, proptest, madsim simulation
- Serialization: bincode for internal, prost for Raft messages
</architecture>
</technical_requirements>

<iroh_protocol_implementation>
## Iroh Protocol Handler Implementation

```rust
use iroh::endpoint::{Connection, Endpoint};
use snafu::{prelude::*, Backtrace};
use std::future::Future;
use std::pin::Pin;

/// Protocol ALPN identifier
pub const RAFT_ALPN: &[u8] = b"iroh-raft/0";

/// Protocol errors using SNAFU
#[derive(Debug, Snafu)]
pub enum ProtocolError {
    #[snafu(display("Failed to establish connection to {node_id}"))]
    ConnectionFailed {
        node_id: String,
        source: iroh::endpoint::ConnectionError,
        backtrace: Backtrace,
    },
    
    #[snafu(display("Stream error on node {node_id}"))]
    StreamError {
        node_id: String,
        source: std::io::Error,
        backtrace: Backtrace,
    },
    
    #[snafu(display("Message serialization failed"))]
    Serialization {
        source: bincode::Error,
        backtrace: Backtrace,
    },
    
    #[snafu(display("Protocol version mismatch: expected {expected}, got {actual}"))]
    VersionMismatch {
        expected: String,
        actual: String,
        backtrace: Backtrace,
    },
}

/// Raft protocol handler following Iroh patterns
#[derive(Debug, Clone)]
pub struct RaftProtocolHandler {
    node_id: u64,
    raft_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
}

impl RaftProtocolHandler {
    pub fn new(
        node_id: u64,
        raft_tx: mpsc::UnboundedSender<(u64, raft::prelude::Message)>,
    ) -> Self {
        Self { node_id, raft_tx }
    }
    
    /// Accept handler following Iroh protocol pattern
    pub fn accept(
        &self,
        connection: Connection,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProtocolError>> + Send + 'static>> {
        let handler = self.clone();
        Box::pin(async move {
            handler.handle_connection(connection).await
        })
    }
    
    async fn handle_connection(&self, connection: Connection) -> Result<(), ProtocolError> {
        loop {
            // Accept bidirectional streams for Raft messages
            match connection.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    // Handle RPC-style request/response
                    let message = self.read_message(&mut recv).await?;
                    self.process_raft_message(message).await?;
                    self.send_ack(&mut send).await?;
                    
                    // Close stream properly
                    send.finish().await.context(StreamSnafu {
                        node_id: connection.remote_node_id().to_string(),
                    })?;
                }
                Err(e) => {
                    tracing::debug!("Connection closed: {}", e);
                    break;
                }
            }
        }
        
        // Ensure connection closes gracefully
        connection.closed().await;
        Ok(())
    }
}
```
</iroh_protocol_implementation>

<raft_integration_patterns>
## Raft State Machine Integration

```rust
use snafu::{prelude::*, Backtrace};

/// Raft-specific errors
#[derive(Debug, Snafu)]
pub enum RaftError {
    #[snafu(display("Storage operation failed: {operation}"))]
    Storage {
        operation: String,
        source: redb::Error,
        backtrace: Backtrace,
    },
    
    #[snafu(display("Network error communicating with peer {peer_id}"))]
    Network {
        peer_id: u64,
        source: ProtocolError,
        backtrace: Backtrace,
    },
    
    #[snafu(display("Consensus violation: {details}"))]
    ConsensusViolation {
        details: String,
        backtrace: Backtrace,
    },
    
    #[snafu(display("State machine error: {context}"))]
    StateMachine {
        context: String,
        source: Box<dyn std::error::Error + Send + Sync>,
        backtrace: Backtrace,
    },
}

/// Generic state machine trait for Raft
pub trait StateMachine: Send + Sync + 'static {
    type Command: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;
    type State: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync;
    
    async fn apply_command(&mut self, command: Self::Command) -> Result<(), RaftError>;
    async fn create_snapshot(&self) -> Result<Self::State, RaftError>;
    async fn restore_from_snapshot(&mut self, snapshot: Self::State) -> Result<(), RaftError>;
    fn get_current_state(&self) -> &Self::State;
}

/// Transport layer abstraction
pub struct IrohRaftTransport {
    endpoint: Endpoint,
    connections: Arc<RwLock<HashMap<u64, Connection>>>,
    metrics: Arc<TransportMetrics>,
}

impl IrohRaftTransport {
    pub async fn send_message(
        &self,
        to: u64,
        message: raft::prelude::Message,
    ) -> Result<(), RaftError> {
        let conn = self.get_or_create_connection(to).await
            .context(NetworkSnafu { peer_id: to })?;
        
        // Open bidirectional stream for RPC
        let (mut send, mut recv) = conn.open_bi().await
            .context(NetworkSnafu { peer_id: to })?;
        
        // Send message and await acknowledgment
        self.write_message(&mut send, &message).await?;
        send.finish().await.context(NetworkSnafu { peer_id: to })?;
        
        self.read_ack(&mut recv).await?;
        
        // Update metrics
        self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }
}
```
</raft_integration_patterns>

<testing_patterns>
## Comprehensive Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use madsim::runtime::Runtime;
    
    /// Property-based testing for protocol messages
    proptest! {
        #[test]
        fn test_message_serialization_roundtrip(
            msg in raft_message_strategy()
        ) {
            let serialized = serialize_message(&msg)?;
            let deserialized = deserialize_message(&serialized)?;
            prop_assert_eq!(msg, deserialized);
        }
        
        #[test]
        fn test_protocol_handles_malformed_input(
            data in prop::collection::vec(any::<u8>(), 0..10000)
        ) {
            // Should not panic on arbitrary input
            let _ = deserialize_message(&data);
        }
    }
    
    /// Deterministic simulation testing with MadSim
    #[test]
    fn test_raft_consensus_under_partition() {
        let runtime = Runtime::with_seed(42); // Deterministic
        
        runtime.block_on(async {
            // Create 5-node Raft cluster
            let nodes = create_test_cluster(5).await;
            
            // Elect initial leader
            wait_for_leader(&nodes).await;
            
            // Simulate network partition (3-2 split)
            runtime.net.disconnect(0, 3);
            runtime.net.disconnect(0, 4);
            runtime.net.disconnect(1, 3);
            runtime.net.disconnect(1, 4);
            runtime.net.disconnect(2, 3);
            runtime.net.disconnect(2, 4);
            
            // Majority partition should maintain consensus
            let majority = &nodes[0..3];
            propose_and_verify(majority, "test_value").await;
            
            // Heal partition
            runtime.net.connect_all();
            
            // Verify eventual consistency
            wait_for_convergence(&nodes).await;
            verify_all_nodes_consistent(&nodes).await;
        });
    }
    
    #[test]
    fn test_leader_election_determinism() {
        // Multiple runs with same seed should produce same result
        let results: Vec<_> = (0..10)
            .map(|_| {
                let runtime = Runtime::with_seed(123);
                runtime.block_on(async {
                    let nodes = create_test_cluster(3).await;
                    wait_for_leader(&nodes).await
                })
            })
            .collect();
        
        // All runs should elect the same leader
        assert!(results.windows(2).all(|w| w[0] == w[1]));
    }
}

/// Generate arbitrary Raft messages for testing
fn raft_message_strategy() -> impl Strategy<Value = raft::prelude::Message> {
    prop_oneof![
        // Vote request
        (any::<u64>(), any::<u64>(), any::<u64>()).prop_map(|(term, candidate, last_log)| {
            let mut msg = raft::prelude::Message::default();
            msg.set_msg_type(raft::prelude::MessageType::MsgRequestVote);
            msg.term = term;
            msg.from = candidate;
            msg.log_term = last_log;
            msg
        }),
        // Heartbeat
        (any::<u64>(), any::<u64>()).prop_map(|(term, leader)| {
            let mut msg = raft::prelude::Message::default();
            msg.set_msg_type(raft::prelude::MessageType::MsgHeartbeat);
            msg.term = term;
            msg.from = leader;
            msg
        }),
    ]
}
```
</testing_patterns>

<performance_optimizations>
## Performance-Critical Patterns

```rust
/// Message batching for efficiency
pub struct MessageBatcher {
    buffer: Arc<Mutex<HashMap<u64, VecDeque<raft::prelude::Message>>>>,
    batch_size: usize,
    flush_interval: Duration,
}

impl MessageBatcher {
    pub async fn add_message(&self, to: u64, msg: raft::prelude::Message) {
        let mut buffer = self.buffer.lock().await;
        let queue = buffer.entry(to).or_insert_with(VecDeque::new);
        queue.push_back(msg);
        
        if queue.len() >= self.batch_size {
            self.flush_peer(to).await;
        }
    }
    
    async fn flush_peer(&self, peer_id: u64) {
        let messages = {
            let mut buffer = self.buffer.lock().await;
            buffer.remove(&peer_id).unwrap_or_default()
        };
        
        if !messages.is_empty() {
            // Send as batch for better throughput
            self.send_batch(peer_id, messages).await;
        }
    }
}

/// Zero-copy message handling
pub struct ZeroCopyMessage<'a> {
    header: MessageHeader,
    payload: &'a [u8],
}

impl<'a> ZeroCopyMessage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, ProtocolError> {
        // Parse without copying payload
        let header = MessageHeader::decode(&data[..HEADER_SIZE])
            .context(DeserializationSnafu)?;
        let payload = &data[HEADER_SIZE..];
        Ok(Self { header, payload })
    }
}
```
</performance_optimizations>

<success_criteria>
- ✅ Implements Iroh protocol handler pattern correctly
- ✅ Uses ALPN for protocol negotiation
- ✅ Proper bidirectional stream handling
- ✅ SNAFU errors with full context and backtraces
- ✅ Deterministic testing with MadSim
- ✅ Property-based testing with proptest
- ✅ Zero-copy optimizations where applicable
- ✅ Message batching for efficiency
- ✅ Graceful connection lifecycle management
- ✅ Comprehensive metrics and observability
- ✅ Thread-safe with Arc<RwLock<T>> patterns
- ✅ No unwrap() in library code
</success_criteria>

<implementation_checklist>
## Implementation Priority Order

1. **Protocol Foundation**
   - [ ] Define ALPN constant and message types
   - [ ] Implement RaftProtocolHandler with accept pattern
   - [ ] Add bidirectional stream handling
   - [ ] Implement proper connection lifecycle

2. **Error Handling**
   - [ ] Define comprehensive error types with SNAFU
   - [ ] Add context to all fallible operations
   - [ ] Include backtraces for debugging

3. **Raft Integration**
   - [ ] Implement StateMachine trait
   - [ ] Add storage layer with redb
   - [ ] Wire up message routing
   - [ ] Handle consensus operations

4. **Testing**
   - [ ] Property tests for serialization
   - [ ] MadSim tests for distributed scenarios
   - [ ] Deterministic failure injection
   - [ ] Benchmark critical paths

5. **Optimizations**
   - [ ] Message batching
   - [ ] Connection pooling
   - [ ] Zero-copy where possible
   - [ ] Async channel optimizations
</implementation_checklist>