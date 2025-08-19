# Deterministic Simulation Testing with madsim for Distributed Consensus

This guide demonstrates comprehensive deterministic simulation testing patterns for distributed consensus systems using [madsim](https://github.com/madsim-rs/madsim). These patterns are essential for testing distributed systems scenarios that are difficult or impossible to test reliably with traditional testing methods.

## Overview

Madsim provides deterministic simulation capabilities that enable:

- **Reproducible Testing**: Fixed seeds ensure identical test runs
- **Network Simulation**: Control over message delivery, delays, and partitions
- **Failure Injection**: Deterministic node failures and recoveries
- **Time Control**: Precise control over timing and delays
- **Invariant Validation**: Verification of distributed system safety properties

## Key Testing Scenarios

### 1. Network Partition Testing

```rust
#[test]
fn test_raft_consensus_under_partition() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 42, // Fixed seed for determinism
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Create network partition: [1, 2] vs [3, 4, 5]
        cluster.create_partition(vec![1, 2], vec![3, 4, 5]);
        
        // Test majority partition behavior
        assert!(cluster.elect_leader(3).await.is_ok());
        
        // Heal partition and verify consistency
        cluster.heal_partition();
        assert!(cluster.check_consistency().await);
    });
}
```

**Key Points:**
- Test both minority and majority partition behavior
- Verify split-brain prevention in minority partitions
- Test consistency restoration after partition healing

### 2. Leader Election Determinism

```rust
#[test]
fn test_leader_election_determinism() {
    let runtime = Runtime::with_seed(123); // Same seed = same results
    
    runtime.block_on(async {
        // Multiple runs with same seed should produce identical results
        for iteration in 1..=5 {
            let mut cluster = SimRaftCluster::new(node_ids);
            cluster.elect_leader(1).await.expect("Election should succeed");
            assert_eq!(cluster.get_leader_id(), Some(1));
        }
    });
}
```

**Key Points:**
- Use fixed seeds to ensure reproducible results
- Test election outcomes under various conditions
- Verify deterministic behavior across multiple runs

### 3. Message Loss and Network Unreliability

```rust
#[test]
fn test_message_loss_scenarios() {
    let runtime = Runtime::with_seed(456);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Simulate message drops
        cluster.drop_messages(1, 2); // Leader -> Follower 2
        cluster.drop_messages(2, 1); // Follower 2 -> Leader
        
        // Test system behavior with partial connectivity
        let proposal = ProposalData::set("test_key", "test_value");
        assert!(cluster.propose(proposal).await.is_ok());
        
        // Test complete isolation
        cluster.drop_all_messages_to_leader();
        assert!(!cluster.has_leader()); // Should lose leadership
    });
}
```

**Key Points:**
- Test partial and complete message loss scenarios
- Verify system behavior under network unreliability
- Test leadership changes due to communication failures

### 4. Node Failure and Recovery

```rust
#[test]
fn test_node_failure_and_recovery() {
    let runtime = Runtime::with_seed(789);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Propose data before failure
        cluster.propose(ProposalData::set("pre_failure", "data")).await?;
        
        // Simulate leader crash
        cluster.stop_node(leader_id);
        
        // Test new leader election
        cluster.elect_leader(new_leader_id).await?;
        
        // Propose data with new leader
        cluster.propose(ProposalData::set("post_failure", "data")).await?;
        
        // Restart failed node and verify consistency
        cluster.restart_node(leader_id);
        assert!(cluster.check_consistency().await);
    });
}
```

**Key Points:**
- Test both leader and follower failures
- Verify data persistence across failures
- Test cluster recovery and consistency restoration

### 5. Concurrent Operations Testing

```rust
#[test]
fn test_concurrent_proposals() {
    let runtime = Runtime::with_seed(999);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Submit multiple concurrent proposals
        let proposals = vec![
            ProposalData::set("key1", "value1"),
            ProposalData::set("key2", "value2"),
            ProposalData::set("key3", "value3"),
        ];
        
        for proposal in proposals {
            cluster.propose(proposal).await?;
        }
        
        cluster.commit_entries().await?;
        
        // Verify all proposals are applied in correct order
        assert!(cluster.check_consistency().await);
    });
}
```

**Key Points:**
- Test multiple concurrent operations
- Verify ordering guarantees
- Test consistency under high load

### 6. Split-Brain Prevention

```rust
#[test]
fn test_split_brain_prevention() {
    let runtime = Runtime::with_seed(2024);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(vec![1, 2, 3, 4]);
        
        // Create symmetric partition: [1, 2] vs [3, 4]
        cluster.create_partition(vec![1, 2], vec![3, 4]);
        
        // Neither partition should elect a leader (no majority)
        assert!(cluster.elect_leader(1).await.is_err());
        assert!(cluster.elect_leader(3).await.is_err());
        
        // Verify no split-brain scenario
        assert!(!cluster.has_leader());
    });
}
```

**Key Points:**
- Test symmetric partitions where no group has majority
- Verify no leader can be elected without majority
- Test recovery when majority is restored

### 7. Invariant Safety Properties

```rust
#[test]
fn test_invariant_safety_properties() {
    let runtime = Runtime::with_seed(12345);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Test: At most one leader per term
        cluster.elect_leader(1).await?;
        let term = cluster.current_term;
        
        // Verify only one leader exists in this term
        let leaders = cluster.count_leaders_in_term(term);
        assert!(leaders <= 1, "Election Safety: At most one leader per term");
        
        // Test: Leader completeness
        cluster.propose_and_commit(data).await?;
        
        // Change leadership
        cluster.stop_node(1);
        cluster.elect_leader(2).await?;
        
        // Committed entries must persist
        assert!(cluster.has_committed_entry(&data), 
                "Leader Completeness: Committed entries must persist");
        
        // Test: State machine safety
        let states: Vec<_> = cluster.get_all_committed_states();
        for i in 1..states.len() {
            assert_eq!(states[0], states[i],
                      "State Machine Safety: All nodes apply same sequence");
        }
    });
}
```

**Key Points:**
- Test core Raft safety properties
- Verify election safety (at most one leader per term)
- Verify leader completeness (committed entries persist)
- Verify state machine safety (deterministic application)

## Advanced Testing Patterns

### Time-Based Testing

```rust
#[test]
fn test_timing_edge_cases() {
    let runtime = Runtime::with_seed(1337);
    
    runtime.block_on(async {
        let mut cluster = SimRaftCluster::new(node_ids);
        
        // Add controlled delays
        runtime.net.delay(node1, node2, Duration::from_millis(100));
        
        // Test election timeouts
        runtime.run_for(Duration::from_millis(election_timeout)).await;
        
        // Verify timing-dependent behavior
        assert!(cluster.check_election_occurred());
    });
}
```

### Stress Testing

```rust
#[test]
fn test_comprehensive_stress_scenario() {
    let runtime = Runtime::with_seed(98765);
    
    runtime.block_on(async {
        // Phase 1: Normal operation
        perform_normal_operations(&mut cluster).await;
        
        // Phase 2: Network partitions
        simulate_network_partitions(&mut cluster).await;
        
        // Phase 3: Node failures
        simulate_node_failures(&mut cluster).await;
        
        // Phase 4: Recovery and healing
        recover_and_heal(&mut cluster).await;
        
        // Verify final consistency
        assert!(cluster.check_consistency().await);
    });
}
```

## Best Practices

### 1. Use Fixed Seeds
```rust
// Good: Deterministic
let runtime = Runtime::with_seed(42);

// Bad: Non-deterministic
let runtime = Runtime::new();
```

### 2. Test Edge Cases
- Network partitions with different group sizes
- Simultaneous node failures
- Message reordering and delays
- Election timeouts and heartbeat failures

### 3. Verify Invariants
```rust
// Always verify safety properties
assert!(cluster.election_safety());
assert!(cluster.leader_completeness());
assert!(cluster.state_machine_safety());
```

### 4. Test Recovery Scenarios
```rust
// Test various recovery patterns
cluster.stop_nodes(&[1, 2]).await;
cluster.restart_nodes(&[1, 2]).await;
assert!(cluster.recovers_to_consistency().await);
```

### 5. Use Comprehensive Assertions
```rust
// Detailed consistency checks
assert_eq!(cluster.count_active_nodes(), expected_count);
assert_eq!(cluster.get_committed_log_length(), expected_length);
assert!(cluster.all_nodes_have_same_committed_state());
```

## Running the Tests

```bash
# Run all madsim simulation tests
cargo test --features madsim --test madsim_tests

# Run specific test scenario
cargo test --features madsim test_raft_consensus_under_partition

# Run with output for debugging
cargo test --features madsim --test madsim_tests -- --nocapture

# Run demo
cd examples/madsim_demo
cargo run --bin raft_simulation_test
```

## Integration with CI/CD

```yaml
# GitHub Actions example
- name: Run Madsim Tests
  run: |
    cargo test --features madsim --test madsim_tests
    cargo test --features madsim --test madsim_tests -- --nocapture
```

## Conclusion

Deterministic simulation testing with madsim provides comprehensive coverage of distributed system scenarios that are impossible to test reliably with traditional methods. The patterns demonstrated here ensure:

- **Reproducibility**: Same input always produces same output
- **Comprehensive Coverage**: All critical failure scenarios tested
- **Safety Verification**: Core distributed system invariants validated
- **CI/CD Integration**: Reliable automated testing pipeline

These testing patterns are essential for building robust distributed consensus systems and should be integrated early in the development process.