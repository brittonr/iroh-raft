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

## Future Improvements

- Add connection pooling for better performance
- Implement message batching for efficiency
- Add metrics and observability
- Support for prioritized message delivery