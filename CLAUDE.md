# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Build and Test
```bash
# Build the project
cargo build

# Run all tests
cargo test

# Run standard unit and integration tests
cargo test --features test-helpers

# Build with all features
cargo build --all-features

# Check code without building
cargo check

# Format code
cargo fmt

# Run clippy lints
cargo clippy

# Generate documentation
cargo doc --open
```

### Simulation Testing
```bash
# Run deterministic madsim tests (most important for distributed consensus testing)
cargo test --features madsim --test madsim_tests

# Run specific madsim test
cargo test --features madsim --test madsim_tests test_raft_consensus_under_partition

# Run with output for debugging
cargo test --features madsim --test madsim_tests -- --nocapture

# Run property tests
cargo test property_tests
```

### Metrics and Observability
```bash
# Build with metrics support
cargo build --features metrics-otel

# Run tests with metrics enabled
cargo test --features metrics-otel

# Run examples with full observability
cargo run --features metrics-otel --example metrics_shutdown_example

# Build with all observability features
cargo build --features "metrics-otel,tracing"

# Test zero-copy optimizations
cargo run --example zero_copy_example

# Performance benchmarks (when available)
cargo bench --features metrics-otel
```

### Examples
```bash
# Run basic configuration example
cargo run --example simple_example

# Run protocol example
cargo run --example protocol_example

# Test Iroh API integration
cargo run --example test_iroh

# Demonstrate zero-copy optimizations
cargo run --example zero_copy_example

# Show connection pooling configuration
cargo run --example connection_pooling_example

# Demonstrate metrics and graceful shutdown
cargo run --example metrics_shutdown_example

# Run madsim demo
cd examples/madsim_demo && cargo run
```

## Architecture Overview

This is a distributed consensus library that combines Raft consensus with Iroh P2P networking and redb storage.

### Core Components

1. **Configuration** (`config.rs`): Builder pattern configuration with validation
2. **Error Handling** (`error.rs`): Structured error types with snafu and thiserror
3. **Types** (`types.rs`): Core data structures, NodeId, ProposalData
4. **Storage** (`storage/`): Persistent storage backend using redb
   - `simple.rs`: Basic redb implementation (RaftStorage)
   - `codec.rs`: Serialization utilities for Raft data structures
5. **Transport** (`transport/`): P2P networking using Iroh
   - `protocol.rs`: RaftProtocolHandler with ALPN `iroh-raft/0`
   - `iroh.rs`: Iroh-specific transport implementation
   - `simple.rs`: Basic transport abstractions
6. **Raft Module** (`raft/`): Generic Raft consensus implementation
   - `generic_state_machine.rs`: Core StateMachine trait
   - `kv_example.rs`: Example key-value store implementation
   - `proposals.rs`: Generic proposal system with metadata
   - `state_machine.rs`: Wrapper types and command encoding

### Key Design Patterns

- **Generic State Machine**: Applications implement `StateMachine` trait for custom command types
- **Builder Pattern**: ConfigBuilder for comprehensive configuration management
- **Type Safety**: Generic types with SerDe constraints for compile-time guarantees
- **Error Safety**: Comprehensive error handling throughout the stack
- **Zero-Copy Optimizations**: Efficient memory management with `Cow<[u8]>` and `Bytes`
- **Connection Pooling**: Sophisticated connection lifecycle management for performance
- **Graceful Shutdown**: Coordinated shutdown procedures with proper resource cleanup
- **Observability**: Comprehensive metrics collection with OpenTelemetry integration

### Dependencies

- **Raft Core**: Uses `raft` and `raft-proto` crates with prost-codec
- **Storage**: `redb` for persistent storage
- **Networking**: `iroh` for P2P QUIC transport
- **Serialization**: `bincode`, `postcard`, `prost` for different serialization needs
- **Async**: Tokio runtime with async-trait for async interfaces

### Testing Strategy

- **Unit Tests**: Standard cargo test for module-level testing
- **Integration Tests**: Cross-module functionality testing
- **Property Tests**: Using proptest for randomized testing
- **Deterministic Simulation**: madsim for distributed consensus scenarios
  - Network partitions, leader election, message loss, node failures
  - Uses fixed seeds for reproducible results
  - Essential for testing distributed system edge cases

### Module Dependencies

Most development work will involve these key modules:
- `config.rs` for configuration management (includes connection pooling config)
- `metrics.rs` for observability and monitoring
- `raft/` modules for consensus logic and state machines
- `transport/protocol.rs` for network message handling (zero-copy optimizations)
- `transport/iroh.rs` for Iroh P2P transport with connection pooling
- `storage/simple.rs` for data persistence
- `error.rs` for error handling

The transport and storage modules provide pluggable interfaces, allowing different implementations to be swapped in based on requirements.

### New Features and Capabilities

#### Iroh-Native P2P Management
- **100% P2P Architecture**: All operations over QUIC streams, no HTTP dependencies
- **Custom ALPN Protocols**: `iroh-raft/0`, `iroh-raft-mgmt/0`, `iroh-raft-discovery/0`
- **Service Discovery**: Automatic cluster formation using Iroh's P2P discovery
- **P2P Event Streaming**: Real-time cluster events without WebSockets
- **Role-Based Access**: Authentication integrated with Iroh peer IDs

#### Performance Optimizations
- **Zero-Copy Messaging**: `ZeroCopyMessage` struct with `Cow<[u8]>` payloads
- **Streaming Support**: Automatic chunking for large messages (>64KB)
- **Connection Pooling**: Configurable connection reuse with health monitoring
- **Fast Serialization**: Optimized Raft message processing with prost
- **Lock-Free Structures**: Actor patterns and dashmap for zero contention

#### Observability
- **OpenTelemetry Integration**: Comprehensive metrics with `MetricsRegistry`
- **Raft Metrics**: Leader elections, log operations, consensus errors
- **Transport Metrics**: Connection status, message throughput, latencies
- **Storage Metrics**: Read/write operations, disk usage
- **Custom Metrics**: Application-specific measurements

#### Operational Excellence
- **Graceful Shutdown**: Coordinated shutdown with `shutdown()` and `wait_for_shutdown()`
- **Health Monitoring**: Connection health checks and automatic cleanup
- **Configuration Validation**: Comprehensive config validation with builder pattern
- **Resource Management**: Automatic cleanup of connections, memory, and file handles
- **Circuit Breakers**: Fault isolation with adaptive timeout management
- **Backpressure Control**: Flow control for preventing overload