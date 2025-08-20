# Integration Strategy for iroh-raft API Architecture

## Overview

This document outlines the strategy for integrating the new API architecture into the existing iroh-raft codebase. The approach is designed to be incremental, maintaining backward compatibility while providing a clear migration path to the improved APIs.

## Feature Flag Organization

### Core Feature Flags

```toml
[features]
# Default features for most users
default = ["metrics", "cluster-api"]

# Core library features
metrics = ["opentelemetry", "prometheus"]
tracing = ["opentelemetry-jaeger"] 
cluster-api = [] # New high-level API

# Optional management layer
management-api = ["axum", "tower", "serde_json", "cluster-api"]
web-dashboard = ["management-api", "static-files"]

# Development and testing
dev-tools = ["management-api", "web-dashboard"]
test-helpers = []

# Legacy support (deprecated)
legacy-api = [] # Maintains old StateMachine trait
```

### Feature Dependencies

```
cluster-api (New high-level API)
├── Core library components
├── Unified StateMachine trait
└── ClusterBuilder with defaults

management-api
├── cluster-api (required)
├── REST endpoints
├── Authentication/authorization
└── Health checking

web-dashboard  
├── management-api (required)
├── React frontend
├── Real-time monitoring
└── Administrative interface

legacy-api
├── Old StateMachine traits
├── Deprecated configuration patterns
└── Compatibility shims
```

## Migration Phases

### Phase 1: Core API Unification (Weeks 1-2)

**Goals:**
- Introduce unified StateMachine trait
- Create RaftCluster high-level API
- Add ClusterBuilder with sensible defaults
- Maintain full backward compatibility

**Tasks:**

1. **Create new cluster API module** ✅
   - `src/cluster_api.rs` with unified interfaces
   - Feature-gated to avoid affecting existing code
   - Comprehensive documentation and examples

2. **Update library exports**
   ```rust
   // In src/lib.rs
   #[cfg(feature = "cluster-api")]
   pub mod cluster;
   
   #[cfg(feature = "cluster-api")]
   pub use cluster::{RaftCluster, StateMachine, ClusterBuilder};
   
   // Legacy exports (deprecated but available)
   #[cfg(feature = "legacy-api")]
   pub use crate::raft::{
       generic_state_machine::StateMachine as LegacyStateMachine,
       state_machine::ExampleRaftStateMachine,
   };
   ```

3. **Add configuration validation**
   - Enhance `ConfigBuilder` with better defaults
   - Add comprehensive validation with helpful error messages
   - Support for environment variable overrides

4. **Create migration examples**
   - Side-by-side examples showing old vs new APIs
   - Migration guide with step-by-step instructions
   - Performance comparison demonstrations

**Deliverables:**
- New cluster API module
- Updated examples demonstrating new API
- Migration guide documentation
- All existing tests continue to pass

### Phase 2: Management API Implementation (Weeks 3-4)

**Goals:**
- Implement REST API endpoints
- Add authentication and authorization
- Create health checking infrastructure
- Build monitoring capabilities

**Tasks:**

1. **Implement REST endpoints**
   ```rust
   // Feature-gated REST API implementation
   #[cfg(feature = "management-api")]
   mod management {
       use axum::{routing::get, Router};
       
       pub fn create_router<SM: StateMachine>() -> Router<RaftCluster<SM>> {
           Router::new()
               .route("/api/v1/cluster", get(handlers::get_cluster_status))
               .route("/api/v1/cluster/health", get(handlers::get_health))
               // ... other routes
       }
   }
   ```

2. **Add authentication providers**
   ```rust
   pub trait AuthProvider: Send + Sync + 'static {
       async fn authenticate(&self, request: &HttpRequest) -> Result<Principal, AuthError>;
       async fn authorize(&self, principal: &Principal, operation: &Operation) -> Result<(), AuthError>;
   }
   
   // Built-in implementations
   pub struct BearerTokenAuth { /* ... */ }
   pub struct MutualTlsAuth { /* ... */ }
   pub struct NoAuth; // Development only
   ```

3. **Create health checking system**
   - Consensus health checks
   - Storage health checks  
   - Network connectivity checks
   - Custom application health checks

4. **Add metrics collection**
   - Extend existing metrics system
   - Add HTTP endpoint metrics
   - Cluster-level operational metrics

**Deliverables:**
- Complete REST API implementation
- Authentication/authorization system
- Health checking infrastructure
- OpenAPI 3.0 specification
- Postman/curl examples

### Phase 3: Administrative Tools (Weeks 5-6)

**Goals:**
- Build CLI tool for cluster management
- Create web dashboard for monitoring
- Add operational runbooks
- Complete documentation

**Tasks:**

1. **CLI tool implementation**
   ```bash
   # Install CLI tool
   cargo install iroh-raft-cli
   
   # Basic operations
   iroh-raft cluster status --endpoint http://localhost:8090
   iroh-raft node add 2 192.168.1.100:8080
   iroh-raft health check
   ```

2. **Web dashboard**
   - React-based single-page application
   - Real-time cluster visualization
   - Metrics dashboards with charts
   - Administrative operations interface

3. **Monitoring integration**
   - Prometheus metrics export
   - Grafana dashboard templates
   - Alert rule templates
   - Log aggregation guides

4. **Documentation completion**
   - Complete API reference
   - Operational runbooks
   - Troubleshooting guides
   - Best practices documentation

**Deliverables:**
- CLI tool with package distribution
- Web dashboard with deployment guide
- Monitoring templates and guides
- Complete documentation suite

## Backward Compatibility Strategy

### Compatibility Matrix

| Component | Current API | New API | Compatibility |
|-----------|-------------|---------|---------------|
| StateMachine trait | Multiple conflicting patterns | Unified trait | Via legacy feature |
| Configuration | Complex manual setup | Builder with defaults | Full compatibility |
| Cluster creation | Manual component assembly | High-level builder | New API only |
| State queries | Direct trait access | Cluster.query() | Via adapter |
| Administrative ops | None | REST API + CLI | New functionality |

### Legacy Support Approach

1. **Deprecation Warnings**
   ```rust
   #[deprecated(since = "0.2.0", note = "Use iroh_raft::cluster::StateMachine instead")]
   pub trait StateMachine {
       // Old trait definition
   }
   ```

2. **Adapter Pattern**
   ```rust
   // Adapter to use old state machines with new cluster API
   pub struct LegacyStateMachineAdapter<T> {
       inner: T,
   }
   
   impl<T: LegacyStateMachine> cluster::StateMachine for LegacyStateMachineAdapter<T> {
       // Adapt old interface to new
   }
   ```

3. **Feature Flag Controls**
   ```toml
   # Gradual migration approach
   [dependencies]
   iroh-raft = { version = "0.2", features = ["cluster-api", "legacy-api"] }
   ```

### Migration Path

1. **Week 1-2**: Legacy and new APIs coexist
2. **Week 3-4**: New APIs stable, migration guide published
3. **Week 5-6**: Legacy APIs marked deprecated
4. **Version 0.3**: Legacy APIs removed (with clear upgrade path)

## Integration with Existing Components

### Storage Layer Integration

```rust
// The new cluster API uses existing storage components
impl<SM: StateMachine> RaftCluster<SM> {
    async fn build(builder: ClusterBuilder<SM>) -> Result<Self, RaftError> {
        // Use existing storage implementations
        let storage = RaftStorage::new(&builder.config.storage)?;
        
        // Use existing transport layer
        let transport = RaftProtocolHandler::new(&builder.config.transport)?;
        
        // Integrate with existing metrics
        let metrics = MetricsRegistry::new(&builder.config.observability.metrics)?;
        
        // Build cluster with existing components
        Self::new_with_components(storage, transport, metrics, builder.state_machine)
    }
}
```

### Transport Layer Integration

```rust
// Management API reuses existing transport infrastructure
#[cfg(feature = "management-api")]
impl<SM: StateMachine> RaftCluster<SM> {
    async fn start_management_api(&self, addr: SocketAddr) -> Result<(), RaftError> {
        // Leverage existing connection pooling
        let transport = &self.inner.transport;
        
        // Use existing authentication if configured
        let auth = &self.inner.config.transport.auth;
        
        // Start HTTP server with existing infrastructure
        start_http_server(addr, self.clone(), transport, auth).await
    }
}
```

### Metrics Integration

```rust
// Extend existing metrics with new cluster-level metrics
#[cfg(feature = "metrics-otel")]
impl MetricsRegistry {
    pub fn register_cluster_metrics(&mut self) -> Result<ClusterMetrics, MetricsError> {
        ClusterMetrics::new(self)
    }
}

pub struct ClusterMetrics {
    pub proposals_total: Counter,
    pub leadership_changes: Counter,
    pub node_health: Gauge,
    pub api_requests: Histogram,
}
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cluster_api_compatibility() {
        // Test that new API can use old state machines via adapter
        let legacy_sm = LegacyKvStateMachine::new();
        let adapter = LegacyStateMachineAdapter::new(legacy_sm);
        
        let cluster = RaftCluster::builder(1)
            .build(adapter)
            .await
            .expect("Should build with legacy adapter");
            
        // Verify functionality works
        cluster.propose(Command::Set("key".into(), "value".into())).await.unwrap();
        let state = cluster.query().await;
        assert_eq!(state.get("key"), Some(&"value".to_string()));
    }
    
    #[tokio::test]
    async fn test_management_api_endpoints() {
        // Test all REST endpoints
    }
}
```

### Integration Tests

```rust
// Test new API with existing transport and storage
#[tokio::test]
async fn test_full_cluster_integration() {
    let cluster = RaftCluster::builder(1)
        .bind_address("127.0.0.1:0".parse().unwrap())
        .join_cluster(vec!["127.0.0.1:8081".parse().unwrap()])
        .enable_management_api("127.0.0.1:9090".parse().unwrap())
        .build(TestStateMachine::new())
        .await
        .expect("Should build cluster");
        
    // Test proposal path through existing Raft implementation
    // Test storage persistence with existing storage layer
    // Test network communication with existing transport
}
```

### Backward Compatibility Tests

```rust
#[tokio::test]
async fn test_legacy_api_still_works() {
    // Ensure old examples and tests continue to pass
    use iroh_raft::raft::{ExampleRaftStateMachine, KvCommand};
    
    let mut sm = ExampleRaftStateMachine::new_kv_store();
    sm.inner_mut().apply_command(KvCommand::Set {
        key: "test".to_string(),
        value: "value".to_string(),
    }).await.unwrap();
    
    // Should work exactly as before
}
```

## Documentation Strategy

### API Documentation

1. **Separate documentation sections**:
   - "Getting Started" - New high-level API
   - "Advanced Usage" - Low-level library APIs
   - "Migration Guide" - Moving from old to new APIs
   - "Legacy APIs" - Deprecated but documented

2. **Example organization**:
   ```
   examples/
   ├── cluster_api/          # New high-level examples
   │   ├── simple_counter.rs
   │   ├── distributed_kv.rs
   │   └── production_setup.rs
   ├── management_api/       # REST API examples
   │   ├── health_monitoring.rs
   │   └── administrative_ops.rs
   ├── migration/           # Migration examples
   │   ├── before_and_after.rs
   │   └── gradual_migration.rs
   └── legacy/              # Old examples (deprecated)
       └── legacy_usage.rs
   ```

### Migration Documentation

```markdown
# Migration Guide: From Legacy to Cluster API

## Before (Legacy API)
```rust
// Complex setup with manual component wiring
let config = ConfigBuilder::new()
    .node_id(1)
    .build()?;

let storage = RaftStorage::new(&config.storage)?;
let transport = IrohTransport::new(&config.transport)?;
let state_machine = GenericRaftStateMachine::new(MyStateMachine::new());

// Manual assembly required
let node = RaftNode::new(config, storage, transport, state_machine)?;
```

## After (New Cluster API)
```rust
// Simple, ergonomic setup
let cluster = RaftCluster::builder(1)
    .bind_address("127.0.0.1:8080")
    .join_cluster(vec!["127.0.0.1:8081".parse()?])
    .enable_management_api("127.0.0.1:9090".parse()?)
    .build(MyStateMachine::new())
    .await?;
```
```

## Risk Mitigation

### Compatibility Risks

1. **API Breaking Changes**
   - Mitigation: Feature flags maintain old APIs
   - Testing: Comprehensive backward compatibility test suite
   - Documentation: Clear migration paths

2. **Performance Regression**
   - Mitigation: New APIs are thin wrappers over existing code
   - Testing: Performance benchmarks comparing old vs new
   - Monitoring: Built-in performance metrics

3. **Feature Creep**
   - Mitigation: Incremental rollout with clear milestones
   - Review: Each phase reviewed before proceeding
   - Scope: Management API is optional feature

### Rollback Strategy

1. **Phase 1 Rollback**: Remove cluster-api feature flag
2. **Phase 2 Rollback**: Disable management-api feature
3. **Phase 3 Rollback**: Remove CLI and dashboard packages

Each phase is independently reversible without affecting core functionality.

## Success Metrics

### Developer Experience
- Reduced lines of code for common use cases (target: 50% reduction)
- Time to first working cluster (target: <5 minutes)
- Documentation completeness (target: 100% API coverage)

### Operational Excellence
- Health check coverage (target: 100% of critical components)
- Management API response times (target: <100ms p95)
- CLI tool usability (target: intuitive for common operations)

### Adoption
- Migration rate from legacy APIs (track over 6 months)
- Community feedback and issue reports
- Production deployment success rate

## Timeline Summary

| Week | Phase | Deliverables | Success Criteria |
|------|-------|--------------|------------------|
| 1-2  | Core API | Unified APIs, ClusterBuilder | All tests pass, examples work |
| 3-4  | Management | REST API, auth, health checks | OpenAPI spec complete, health checks work |
| 5-6  | Tools | CLI, dashboard, docs | CLI functional, dashboard deployed |

The integration strategy ensures a smooth transition while maintaining the high performance and reliability characteristics that make iroh-raft suitable for production use.