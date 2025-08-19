# Iroh-Raft Protocol Implementation

## Overview

This crate implements a Raft consensus protocol using Iroh's P2P networking stack. The implementation follows Iroh's protocol patterns for proper connection handling and message passing.

## Key Components

### 1. Protocol Handler (`src/transport/protocol.rs`)

The `RaftProtocolHandler` implements the protocol for handling Raft messages over Iroh connections:

- **ALPN**: Uses `iroh-raft/0` as the protocol identifier
- **Message Types**: Supports Request, Response, RaftMessage, Heartbeat, and Error message types
- **RPC Pattern**: Implements an RPC-style communication pattern for Raft messages

### 2. Transport Module

The transport module provides:
- Protocol definition and message serialization
- Connection management
- Integration with the Raft consensus engine

### 3. Protocol Features

- **Bidirectional Streams**: Uses Iroh's bidirectional streams for request/response patterns
- **Message Serialization**: Uses bincode for efficient binary serialization
- **Error Handling**: Comprehensive error handling with proper error types
- **Async/Await**: Fully async implementation using Tokio

## Usage Example

```rust
use iroh::endpoint::Endpoint;
use iroh_raft::transport::{RaftProtocolHandler, RAFT_ALPN};
use tokio::sync::mpsc;

// Create an endpoint
let endpoint = Endpoint::builder()
    .alpns(vec![RAFT_ALPN.to_vec()])
    .bind()
    .await?;

// Create the protocol handler
let (raft_tx, raft_rx) = mpsc::unbounded_channel();
let handler = RaftProtocolHandler::new(node_id, raft_tx);

// Accept connections
while let Some(incoming) = endpoint.accept().await {
    let connection = incoming.accept()?.await?;
    handler.accept(connection).await?;
}
```

## Protocol Flow

1. **Connection Establishment**: Nodes connect using Iroh's QUIC-based transport with the `iroh-raft/0` ALPN
2. **Message Exchange**: Raft messages are serialized and sent over bidirectional streams
3. **RPC Pattern**: Each request gets a response for delivery confirmation
4. **Error Handling**: Errors are properly propagated and logged

## Integration with Raft

The protocol handler integrates with the Raft consensus engine by:
- Receiving messages from remote nodes and forwarding them to the Raft state machine
- Sending outgoing Raft messages to peer nodes
- Managing connection lifecycle and health checks

## Testing

Run the protocol example to test the implementation:

```bash
cargo run --example protocol_example
```

This demonstrates:
- Setting up an Iroh endpoint with the Raft protocol
- Accepting incoming connections
- Processing Raft messages

## Advanced Features

### 4. Connection Pooling Architecture

The protocol handler now supports sophisticated connection pooling for improved performance:

- **Pool Management**: Automatic connection lifecycle management with configurable pool sizes
- **Connection Reuse**: Efficient reuse of existing connections to minimize connection overhead
- **Health Monitoring**: Automatic detection and cleanup of stale or failed connections
- **Load Balancing**: Intelligent distribution of messages across available connections

#### Connection Pool Configuration

```rust
use iroh_raft::config::{ConfigBuilder, ConnectionConfig};
use std::time::Duration;

let config = ConfigBuilder::new()
    .node_id(1)
    .data_dir("./data")
    .bind_address("127.0.0.1:8080")
    .connection_config(ConnectionConfig {
        enable_pooling: true,
        max_connections_per_peer: 5,
        connect_timeout: Duration::from_secs(10),
        keep_alive_interval: Duration::from_secs(30),
        idle_timeout: Duration::from_secs(300),
        max_message_size: 1024 * 1024, // 1MB
    })
    .build()?;
```

#### Connection Metrics

The transport layer provides detailed metrics about connection pool performance:

```rust
use iroh_raft::metrics::MetricsRegistry;

let metrics = MetricsRegistry::new("my-raft-cluster")?;
let transport_metrics = metrics.transport_metrics();

// Connection metrics are automatically recorded
transport_metrics.record_connection_established("peer-node-id");
transport_metrics.record_connection_closed("peer-node-id", duration);
```

### 5. Zero-Copy Message Flow

The protocol implements zero-copy optimizations for high-performance message handling:

- **Zero-Copy Structures**: Uses `Cow<[u8]>` and `Bytes` for efficient memory management
- **Streaming Support**: Automatic streaming for large messages (>64KB) to prevent memory spikes
- **Buffer Reuse**: Pre-allocated buffers and efficient serialization paths
- **Fast Paths**: Optimized processing for common message types like heartbeats

#### Zero-Copy Message Processing

```rust
use iroh_raft::transport::protocol::{ZeroCopyMessage, MessageType};

// Create zero-copy message with borrowed data
let payload = b"raft message data";
let message = ZeroCopyMessage::new_borrowed(
    MessageType::RaftMessage,
    Some(request_id),
    payload
);

// Automatic streaming for large messages
if message.is_large_message() {
    // Uses chunked I/O automatically
    write_message_zero_copy(stream, &message).await?;
}
```

### 6. Graceful Shutdown Procedures

The transport layer supports graceful shutdown with proper cleanup:

- **Shutdown Signaling**: Uses watch channels for coordinated shutdown across components
- **Connection Cleanup**: Graceful closure of all active connections with proper notifications
- **Task Termination**: Coordinated shutdown of background tasks and handlers
- **Resource Cleanup**: Proper cleanup of file handles, network resources, and memory

#### Implementing Graceful Shutdown

```rust
use iroh_raft::transport::iroh::IrohRaftTransport;

// Create transport with shutdown capability
let transport = IrohRaftTransport::new(config).await?;

// In your main application loop
tokio::select! {
    _ = shutdown_signal() => {
        println!("Shutdown signal received");
        transport.shutdown().await;
        println!("Transport shutdown complete");
    }
    _ = transport.run() => {
        println!("Transport terminated");
    }
}

// Wait for complete shutdown
transport.wait_for_shutdown().await;
```

### 7. Metrics Integration

Comprehensive OpenTelemetry metrics integration provides observability:

- **Raft Metrics**: Leader elections, log operations, consensus errors
- **Transport Metrics**: Connection status, message throughput, network latencies
- **Storage Metrics**: Read/write operations, disk usage, compaction statistics
- **Custom Metrics**: Application-specific measurements and KPIs

#### Metrics Configuration

```rust
use iroh_raft::metrics::MetricsRegistry;

// Enable metrics collection
let metrics = MetricsRegistry::new("my-raft-cluster")?;

// Record Raft operations
let raft_metrics = metrics.raft_metrics();
raft_metrics.record_leader_election(node_id, term);
raft_metrics.record_log_append(entries_count, latency_ms);

// Export for Prometheus scraping
let prometheus_data = metrics.export_prometheus()?;
println!("{}", prometheus_data);
```

## Future Improvements

- SIMD operations for large payload processing
- Custom allocators for message batching scenarios
- Ring buffers for lock-free high-throughput operations
- Optional compression for large payloads