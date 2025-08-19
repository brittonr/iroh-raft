# iroh-raft

A simplified distributed consensus library that combines Raft consensus with Iroh P2P networking and redb storage.

## Status

This crate provides a **minimal but functional** structure for building distributed consensus systems. The core components are in place but some implementations may need additional refinement for production use.

## Features

- ✅ **Configuration Management**: Comprehensive configuration system with builder pattern
- ✅ **Error Handling**: Well-structured error types for all failure modes
- ✅ **Core Types**: Basic data structures for proposals, node IDs, VM management
- ✅ **Storage Interface**: Simplified redb-based storage implementation for Raft
- ✅ **Transport Interface**: Basic Iroh P2P transport layer
- ✅ **Compilation**: The crate compiles with warnings (mostly documentation)

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
iroh-raft = "0.1.0"
```

Basic usage:

```rust
use iroh_raft::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Create configuration
    let config = ConfigBuilder::new()
        .node_id(1)
        .data_dir("/tmp/raft-node-1")
        .build()?;
    
    println!("Node configuration created for ID: {:?}", config.node.id);
    
    Ok(())
}
```

## Architecture

### Core Components

1. **Configuration** (`config.rs`): Comprehensive configuration management with validation
2. **Error Handling** (`error.rs`): Structured error types for all failure modes  
3. **Types** (`types.rs`): Core data structures and proposal formats
4. **Storage** (`storage/`): Persistent storage backend using redb
5. **Transport** (`transport/`): P2P networking layer using Iroh

### Module Structure

```
iroh-raft/
├── src/
│   ├── config.rs          # Configuration management
│   ├── error.rs           # Error types
│   ├── types.rs           # Core data structures
│   ├── storage/
│   │   ├── mod.rs         # Storage module exports
│   │   ├── simple.rs      # Basic redb storage implementation
│   │   └── codec.rs       # Serialization utilities
│   └── transport/
│       ├── mod.rs         # Transport module exports
│       └── simple.rs      # Basic Iroh transport implementation
```

## Development Status

This crate represents a **working foundation** that applications can build upon. Key aspects:

### ✅ What Works
- **Clean compilation** with warnings (mostly missing documentation)
- **Comprehensive error handling** with proper error categorization
- **Builder pattern configuration** with validation
- **Basic storage and transport abstractions**
- **Type-safe interfaces** for Raft operations

### 🔧 What Needs Work
- Transport implementation needs peer management refinement
- Storage implementation could use more optimization
- Message serialization needs protocol buffer integration
- Documentation needs to be completed
- Integration testing needs expansion

## Design Philosophy

The crate follows these principles:

1. **Minimal Dependencies**: Only essential dependencies for core functionality
2. **Clean Abstractions**: Well-defined traits and interfaces
3. **Error Safety**: Comprehensive error handling throughout
4. **Type Safety**: Leverage Rust's type system for correctness
5. **Testability**: Mockable components for testing

## Testing

### Deterministic Simulation Tests

The crate includes comprehensive deterministic simulation tests using [madsim](https://github.com/madsim-rs/madsim) to test distributed consensus scenarios that are difficult to test reliably with traditional testing methods.

#### Running Simulation Tests

```bash
# Run all madsim simulation tests
cargo test --features madsim --test madsim_tests

# Run specific simulation test
cargo test --features madsim --test madsim_tests test_raft_consensus_under_partition

# Run with output to see test progress
cargo test --features madsim --test madsim_tests -- --nocapture
```

#### Test Scenarios Covered

- **Network Partitions**: Test Raft behavior under various network partition scenarios
- **Leader Election**: Deterministic leader election with controlled timing
- **Message Loss**: Behavior under message loss and network unreliability
- **Node Failures**: Node crash and recovery scenarios
- **Concurrent Operations**: Multiple concurrent proposals and leadership changes
- **Split-Brain Prevention**: Ensuring no split-brain scenarios occur
- **Log Replication**: Proper log replication with failures and recovery
- **Invariant Safety**: Verification of core Raft safety properties

All tests use fixed seeds for deterministic execution, making them reproducible and reliable for CI/CD pipelines.

### Regular Tests

```bash
# Run standard unit and integration tests
cargo test

# Run with test helpers
cargo test --features test-helpers
```

## Contributing

This crate provides a solid foundation for distributed consensus applications. Areas where contributions would be valuable:

- Improving transport reliability and performance
- Adding comprehensive integration tests
- Completing documentation
- Optimizing storage operations
- Adding more sophisticated peer management

## License

MIT OR Apache-2.0