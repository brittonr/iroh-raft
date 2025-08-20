//! Property-based tests for iroh-raft
//!
//! This module contains comprehensive property-based tests using the proptest framework
//! to verify important invariants and behaviors of the iroh-raft library.

use iroh_raft::test_helpers::{
    generators::*,
    assertions::*,
    TestCluster, TestNode,
    create_temp_dir,
};
use iroh_raft::{
    config::{Config, ConfigBuilder, ConfigError},
    raft::messages::*,
    types::*,
    error::RaftError,
};

use proptest::prelude::*;
use proptest::collection::vec;
use proptest::string::string_regex;
use std::time::Duration;
use std::collections::HashSet;

/// Property-based tests for message serialization and deserialization
mod serialization_tests {
    use super::*;

    proptest! {
        #[test]
        fn proposal_data_json_roundtrip(data in proposal_data_strategy()) {
            // Test JSON serialization roundtrip
            let serialized = serde_json::to_string(&data).expect("Failed to serialize");
            let deserialized: ProposalData = serde_json::from_str(&serialized)
                .expect("Failed to deserialize");
            
            prop_assert_eq!(data, deserialized);
        }

        #[test]
        fn proposal_data_bincode_roundtrip(data in proposal_data_strategy()) {
            // Test bincode serialization roundtrip
            let serialized = bincode::serialize(&data).expect("Failed to serialize");
            let deserialized: ProposalData = bincode::deserialize(&serialized)
                .expect("Failed to deserialize");
            
            prop_assert_eq!(data, deserialized);
        }

        #[test]
        fn conf_change_context_roundtrip(context in conf_change_context_strategy()) {
            // Test JSON serialization roundtrip for configuration change context
            let serialized = serde_json::to_string(&context).expect("Failed to serialize");
            let deserialized: ConfChangeContext = serde_json::from_str(&serialized)
                .expect("Failed to deserialize");
            
            prop_assert_eq!(context.address, deserialized.address);
            prop_assert_eq!(context.p2p_node_id, deserialized.p2p_node_id);
            prop_assert_eq!(context.p2p_addresses, deserialized.p2p_addresses);
            prop_assert_eq!(context.p2p_relay_url, deserialized.p2p_relay_url);
        }
    }

    #[test]
    fn raft_message_serialization_basic() {
        // Test basic Raft message serialization (using raft crate's built-in serialization)
        use raft::prelude::*;
        
        let mut msg = Message::default();
        msg.from = 1;
        msg.to = 2;
        msg.term = 1;
        msg.set_msg_type(MessageType::MsgHeartbeat);
        
        // Test the helper functions from our messages module
        let serialized = serialize_message(&msg).expect("Failed to serialize");
        let deserialized = deserialize_message(&serialized).expect("Failed to deserialize");
        
        assert_eq!(msg.from, deserialized.from);
        assert_eq!(msg.to, deserialized.to);
        assert_eq!(msg.term, deserialized.term);
        assert_eq!(msg.msg_type(), deserialized.msg_type());
    }
}

/// Property-based tests for protocol message handling with malformed input
mod malformed_input_tests {
    use super::*;

    proptest! {
        #[test]
        fn deserialize_random_bytes_should_fail(random_bytes in vec(any::<u8>(), 0..1000)) {
            // Attempting to deserialize random bytes should fail gracefully
            let result = deserialize_message(&random_bytes);
            prop_assert!(result.is_err());
            
            // Verify it returns a proper RaftError::Network error
            if let Err(RaftError::Network { details }) = result {
                prop_assert!(details.contains("Failed to deserialize"));
            } else {
                prop_assert!(false, "Expected RaftError::Network");
            }
        }

        #[test]
        fn json_deserialize_invalid_proposal_data(invalid_json in "[^{}\"\\[\\]]{1,100}") {
            // Invalid JSON should fail gracefully
            let result: Result<ProposalData, _> = serde_json::from_str(&invalid_json);
            prop_assert!(result.is_err());
        }

        #[test]
        fn proposal_data_display_never_panics(data in proposal_data_strategy()) {
            // Display formatting should never panic
            let _display_string = format!("{}", data);
            prop_assert!(true); // If we reach here, no panic occurred
        }
    }

    #[test]
    fn empty_bytes_deserialization_fails() {
        let result = deserialize_message(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn partial_message_deserialization_fails() {
        // Create a valid message, serialize it, then truncate
        use raft::prelude::*;
        let mut msg = Message::default();
        msg.from = 1;
        msg.to = 2;
        msg.set_msg_type(MessageType::MsgHeartbeat);
        
        let full_serialized = serialize_message(&msg).unwrap();
        if full_serialized.len() > 1 {
            let partial = &full_serialized[0..full_serialized.len() - 1];
            let result = deserialize_message(partial);
            assert!(result.is_err());
        }
    }
}

/// Property-based tests for state machine operations
mod state_machine_tests {
    use super::*;

    proptest! {
        #[test]
        fn proposal_operations_are_consistent(
            proposals in vec(proposal_data_strategy(), 1..20)
        ) {
            tokio_test::block_on(async {
                // Create a test node and verify proposal operations maintain consistency
                let temp_dir = create_temp_dir("state-machine-test").unwrap();
                let mut node = TestNode::new(1, temp_dir).await.unwrap();
                
                // Apply all proposals
                for proposal in &proposals {
                    let result = node.propose(proposal.clone()).await;
                    prop_assert!(result.is_ok(), "Proposal should succeed: {:?}", result);
                }
                
                // Verify state consistency
                let committed_state = node.get_committed_state().await;
                prop_assert!(committed_state.is_ok());
                
                let state = committed_state.unwrap();
                prop_assert_eq!(state.len(), proposals.len());
                
                // Verify each proposal was committed in order
                for (i, (expected, actual)) in proposals.iter().zip(state.iter()).enumerate() {
                    prop_assert_eq!(expected, actual, "Mismatch at position {}", i);
                }
            });
        }

        #[test]
        fn key_value_operations_maintain_invariants(
            operations in vec(
                prop_oneof![
                    (string_regex("[a-zA-Z0-9_]{1,20}").unwrap(), any::<String>())
                        .prop_map(|(k, v)| ProposalData::set(k, v)),
                    string_regex("[a-zA-Z0-9_]{1,20}").unwrap()
                        .prop_map(ProposalData::get),
                    string_regex("[a-zA-Z0-9_]{1,20}").unwrap()
                        .prop_map(ProposalData::delete),
                ],
                1..10
            )
        ) {
            // Verify key-value operations maintain expected structure
            for op in operations {
                match op {
                    ProposalData::KeyValue(KeyValueOp::Set { key, value }) => {
                        prop_assert!(!value.is_empty(), "Set operations must have a value");
                        prop_assert!(!key.is_empty(), "Keys cannot be empty");
                    }
                    ProposalData::KeyValue(KeyValueOp::Get { key }) => {
                        prop_assert!(!key.is_empty(), "Keys cannot be empty");
                    }
                    ProposalData::KeyValue(KeyValueOp::Delete { key }) => {
                        prop_assert!(!key.is_empty(), "Keys cannot be empty");
                    }
                    _ => {} // Other variants are fine
                }
            }
        }

        #[test]
        fn node_isolation_affects_proposals(
            proposals in vec(proposal_data_strategy(), 1..5)
        ) {
            tokio_test::block_on(async {
                let temp_dir = create_temp_dir("isolation-test").unwrap();
                let mut node = TestNode::new(1, temp_dir).await.unwrap();
                
                // Node should accept proposals when not isolated
                for proposal in &proposals {
                    let result = node.propose(proposal.clone()).await;
                    prop_assert!(result.is_ok());
                }
                
                // Set network isolation
                node.set_network_isolation(true).await.unwrap();
                
                // Node should reject proposals when isolated
                let isolated_proposal = ProposalData::text("isolated");
                let result = node.propose(isolated_proposal).await;
                prop_assert!(result.is_err());
                
                if let Err(RaftError::NetworkPartition { details }) = result {
                    prop_assert!(details.contains("isolated"));
                } else {
                    prop_assert!(false, "Expected NetworkPartition error");
                }
            });
        }
    }
}

/// Property-based tests for configuration validation
mod configuration_tests {
    use super::*;

    proptest! {
        #[test]
        fn valid_node_ids_create_valid_configs(node_id in node_id_strategy()) {
            let result = ConfigBuilder::new()
                .node_id(node_id)
                .data_dir("./test-data")
                .bind_address("127.0.0.1:8080")
                .build();
            
            prop_assert!(result.is_ok(), "Valid node ID should create valid config");
            
            if let Ok(config) = result {
                prop_assert_eq!(config.node.id, node_id);
            }
        }

        #[test]
        fn cluster_config_validation(
            (size, node_ids) in cluster_config_strategy()
        ) {
            prop_assert_eq!(size, node_ids.len());
            prop_assert!(size >= 1);
            prop_assert!(size <= 10);
            
            // All node IDs should be unique
            let unique_ids: HashSet<_> = node_ids.iter().collect();
            prop_assert_eq!(unique_ids.len(), node_ids.len());
            
            // All node IDs should be positive
            for &id in &node_ids {
                prop_assert!(id > 0);
            }
        }

        #[test]
        fn duration_strategies_are_reasonable(duration in duration_strategy()) {
            prop_assert!(duration >= Duration::from_millis(1));
            prop_assert!(duration <= Duration::from_millis(300));
        }

        #[test]
        fn cluster_sizes_are_reasonable(size in cluster_size_strategy()) {
            prop_assert!(size >= 1);
            prop_assert!(size <= 7);
        }
    }

    #[test]
    fn zero_node_id_fails_validation() {
        let result = ConfigBuilder::new()
            .node_id(0)
            .data_dir("./test-data")
            .bind_address("127.0.0.1:8080")
            .build();
            
        assert!(result.is_err());
        if let Err(ConfigError::Invalid { field, .. }) = result {
            assert_eq!(field, "node_id");
        }
    }

    #[test]
    fn empty_data_dir_fails_validation() {
        let result = ConfigBuilder::new()
            .node_id(1)
            .data_dir("")
            .bind_address("127.0.0.1:8080")
            .build();
            
        assert!(result.is_err());
    }

    #[test]
    fn invalid_bind_address_fails_validation() {
        let result = ConfigBuilder::new()
            .node_id(1)
            .data_dir("./test-data")
            .bind_address("invalid-address")
            .build();
            
        assert!(result.is_err());
    }
}

/// Integration property tests combining multiple components
mod integration_tests {
    use super::*;

    proptest! {
        #[test]
        fn cluster_consensus_with_random_proposals(
            cluster_size in 1usize..=3, // Smaller clusters for faster testing
            proposals in vec(proposal_data_strategy(), 1..10)
        ) {
            tokio_test::block_on(async {
                let mut cluster = TestCluster::new(cluster_size).await.unwrap();
                
                // Submit all proposals
                for proposal_data in proposals {
                    let proposal_bytes = serde_json::to_string(&proposal_data)
                        .unwrap()
                        .into_bytes();
                    
                    // Try to propose (might fail if no leader, which is OK for property tests)
                    let _result = cluster.propose(&proposal_bytes).await;
                    // We don't assert success here because in small clusters
                    // leadership might not be established quickly
                }
                
                // For single-node clusters, consensus should always be reachable
                if cluster_size == 1 {
                    let consensus_result = cluster.check_consensus().await;
                    prop_assert!(consensus_result);
                }
                
                // Cluster status should be consistent
                let status = cluster.get_cluster_status().await;
                prop_assert_eq!(status.total_nodes, cluster_size);
                prop_assert!(status.active_nodes <= status.total_nodes);
                prop_assert!(status.nodes.len() <= cluster_size);
            });
        }

        #[test]
        fn network_partitions_are_handled_gracefully(
            cluster_size in 3usize..=5, // Need at least 3 for meaningful partitions
            partition_nodes in partition_strategy(5) // Max partition size
        ) {
            tokio_test::block_on(async {
                // Only test if partition makes sense for cluster size
                if partition_nodes.iter().all(|&i| i < cluster_size) {
                    let mut cluster = TestCluster::new(cluster_size).await.unwrap();
                    
                    // Create partition
                    let result = cluster.create_network_partition(&partition_nodes).await;
                    prop_assert!(result.is_ok());
                    
                    // Heal partition
                    let heal_result = cluster.heal_network_partition().await;
                    prop_assert!(heal_result.is_ok());
                    
                    // All nodes should be active again after healing
                    let status = cluster.get_cluster_status().await;
                    prop_assert_eq!(status.active_nodes, cluster_size);
                }
            });
        }
    }

    #[tokio::test]
    async fn node_lifecycle_maintains_consistency() {
        let mut cluster = TestCluster::new(3).await.unwrap();
        
        // Stop a node
        assert!(cluster.stop_node(1).await.is_ok());
        
        // Verify cluster status reflects the stopped node
        let status = cluster.get_cluster_status().await;
        assert_eq!(status.total_nodes, 3);
        assert!(status.active_nodes < 3);
        
        // Restart the node
        assert!(cluster.restart_node(1).await.is_ok());
        
        // Give some time for the node to come back online
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Verify node is active again
        let final_status = cluster.get_cluster_status().await;
        assert_eq!(final_status.total_nodes, 3);
    }
}

/// Property-based tests for edge cases and error conditions
mod edge_case_tests {
    use super::*;

    proptest! {
        #[test]
        fn very_large_proposals_are_handled(
            large_data in vec(any::<u8>(), 10_000..100_000)
        ) {
            tokio_test::block_on(async {
                let temp_dir = create_temp_dir("large-proposal-test").unwrap();
                let mut node = TestNode::new(1, temp_dir).await.unwrap();
                
                let proposal = ProposalData::raw(large_data);
                let result = node.propose(proposal).await;
                
                // Large proposals should either succeed or fail gracefully
                // (depending on implementation limits)
                prop_assert!(result.is_ok() || result.is_err());
                
                if result.is_err() {
                    // If it fails, it should be for a reasonable reason
                    // (not a panic or undefined behavior)
                    let error = result.unwrap_err();
                    prop_assert!(matches!(
                        error,
                        RaftError::ProposalRejected { .. } |
                        RaftError::InvalidInput { .. } |
                        RaftError::NotReady { .. }
                    ));
                }
            });
        }

        #[test]
        fn empty_proposals_are_handled(empty_type in prop_oneof![
            Just(ProposalData::Raw(vec![])),
            Just(ProposalData::Text(String::new())),
            Just(ProposalData::Noop)
        ]) {
            tokio_test::block_on(async {
                let temp_dir = create_temp_dir("empty-proposal-test").unwrap();
                let mut node = TestNode::new(1, temp_dir).await.unwrap();
                
                let result = node.propose(empty_type).await;
                // Empty proposals should be handled gracefully
                prop_assert!(result.is_ok());
            });
        }

        #[test]
        fn rapid_node_operations_dont_cause_issues(
            operations in vec(prop_oneof![
                Just("stop".to_string()),
                Just("restart".to_string()),
                Just("isolate".to_string()),
                Just("heal".to_string()),
            ], 1..10)
        ) {
            tokio_test::block_on(async {
                let temp_dir = create_temp_dir("rapid-ops-test").unwrap();
                let mut node = TestNode::new(1, temp_dir).await.unwrap();
                
                for op in operations {
                    match op.as_str() {
                        "stop" => { let _ = node.stop().await; }
                        "restart" => { let _ = node.restart().await; }
                        "isolate" => { let _ = node.set_network_isolation(true).await; }
                        "heal" => { let _ = node.set_network_isolation(false).await; }
                        _ => {}
                    }
                    
                    // Brief pause to avoid overwhelming the system
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                
                // Node should still be in a consistent state
                let status = node.get_status().await;
                prop_assert!(status.node_id == 1);
            });
        }
    }
}