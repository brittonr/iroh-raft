# iroh-raft

A distributed consensus library that combines Raft consensus with Iroh P2P networking and redb storage, featuring improved ergonomics and comprehensive observability.

## Features

### 🚀 **Core Features**
-  **Simplified RaftCluster API**: Easy-to-use cluster management with minimal boilerplate
-  **Configuration Presets**: Pre-configured settings for Development, Testing, and Production
-  **Generic State Machines**: Type-safe, application-specific command handling
-  **Zero-Copy Optimizations**: High-performance message processing with `Cow<[u8]>` payloads
-  **Connection Pooling**: Sophisticated connection lifecycle management
-  **Graceful Shutdown**: Coordinated shutdown procedures with proper resource cleanup

### 🌐 **Iroh-Native P2P Architecture**
-  **QUIC Streams**: All operations over high-performance P2P connections
-  **Custom ALPN Protocols**: `iroh-raft/0`, `iroh-raft-mgmt/0`, `iroh-raft-discovery/0`
-  **P2P Event Streaming**: Real-time cluster events without WebSockets
-  **Built-in Discovery**: Automatic cluster formation using Iroh's discovery

### 📊 **Observability & Monitoring**
-  **Prometheus Metrics**: Comprehensive metrics export
-  **OpenTelemetry Tracing**: Distributed tracing support
-  **Health Checks**: Configurable health monitoring
-  **Performance Monitoring**: Latency histograms and throughput metrics

### 🔧 **Production Features**
-  **Deployment Management**: Multi-node cluster bootstrap and management
-  **Rolling Updates**: Zero-downtime cluster updates
-  **Dynamic Scaling**: Add/remove nodes during operation
-  **Backup & Restore**: Automated backup scheduling and disaster recovery
-  **Error Handling**: Comprehensive error types and recovery strategies

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
iroh-raft = "0.1.0"
```

### Simple Cluster Setup

```rust
use iroh_raft::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a cluster with minimal configuration
    let mut cluster = RaftCluster::builder()
        .node_id(1)
        .bind_address("127.0.0.1:8080")
        .peers(vec!["127.0.0.1:8081", "127.0.0.1:8082"])
        .state_machine(KeyValueStore::new())
        .preset(Preset::Production)
        .build()
        .await?;

    // Simple operations
    cluster.propose(KvCommand::Set {
        key: "hello".to_string(),
        value: "world".to_string(),
    }).await?;

    let state = cluster.query();
    println!("Value: {:?}", state.get("hello"));

    Ok(())
}
```

### Iroh-Native Management

```rust
use iroh_raft::management::*;

#[tokio::main] 
async fn main() -> Result<()> {
    // Connect to cluster using P2P discovery
    let client = IrohRaftManagementClient::connect_to_cluster("my-cluster").await?;
    
    // All operations over QUIC streams - no HTTP needed!
    let status = client.get_cluster_status().await?;
    println!("Leader: {:?}, Term: {}", status.leader, status.term);
    
    // Add a new member using P2P
    let result = client.add_member(
        4,
        "new-node.example.com:8080".to_string(),
        MemberRole::Voter,
    ).await?;

    // Stream real-time events over P2P
    let mut events = client.subscribe_to_events().await?;
    while let Some(event) = events.next().await {
        match event {
            ClusterEvent::LeaderElected { new_leader, .. } => {
                println!("New leader: {}", new_leader);
            }
            _ => {}
        }
    }

    Ok(())
}
```

## Architecture

### 100% Iroh-Native P2P Design

iroh-raft uses Iroh's P2P primitives exclusively for all operations:

```text
┌─────────────────────────────────────────────────────────────────┐
│                    iroh-raft P2P Architecture                   │
├─────────────────────────────────────────────────────────────────┤
│                     Application Layer                           │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ • Direct library calls for consensus operations         │   │
│  │ • P2P management client for administration              │   │
│  │ • Type-safe interfaces with zero-copy optimizations     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  ┌─────────────────────────────▼─────────────────────────────┐ │
│  │                  P2P Protocol Layer                       │ │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐     │ │
│  │  │ iroh-raft/0  │ │iroh-raft-mgmt│ │iroh-discovery│     │ │
│  │  │ (Consensus)  │ │ (Management) │ │  (Formation) │     │ │
│  │  └──────────────┘ └──────────────┘ └──────────────┘     │ │
│  │         │                │                │               │ │
│  │  ┌──────────────────────────────────────────────────┐    │ │
│  │  │        Iroh QUIC Streams (Zero-Copy)            │    │ │
│  │  │  • Bidirectional streams for RPC                │    │ │
│  │  │  • Automatic encryption & authentication        │    │ │
│  │  │  • Connection pooling & multiplexing            │    │ │
│  │  └──────────────────────────────────────────────────┘    │ │
│  └─────────────────────────────────────────────────────────┘ │
│                              │                                  │
│  ┌─────────────────────────────▼─────────────────────────────┐ │
│  │                  RaftCluster Core                         │ │
│  │  • Consensus Engine    • State Machine    • Events       │ │
│  │  • Leader Election     • Log Replication  • Metrics      │ │
│  └─────────────────────────────────────────────────────────┘ │
│                              │                                  │
│  ┌─────────────────────────────▼─────────────────────────────┐ │
│  │                    Storage Layer                          │ │
│  │  • redb for persistence  • Zero-copy snapshots           │ │
│  │  • Atomic operations     • Efficient compaction          │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

1. **RaftCluster API** - Simplified cluster management interface
2. **Configuration Presets** - Environment-specific configurations (Development/Testing/Production)
3. **Generic State Machines** - Type-safe application logic with custom command types
4. **Management API** - REST endpoints for cluster administration
5. **Observability Stack** - Metrics, tracing, and health monitoring
6. **Transport Layer** - Iroh P2P networking with connection pooling
7. **Storage Backend** - Persistent redb storage with zero-copy optimizations

### Module Structure

```
iroh-raft/
├── src/
│   ├── lib.rs              # Public API exports
│   ├── config.rs           # Configuration with builder pattern
│   ├── error.rs            # Comprehensive error types
│   ├── types.rs            # Core data structures
│   ├── metrics.rs          # Observability and monitoring
│   ├── raft/               # Consensus implementation
│   │   ├── mod.rs          # Generic Raft components
│   │   ├── generic_state_machine.rs  # StateMachine trait
│   │   ├── kv_example.rs   # Example key-value store
│   │   └── proposals.rs    # Generic proposal system
│   ├── storage/            # Persistent storage
│   │   ├── mod.rs          # Storage abstractions  
│   │   ├── simple.rs       # redb implementation
│   │   └── codec.rs        # Serialization utilities
│   └── transport/          # P2P networking
│       ├── mod.rs          # Transport abstractions
│       ├── iroh.rs         # Iroh transport implementation
│       ├── protocol.rs     # Zero-copy message handling
│       └── simple.rs       # Basic transport interface
├── examples/               # Comprehensive examples
│   ├── simple_cluster_example.rs     # Basic usage
│   ├── management_api_example.rs     # REST API demo
│   ├── distributed_kv_store.rs       # Complete application
│   ├── cluster_deployment.rs         # Production deployment
│   ├── monitoring_integration.rs     # Observability
│   └── performance_comparison.rs     # Benchmarks
└── tests/                 # Integration tests
    ├── api_integration_tests.rs      # API testing
    ├── management_api_tests.rs       # REST API tests
    └── madsim_tests.rs              # Distributed simulation
```

## Configuration Presets

iroh-raft provides pre-configured settings for different environments:

```rust
// Development: Fast iterations, verbose logging
let cluster = RaftCluster::builder()
    .preset(Preset::Development)
    .build().await?;

// Testing: Fast timeouts, deterministic behavior  
let cluster = RaftCluster::builder()
    .preset(Preset::Testing)
    .build().await?;

// Production: Conservative timeouts, optimized performance
let cluster = RaftCluster::builder()
    .preset(Preset::Production)
    .build().await?;
```

## Examples

### Basic Key-Value Store

```rust
use iroh_raft::prelude::*;

// Create a distributed key-value store
let mut cluster = RaftCluster::builder()
    .node_id(1)
    .state_machine(KeyValueStore::new())
    .peers(vec!["node2:8080", "node3:8080"])
    .build().await?;

// Write operations go through consensus
cluster.propose(KvCommand::Set {
    key: "user:123".to_string(),
    value: "Alice".to_string(),
}).await?;

// Read operations are served locally
let state = cluster.query();
let user = state.get("user:123");
```

### Custom State Machine

```rust
use iroh_raft::raft::StateMachine;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum BankCommand {
    Transfer { from: String, to: String, amount: u64 },
    CreateAccount { id: String, balance: u64 },
}

struct BankStateMachine {
    accounts: HashMap<String, u64>,
}

impl StateMachine for BankStateMachine {
    type Command = BankCommand;
    type State = HashMap<String, u64>;

    async fn apply_command(&mut self, cmd: Self::Command) -> Result<()> {
        match cmd {
            BankCommand::CreateAccount { id, balance } => {
                self.accounts.insert(id, balance);
            }
            BankCommand::Transfer { from, to, amount } => {
                if let Some(from_balance) = self.accounts.get_mut(&from) {
                    if *from_balance >= amount {
                        *from_balance -= amount;
                        *self.accounts.entry(to).or_insert(0) += amount;
                    }
                }
            }
        }
        Ok(())
    }

    // ... other required methods
}
```

### P2P Management Operations

```rust
// Connect to any cluster member via P2P
let client = IrohRaftManagementClient::connect_to_members(vec![
    (1, node1_addr),
    (2, node2_addr),
    (3, node3_addr),
]).await?;

// Transfer leadership
let result = client.transfer_leadership(Some(2)).await?;
println!("Leadership transferred in {:?}", result.transfer_duration);

// Monitor cluster health
let health = client.get_health_status().await?;
if health.cluster_health.quorum_available {
    println!("Cluster is healthy with {} members", 
             health.cluster_health.healthy_members);
}

// Trigger snapshot
client.trigger_snapshot().await?;

// Create backup
let backup = client.create_backup(BackupMetadata {
    backup_id: "backup_001".to_string(),
    description: "Daily backup".to_string(),
    created_at: SystemTime::now(),
    cluster_state: current_state,
}).await?;
```

### Monitoring Integration

```rust
use iroh_raft::monitoring::*;

let monitoring = MonitoringCoordinator::new(
    "my-service".to_string(),
    "1.0.0".to_string(),
    Duration::from_secs(30), // Export interval
).await;

// Start comprehensive monitoring
monitoring.start_monitoring().await;

// Export Prometheus metrics
let metrics = monitoring.export_metrics_prometheus().await;

// Export Jaeger traces  
let traces = monitoring.export_traces_jaeger().await;

// Get health status
let health = monitoring.get_health_status().await;
```

## Performance

The dual API design provides optimal performance for different use cases:

### Library API Performance
- **Zero-copy optimizations**: Efficient memory usage with `Cow<[u8]>` payloads
- **Direct function calls**: No serialization overhead for library usage
- **Connection pooling**: Reuse connections for better throughput
- **Batching support**: Process multiple operations efficiently

### Benchmarks
```bash
# Run performance comparisons
cargo run --example performance_comparison

# Results (example):
# Library API:  10,000 ops/sec, 2ms median latency
# REST API:     1,000 ops/sec, 15ms median latency  
# Overhead:     10x throughput improvement with library API
```

## Production Deployment

### Cluster Bootstrap

```rust
use iroh_raft::deployment::*;

let config = DeploymentConfig {
    cluster_name: "production-cluster".to_string(),
    environment: Environment::Production,
    nodes: vec![
        NodeConfig {
            node_id: 1,
            hostname: "raft-1.example.com".to_string(),
            port: 8080,
            data_dir: "/var/lib/raft/node1".to_string(),
            resources: ResourceLimits {
                max_memory_mb: 4096,
                max_disk_gb: 100,
                max_connections: 1000,
            },
        },
        // ... more nodes
    ],
    monitoring: MonitoringConfig {
        metrics_port: 9090,
        health_check_interval_seconds: 10,
        alert_endpoints: vec!["slack://alerts", "email://ops@example.com"],
    },
    backup: BackupConfig {
        enabled: true,
        schedule: "0 2 * * *".to_string(), // Daily at 2 AM
        retention_count: 7,
        storage_location: "s3://backups/raft".to_string(),
    },
    // ... other config
};

let mut cluster_manager = ClusterManager::new(config).await?;
cluster_manager.bootstrap().await?;
```

### Rolling Updates

```rust
// Zero-downtime updates
cluster_manager.rolling_update("v2.0.0").await?;
```

### Dynamic Scaling

```rust
// Add nodes during operation
cluster_manager.add_node(new_node_config).await?;

// Remove nodes safely
cluster_manager.remove_node(node_id).await?;
```

## Design Philosophy

The crate follows these principles:

1. **Iroh-Native**: Use Iroh's P2P primitives exclusively - no HTTP dependencies
2. **Developer Experience**: Intuitive APIs with minimal boilerplate
4. **Performance**: Zero-copy optimizations and efficient resource usage
5. **Flexibility**: Generic state machines for any application domain
6. **Reliability**: Comprehensive testing including deterministic simulation
7. **Security**: Built-in encryption and authentication via Iroh's QUIC transport

## Testing

### Comprehensive Test Suite

iroh-raft includes multiple levels of testing to ensure reliability:

#### Integration Tests

```bash
# Run API integration tests
cargo test api_integration_tests

# Run management API tests  
cargo test management_api_tests

# Run all integration tests
cargo test --test '*'
```

#### Performance Benchmarks

```bash
# Compare API performance
cargo run --example performance_comparison

# Run stress tests
cargo test --release -- stress_test
```

#### Deterministic Simulation Tests

Using [madsim](https://github.com/madsim-rs/madsim) for reliable distributed system testing:

```bash
# Run all simulation tests
cargo test --features madsim --test madsim_tests

# Test specific scenarios
cargo test --features madsim --test madsim_tests test_raft_consensus_under_partition

# Run with detailed output
cargo test --features madsim --test madsim_tests -- --nocapture
```

**Test Scenarios Covered:**
- **Network Partitions**: Split-brain prevention and quorum behavior
- **Leader Election**: Deterministic timing and conflict resolution  
- **Message Loss**: Behavior under packet loss and delays
- **Node Failures**: Crash recovery and data consistency
- **Concurrent Operations**: Race conditions and ordering guarantees
- **Performance**: Throughput and latency under various conditions

### Property-Based Testing

```bash
# Run property tests with proptest
cargo test property_tests

# Test with different random seeds
PROPTEST_CASES=10000 cargo test property_tests
```

## API Examples

### Running the Examples

Each example demonstrates different aspects of the library:

```bash
# Basic cluster usage
cargo run --example simple_cluster_example

# REST API and WebSocket events  
cargo run --example management_api_example

# Complete distributed application
cargo run --example distributed_kv_store

# Production deployment patterns
cargo run --example cluster_deployment

# Monitoring and observability
cargo run --example monitoring_integration

# Performance analysis
cargo run --example performance_comparison
```

## Monitoring & Observability

### Prometheus Metrics

Access metrics at `/metrics` endpoint:

```bash
curl http://localhost:9090/metrics
```

**Key Metrics:**
- `raft_current_term` - Current Raft term
- `raft_proposals_total` - Total proposals processed
- `raft_leadership_changes` - Leader election events
- `system_cpu_usage_percent` - CPU utilization
- `app_requests_total` - Application request count

### Jaeger Tracing

Distributed traces for request flow analysis:

```rust
let span_id = monitoring.create_trace_span("process_request").await;
// ... operation ...
monitoring.finish_trace_span(&span_id).await;
```

### Health Checks

Comprehensive health monitoring:

```bash
curl http://localhost:8080/health
```

**Health Checks:**
- Raft consensus participation
- Storage connectivity
- Network reachability  
- Resource utilization
- Application-specific health

## Production Considerations

### Deployment Checklist

- [ ] **Cluster Size**: Use odd number of nodes (3, 5, 7)
- [ ] **Network**: Ensure low-latency, reliable connections
- [ ] **Storage**: Use fast, persistent storage (SSD recommended)
- [ ] **Monitoring**: Set up Prometheus, Grafana, and alerting
- [ ] **Backups**: Configure automated backup scheduling
- [ ] **Security**: Enable TLS and authentication
- [ ] **Resource Limits**: Set appropriate memory and disk limits

### Performance Tuning

```rust
let cluster = RaftCluster::builder()
    .preset(Preset::Production)
    .heartbeat_interval(Duration::from_millis(500))
    .election_timeout(Duration::from_millis(3000))
    .max_log_entries(10000)
    .build().await?;
```

### Scaling Guidelines

- **Horizontal**: Add nodes for fault tolerance (not performance)
- **Vertical**: Increase memory/CPU for higher throughput
- **Reads**: Scale read replicas if needed
- **Writes**: All writes go through leader (inherent bottleneck)

## Migration Guide

### From v0.x to v1.0

```rust
// Old API (v0.x)
let config = ConfigBuilder::new().node_id(1).build()?;
let node = RaftNode::new(config).await?;

// New API (v1.0)  
let cluster = RaftCluster::builder()
    .node_id(1)
    .state_machine(MyStateMachine::new())
    .build().await?;
```

**Key Changes:**
- Unified `RaftCluster` API replaces multiple components
- Built-in state machine support with generic types
- Configuration presets for common scenarios
- Integrated monitoring and health checks

## Contributing

We welcome contributions! Please see our contributing guidelines:

### Development Setup

```bash
git clone https://github.com/example/iroh-raft
cd iroh-raft
cargo build
cargo test
```

### Areas for Contribution

- **Performance optimizations** for high-throughput scenarios
- **Additional state machine examples** for common use cases  
- **Transport improvements** for better network efficiency
- **Documentation and tutorials** for complex scenarios
- **Integration examples** with popular frameworks

### Code Style

- Use `cargo fmt` for formatting
- Run `cargo clippy` for linting
- Add tests for new functionality
- Update documentation for API changes

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

- [Raft consensus algorithm](https://raft.github.io/) by Diego Ongaro and John Ousterhout
- [Iroh](https://github.com/n0-computer/iroh) for P2P networking
- [redb](https://github.com/cberner/redb) for embedded storage
- [madsim](https://github.com/madsim-rs/madsim) for deterministic testing
