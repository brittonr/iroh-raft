# iroh-raft API Architecture Design

## Overview

This document outlines a comprehensive API architecture for iroh-raft that addresses the identified ergonomics issues while adding a production-ready management layer. The design follows industry best practices from etcd, Consul, and other successful Raft implementations.

## Current Issues Identified

1. **API Ergonomics Problems**:
   - Complex state machine trait with conflicting patterns
   - Lack of high-level cluster abstraction
   - Configuration complexity without sensible defaults
   - Missing administrative operations

2. **Management Interface Gaps**:
   - No REST API for cluster management
   - Limited health checking and monitoring
   - No standardized administrative operations
   - Difficult operational monitoring

3. **Developer Experience Issues**:
   - Too many low-level concepts exposed
   - No clear migration path for simple use cases
   - Complex setup for common scenarios

## New Architecture

### Layer 1: Core Library API (High Performance)

The foundational layer provides high-performance, low-level access for applications that need maximum control and performance.

#### 1.1 Unified State Machine API

```rust
// New simplified trait that replaces the current complex patterns
pub trait StateMachine: Send + Sync + 'static {
    type Command: Serialize + DeserializeOwned + Send + Sync + Clone;
    type State: Serialize + DeserializeOwned + Clone + Send + Sync;
    type Error: Error + Send + Sync + 'static;

    // Single apply method (no more confusion between different apply patterns)
    async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    
    // Snapshot operations
    async fn snapshot(&self) -> Result<Self::State, Self::Error>;
    async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error>;
    
    // Read-only access
    fn query(&self) -> &Self::State;
}
```

#### 1.2 Cluster API (High-Level Abstraction)

```rust
// Main entry point for applications
pub struct RaftCluster<SM: StateMachine> {
    // Internal implementation hidden
}

impl<SM: StateMachine> RaftCluster<SM> {
    // Easy cluster creation with builder
    pub fn builder() -> ClusterBuilder<SM>;
    
    // Core operations
    pub async fn propose(&self, command: SM::Command) -> Result<(), RaftError>;
    pub async fn query(&self) -> &SM::State;
    
    // Administrative operations
    pub async fn add_node(&self, node_id: NodeId, address: SocketAddr) -> Result<(), RaftError>;
    pub async fn remove_node(&self, node_id: NodeId) -> Result<(), RaftError>;
    pub async fn transfer_leadership(&self, to_node: NodeId) -> Result<(), RaftError>;
    
    // Health and status
    pub async fn health(&self) -> ClusterHealth;
    pub async fn status(&self) -> ClusterStatus;
    pub fn metrics(&self) -> &MetricsRegistry;
    
    // Lifecycle
    pub async fn shutdown(&self) -> Result<(), RaftError>;
    pub async fn wait_for_shutdown(&self);
}
```

#### 1.3 Simplified Configuration

```rust
// Builder with sensible defaults and validation
pub struct ClusterBuilder<SM> {
    config: Config,
    _phantom: PhantomData<SM>,
}

impl<SM: StateMachine> ClusterBuilder<SM> {
    pub fn new(node_id: NodeId) -> Self;
    
    // Essential configuration (most have good defaults)
    pub fn bind_address(self, addr: impl Into<SocketAddr>) -> Self;
    pub fn join_cluster(self, peers: Vec<SocketAddr>) -> Self;
    pub fn data_dir(self, path: impl Into<PathBuf>) -> Self;
    
    // Advanced configuration (optional)
    pub fn election_timeout(self, timeout: Duration) -> Self;
    pub fn heartbeat_interval(self, interval: Duration) -> Self;
    pub fn max_log_entries(self, max: u64) -> Self;
    
    // Feature flags
    pub fn enable_metrics(self, addr: Option<SocketAddr>) -> Self;
    pub fn enable_management_api(self, addr: SocketAddr) -> Self;
    pub fn enable_tracing(self, endpoint: String) -> Self;
    
    // Build the cluster
    pub async fn build(self, state_machine: SM) -> Result<RaftCluster<SM>, RaftError>;
}
```

### Layer 2: Management API (REST/JSON)

A RESTful HTTP API layer for cluster administration, monitoring, and operations. This layer is optional and feature-gated.

#### 2.1 Resource Model

```
Cluster
├── Nodes (cluster members)
├── Logs (raft log entries)  
├── Config (cluster configuration)
├── Health (cluster health status)
└── Metrics (operational metrics)
```

#### 2.2 HTTP Endpoints

**Cluster Management**:
- `GET /api/v1/cluster` - Get cluster status
- `GET /api/v1/cluster/health` - Health check
- `POST /api/v1/cluster/leadership/transfer` - Transfer leadership

**Node Management**:
- `GET /api/v1/nodes` - List cluster nodes
- `POST /api/v1/nodes` - Add node to cluster
- `DELETE /api/v1/nodes/{id}` - Remove node from cluster
- `GET /api/v1/nodes/{id}` - Get node details

**Configuration**:
- `GET /api/v1/config` - Get cluster configuration
- `PUT /api/v1/config` - Update cluster configuration

**Monitoring**:
- `GET /api/v1/metrics` - Prometheus metrics
- `GET /api/v1/logs` - Recent log entries
- `GET /api/v1/events` - Server-sent events stream

#### 2.3 Authentication and Authorization

```rust
// Pluggable auth system
pub trait AuthProvider: Send + Sync + 'static {
    async fn authenticate(&self, request: &HttpRequest) -> Result<Principal, AuthError>;
    async fn authorize(&self, principal: &Principal, operation: &Operation) -> Result<(), AuthError>;
}

// Built-in implementations
pub struct BearerTokenAuth { /* ... */ }
pub struct MutualTlsAuth { /* ... */ }
pub struct NoAuth; // For development
```

### Layer 3: Administrative Tools

#### 3.1 CLI Tool

```bash
# Cluster operations
iroh-raft cluster status
iroh-raft cluster health
iroh-raft cluster list-nodes

# Node operations  
iroh-raft node add <node-id> <address>
iroh-raft node remove <node-id>
iroh-raft node status <node-id>

# Configuration
iroh-raft config show
iroh-raft config validate <file>

# Monitoring
iroh-raft metrics
iroh-raft logs --follow
```

#### 3.2 Web Dashboard

React-based web interface for cluster monitoring and administration:
- Real-time cluster status
- Node health visualization
- Metrics dashboards
- Log streaming
- Configuration management

## Error Handling Strategy

### 1. Structured Error Types

```rust
// Hierarchical error structure
#[derive(Debug, thiserror::Error)]
pub enum RaftError {
    #[error("Configuration error: {0}")]
    Configuration(#[from] ConfigError),
    
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    #[error("Consensus error: {component}: {message}")]
    Consensus { component: String, message: String },
    
    #[error("State machine error: {0}")]
    StateMachine(Box<dyn Error + Send + Sync>),
}
```

### 2. HTTP Error Mapping

```rust
// Consistent HTTP error responses
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<Value>,
    pub timestamp: String,
    pub request_id: String,
}
```

## Performance Considerations

### 1. Feature Gates

```toml
[features]
default = ["metrics"]

# Core features
metrics = ["opentelemetry", "prometheus"]
tracing = ["opentelemetry-jaeger"]

# Optional management layer
management-api = ["axum", "tower", "serde_json"]
web-dashboard = ["management-api", "static-files"]

# Development features  
dev-tools = ["management-api", "web-dashboard"]
```

### 2. Zero-Copy Optimization

The management API layer preserves the zero-copy optimizations in the core library by:
- Using streaming for large responses
- Avoiding unnecessary serialization
- Efficient metrics collection without blocking

### 3. Connection Pooling

HTTP management API reuses the existing connection pooling infrastructure for communicating with cluster nodes.

## Migration Strategy

### Phase 1: Core API Improvements
1. Implement unified StateMachine trait
2. Create RaftCluster high-level API
3. Add ClusterBuilder with sensible defaults
4. Maintain backward compatibility with feature flags

### Phase 2: Management API
1. Implement REST endpoints
2. Add authentication/authorization
3. Create OpenAPI specification
4. Add health checking infrastructure

### Phase 3: Administrative Tools
1. Build CLI tool
2. Create web dashboard
3. Add monitoring integrations
4. Documentation and examples

## Backward Compatibility

- Old APIs available under `legacy` feature flag
- Clear migration guide with examples
- Deprecation warnings with suggested alternatives
- Support for gradual migration

## Security Considerations

### 1. Network Security
- TLS/mTLS for all communications
- Certificate-based node authentication
- Network segmentation support

### 2. API Security
- Rate limiting on management endpoints
- Input validation and sanitization
- Audit logging for administrative operations
- Role-based access control

### 3. Storage Security
- Encryption at rest support
- Secure key management
- File permission controls

## Observability Integration

### 1. Structured Logging
```rust
// Consistent logging across all components
tracing::info!(
    cluster.id = %cluster_id,
    node.id = %node_id,
    operation = "leadership_transfer",
    "Leadership transferred successfully"
);
```

### 2. Metrics
```rust
// Comprehensive metrics collection
pub struct ClusterMetrics {
    pub leadership_changes: Counter,
    pub proposal_latency: Histogram,
    pub node_health: Gauge,
    pub network_bytes: Counter,
}
```

### 3. Tracing
- Distributed tracing across cluster operations
- Request correlation IDs
- Performance profiling support

## Example Usage

### Simple Application

```rust
use iroh_raft::prelude::*;

#[derive(Serialize, Deserialize, Clone)]
enum CounterCommand {
    Increment(u64),
    Decrement(u64),
}

#[derive(Serialize, Deserialize, Clone)]
struct CounterState {
    value: u64,
}

struct CounterStateMachine {
    state: CounterState,
}

impl StateMachine for CounterStateMachine {
    type Command = CounterCommand;
    type State = CounterState;
    type Error = std::io::Error;

    async fn apply(&mut self, command: Self::Command) -> Result<(), Self::Error> {
        match command {
            CounterCommand::Increment(n) => self.state.value += n,
            CounterCommand::Decrement(n) => self.state.value = self.state.value.saturating_sub(n),
        }
        Ok(())
    }

    async fn snapshot(&self) -> Result<Self::State, Self::Error> {
        Ok(self.state.clone())
    }

    async fn restore(&mut self, snapshot: Self::State) -> Result<(), Self::Error> {
        self.state = snapshot;
        Ok(())
    }

    fn query(&self) -> &Self::State {
        &self.state
    }
}

#[tokio::main]
async fn main() -> Result<(), RaftError> {
    let cluster = RaftCluster::builder(1)
        .bind_address("127.0.0.1:8080")
        .join_cluster(vec!["127.0.0.1:8081".parse().unwrap()])
        .enable_metrics(Some("127.0.0.1:9090".parse().unwrap()))
        .enable_management_api("127.0.0.1:8090".parse().unwrap())
        .build(CounterStateMachine {
            state: CounterState { value: 0 }
        })
        .await?;

    // Simple operations
    cluster.propose(CounterCommand::Increment(5)).await?;
    let state = cluster.query().await;
    println!("Counter value: {}", state.value);

    // Admin operations
    let health = cluster.health().await;
    println!("Cluster health: {:?}", health);

    cluster.shutdown().await?;
    Ok(())
}
```

This architecture provides a clear separation of concerns, excellent developer experience, and production-ready management capabilities while preserving the high-performance characteristics of the core library.