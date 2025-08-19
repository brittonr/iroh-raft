# Comprehensive Test Suite for iroh-raft

This document describes the comprehensive test suite for the iroh-raft library, focusing on the new features: connection pooling, zero-copy message handling, and graceful shutdown.

## Test Structure

The test suite is organized into several categories:

### 1. Connection Pooling Tests (`tests/connection_pooling_tests.rs`)

Tests for the connection pooling functionality in the transport layer:

- **Connection Reuse**: Verifies that connections are properly reused between messages
- **Stale Connection Cleanup**: Tests automatic cleanup of inactive connections
- **Concurrent Access**: Validates thread-safe access to the connection pool
- **Health Checks**: Tests connection health monitoring and recovery
- **Metrics Tracking**: Verifies proper metrics collection for pool operations
- **Resource Management**: Tests connection limits and pool exhaustion scenarios

**Key Test Functions:**
- `test_connection_reuse()` - Basic connection reuse functionality
- `test_stale_connection_cleanup()` - Cleanup of inactive connections
- `test_concurrent_connection_access()` - Thread safety under load
- `test_connection_health_checks()` - Health monitoring
- `test_connection_pool_metrics()` - Metrics accuracy
- `property_connection_pooling_correctness()` - Property-based testing

### 2. Zero-Copy Message Tests (`tests/zero_copy_tests.rs`)

Tests for zero-copy message handling optimizations:

- **Message Creation**: Tests for borrowed vs owned payload handling
- **COW Behavior**: Copy-on-Write semantics verification
- **Large Message Handling**: Streaming for messages above threshold
- **Fast Serialization**: Optimized Raft message serialization/deserialization
- **Memory Efficiency**: Verification of minimal allocations
- **Error Handling**: Graceful handling of malformed data

**Key Test Functions:**
- `test_zero_copy_message_borrowed()` - Borrowed payload handling
- `test_zero_copy_message_owned()` - Owned payload handling
- `test_cow_behavior()` - Copy-on-Write mechanics
- `test_large_message_detection()` - Large message threshold detection
- `test_fast_raft_message_serialization()` - Optimized serialization
- `test_streaming_large_messages()` - Streaming for large payloads
- `property_zero_copy_message_roundtrip()` - Property-based roundtrip testing

### 3. Graceful Shutdown Tests (`tests/graceful_shutdown_tests.rs`)

Tests for graceful shutdown functionality:

- **Basic Shutdown**: Simple shutdown completion verification
- **Active Connections**: Shutdown with active connections
- **Task Cancellation**: Proper cancellation of background tasks
- **Concurrent Shutdown**: Multiple simultaneous shutdown calls
- **Metrics Recording**: Final metrics collection during shutdown
- **Resource Cleanup**: Proper cleanup of all resources

**Key Test Functions:**
- `test_basic_graceful_shutdown()` - Basic shutdown functionality
- `test_shutdown_with_active_connections()` - Shutdown with active state
- `test_task_cancellation()` - Background task cleanup
- `test_concurrent_shutdown_calls()` - Concurrent shutdown handling
- `test_shutdown_timeout_handling()` - Timeout behavior
- `property_graceful_shutdown_robustness()` - Property-based shutdown testing

### 4. Integration Tests (`tests/integration_tests.rs`)

End-to-end tests combining all features:

- **Full Message Flow**: Complete message flow with pooling
- **Performance Under Load**: High-throughput scenarios
- **Large Message Flow**: End-to-end large message handling
- **Failure Recovery**: Recovery from various failure scenarios
- **Zero-Copy Integration**: Zero-copy optimizations in practice
- **Stress Testing**: System behavior under extreme load

**Key Test Functions:**
- `test_full_message_flow_with_pooling()` - Complete flow testing
- `test_performance_under_load()` - Load testing
- `test_large_message_handling()` - Large message integration
- `test_failure_recovery_scenarios()` - Recovery testing
- `test_zero_copy_message_flow()` - Zero-copy integration
- `test_transport_stress()` - Stress testing

### 5. Distributed Simulation Tests (`tests/madsim_distributed_tests.rs`)

Madsim-based tests for distributed system scenarios:

- **Network Partitions**: Connection pooling under partitions
- **Large Message Distribution**: Zero-copy with large distributed messages
- **Distributed Shutdown**: Graceful shutdown in distributed scenarios
- **Failure Scenarios**: Complex distributed failure patterns
- **Mixed Workloads**: Combined feature testing under realistic conditions

**Key Test Functions:**
- `test_connection_pooling_under_partitions()` - Pooling with network splits
- `test_zero_copy_large_messages()` - Zero-copy in distributed environment
- `test_graceful_shutdown_under_load()` - Shutdown with distributed load
- `test_connection_pool_exhaustion()` - Pool limits in distributed setting
- `test_concurrent_shutdown_scenarios()` - Distributed shutdown coordination

### 6. Test Runner (`tests/test_runner.rs`)

Automated test execution and reporting:

- **Test Suite Configuration**: Configurable test execution
- **Result Aggregation**: Combined results across test categories
- **Performance Reporting**: Duration and throughput metrics
- **Failure Analysis**: Detailed failure reporting and categorization

## Running the Tests

### Individual Test Categories

```bash
# Connection pooling tests
cargo test --test connection_pooling_tests

# Zero-copy message tests
cargo test --test zero_copy_tests

# Graceful shutdown tests
cargo test --test graceful_shutdown_tests

# Integration tests
cargo test --test integration_tests

# Distributed simulation tests (requires madsim feature)
cargo test --test madsim_distributed_tests --features madsim
```

### Property-Based Tests

```bash
# Run property tests with proptest
cargo test --features test-helpers --test property_tests
```

### Complete Test Suite

```bash
# Run all tests
cargo test --all-features

# Run with test runner for comprehensive reporting
cargo run --bin test_runner
```

## Test Configuration

### Environment Variables

- `RUST_LOG=debug` - Enable debug logging during tests
- `PROPTEST_CASES=1000` - Set number of property test cases
- `MADSIM_TEST_SEED=42` - Set deterministic seed for simulation tests

### Features Required

- `test-helpers` - For property-based testing utilities
- `madsim` - For distributed simulation tests
- `metrics-otel` - For metrics testing (optional)

### Test Data

Test data is generated in the `test-data/` directory:
- Temporary directories for isolated test environments
- Mock cluster configurations
- Sample message payloads for various test scenarios

## Test Coverage Areas

### Connection Pooling
- ✅ Connection establishment and reuse
- ✅ Health checks and stale connection cleanup
- ✅ Concurrent access and thread safety
- ✅ Pool size limits and exhaustion handling
- ✅ Metrics collection and reporting
- ✅ Error recovery and fault tolerance

### Zero-Copy Messages
- ✅ Borrowed vs owned payload handling
- ✅ Copy-on-Write (COW) behavior
- ✅ Large message streaming
- ✅ Fast serialization/deserialization
- ✅ Memory efficiency verification
- ✅ Error handling for malformed data

### Graceful Shutdown
- ✅ Clean shutdown with active connections
- ✅ Background task cancellation
- ✅ Resource cleanup verification
- ✅ Concurrent shutdown handling
- ✅ Timeout behavior
- ✅ Final metrics recording

### Integration Scenarios
- ✅ End-to-end message flow
- ✅ Performance under load
- ✅ Failure recovery
- ✅ Stress testing
- ✅ Feature interaction verification

### Distributed System Behavior
- ✅ Network partition tolerance
- ✅ Distributed failure scenarios
- ✅ Large message distribution
- ✅ Coordinated shutdown
- ✅ Mixed workload handling

## Performance Benchmarks

The test suite includes performance verification:

- **Connection Pool Performance**: Connection establishment vs reuse times
- **Zero-Copy Efficiency**: Memory allocation comparison
- **Shutdown Speed**: Time to complete graceful shutdown
- **Throughput**: Messages per second under various loads
- **Latency**: End-to-end message latency with optimizations

## Property-Based Testing

Key properties verified:

1. **Connection Pool Invariants**:
   - Pool size never exceeds configured maximum
   - Stale connections are eventually cleaned up
   - Metrics accurately reflect pool state

2. **Zero-Copy Message Invariants**:
   - Roundtrip serialization preserves data integrity
   - Large messages use streaming appropriately
   - Memory usage is minimal for borrowed payloads

3. **Shutdown Invariants**:
   - Shutdown always completes within timeout
   - No resource leaks after shutdown
   - New operations are rejected during shutdown

## Debugging Tests

### Verbose Output
```bash
cargo test -- --nocapture
```

### Specific Test Debugging
```bash
# Debug specific test with logs
RUST_LOG=debug cargo test test_connection_reuse -- --nocapture

# Run single property test case
PROPTEST_CASES=1 cargo test property_connection_pooling_correctness
```

### Simulation Debugging
```bash
# Debug simulation with fixed seed
MADSIM_TEST_SEED=42 cargo test --features madsim test_connection_pooling_under_partitions -- --nocapture
```

## Continuous Integration

The test suite is designed for CI/CD pipelines:

- **Fast Tests**: Basic functionality tests run in under 30 seconds
- **Integration Tests**: Complete test suite runs in under 5 minutes
- **Simulation Tests**: Distributed tests complete in under 10 minutes
- **Deterministic**: All tests use fixed seeds for reproducible results
- **Parallel**: Tests can run concurrently where possible

## Contributing to Tests

When adding new features:

1. Add unit tests in the appropriate test file
2. Include property-based tests for key invariants
3. Add integration tests for end-to-end scenarios
4. Include madsim tests for distributed behavior
5. Update this documentation with new test descriptions

### Test Guidelines

- Use descriptive test names that explain the scenario
- Include both positive and negative test cases
- Test edge cases and error conditions
- Verify metrics and observability
- Ensure deterministic behavior
- Document expected behavior in test comments