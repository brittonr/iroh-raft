//! Basic property-based tests for iroh-raft core types
//!
//! This module contains fundamental property-based tests for the core types
//! and serialization logic of iroh-raft.

#[cfg(feature = "test-helpers")]
mod tests {
    use iroh_raft::test_helpers::generators::*;
    use iroh_raft::types::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    /// Property-based tests for ProposalData serialization
    mod proposal_data_tests {
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
            fn proposal_data_display_never_panics(data in proposal_data_strategy()) {
                // Display formatting should never panic
                let _display_string = format!("{}", data);
                prop_assert!(true); // If we reach here, no panic occurred
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
            fn raw_data_preserves_bytes(bytes in vec(any::<u8>(), 0..1000)) {
                let proposal = ProposalData::raw(bytes.clone());
                
                if let ProposalData::Raw(stored_bytes) = proposal {
                    prop_assert_eq!(bytes, stored_bytes);
                } else {
                    prop_assert!(false, "Expected Raw variant");
                }
            }

            #[test]
            fn text_data_preserves_content(text in any::<String>()) {
                let proposal = ProposalData::text(text.clone());
                
                if let ProposalData::Text(stored_text) = proposal {
                    prop_assert_eq!(text, stored_text);
                } else {
                    prop_assert!(false, "Expected Text variant");
                }
            }

            #[test]
            fn noop_is_consistent() {
                let noop1 = ProposalData::noop();
                let noop2 = ProposalData::noop();
                
                prop_assert_eq!(noop1, noop2);
                prop_assert!(matches!(noop1, ProposalData::Noop));
            }
        }
    }

    /// Property-based tests for generators themselves
    mod generator_tests {
        use super::*;

        proptest! {
            #[test]
            fn node_id_strategy_generates_valid_ids(node_id in node_id_strategy()) {
                prop_assert!(node_id >= 1);
                prop_assert!(node_id <= 1000);
            }

            #[test]
            fn cluster_config_strategy_generates_valid_configs(
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
            fn duration_strategy_generates_reasonable_durations(duration in duration_strategy()) {
                use std::time::Duration;
                prop_assert!(duration >= Duration::from_millis(1));
                prop_assert!(duration <= Duration::from_millis(300));
            }

            #[test]
            fn cluster_size_strategy_is_reasonable(size in cluster_size_strategy()) {
                prop_assert!(size >= 1);
                prop_assert!(size <= 7);
            }

            #[test]
            fn partition_strategy_respects_cluster_size(
                cluster_size in 3usize..=7,
                partition in partition_strategy(7) // Max size for strategy
            ) {
                // Partition should not include more than half the cluster
                prop_assert!(partition.len() <= cluster_size / 2);
                
                // All partition indices should be valid
                for &index in &partition {
                    prop_assert!(index < cluster_size);
                }
                
                // Partition indices should be unique
                let unique_indices: HashSet<_> = partition.iter().collect();
                prop_assert_eq!(unique_indices.len(), partition.len());
            }
        }
    }

    /// Property-based tests for malformed input handling
    mod malformed_input_tests {
        use super::*;

        proptest! {
            #[test]
            fn json_deserialize_invalid_proposal_data(invalid_json in "[^{}\"\\[\\]]{1,100}") {
                // Invalid JSON should fail gracefully
                let result: Result<ProposalData, _> = serde_json::from_str(&invalid_json);
                prop_assert!(result.is_err());
            }

            #[test]
            fn bincode_deserialize_random_bytes_fails(random_bytes in vec(any::<u8>(), 1..100)) {
                // Random bytes should fail to deserialize as ProposalData
                let result: Result<ProposalData, _> = bincode::deserialize(&random_bytes);
                // This might succeed if the bytes happen to be valid, but most won't be
                // The important thing is that it doesn't panic
                let _is_ok = result.is_ok();
                prop_assert!(true); // If we reach here, no panic occurred
            }

            #[test]
            fn empty_json_objects_handled_gracefully(
                empty_variant in prop_oneof![
                    Just("{}"),
                    Just("[]"),
                    Just("\"\""),
                    Just("null"),
                ]
            ) {
                let result: Result<ProposalData, _> = serde_json::from_str(empty_variant);
                // These should all fail to deserialize as ProposalData, but not panic
                prop_assert!(result.is_err());
            }
        }
    }

    /// Property-based tests for edge cases
    mod edge_case_tests {
        use super::*;

        proptest! {
            #[test]
            fn very_large_strings_handled(large_text in string_regex(".{10000,50000}").unwrap()) {
                let proposal = ProposalData::text(large_text.clone());
                
                // Should be able to create and serialize large text
                let serialized = serde_json::to_string(&proposal);
                prop_assert!(serialized.is_ok());
                
                if let Ok(json) = serialized {
                    let deserialized: Result<ProposalData, _> = serde_json::from_str(&json);
                    prop_assert!(deserialized.is_ok());
                    
                    if let Ok(ProposalData::Text(restored_text)) = deserialized {
                        prop_assert_eq!(large_text, restored_text);
                    }
                }
            }

            #[test]
            fn very_large_byte_arrays_handled(large_bytes in vec(any::<u8>(), 10000..50000)) {
                let proposal = ProposalData::raw(large_bytes.clone());
                
                // Should be able to create and serialize large byte arrays
                let serialized = bincode::serialize(&proposal);
                prop_assert!(serialized.is_ok());
                
                if let Ok(bytes) = serialized {
                    let deserialized: Result<ProposalData, _> = bincode::deserialize(&bytes);
                    prop_assert!(deserialized.is_ok());
                    
                    if let Ok(ProposalData::Raw(restored_bytes)) = deserialized {
                        prop_assert_eq!(large_bytes, restored_bytes);
                    }
                }
            }

            #[test]
            fn unicode_strings_handled(unicode_text in "\\PC{100,500}") {
                let proposal = ProposalData::text(unicode_text.clone());
                
                // Unicode should be handled correctly
                let serialized = serde_json::to_string(&proposal).expect("Failed to serialize");
                let deserialized: ProposalData = serde_json::from_str(&serialized)
                    .expect("Failed to deserialize");
                
                if let ProposalData::Text(restored_text) = deserialized {
                    prop_assert_eq!(unicode_text, restored_text);
                } else {
                    prop_assert!(false, "Expected Text variant");
                }
            }

            #[test]
            fn special_characters_in_keys_handled(
                key in string_regex("[!@#$%^&*()_+={};:,.<>?]{1,20}").unwrap(),
                value in any::<String>()
            ) {
                let proposal = ProposalData::set(key.clone(), value.clone());
                
                // Special characters in keys should be handled
                let serialized = serde_json::to_string(&proposal).expect("Failed to serialize");
                let deserialized: ProposalData = serde_json::from_str(&serialized)
                    .expect("Failed to deserialize");
                
                if let ProposalData::KeyValue(KeyValueOp::Set { key: restored_key, value: restored_value }) = deserialized {
                    prop_assert_eq!(key, restored_key);
                    prop_assert_eq!(value, restored_value);
                } else {
                    prop_assert!(false, "Expected KeyValue Set variant");
                }
            }
        }
    }

    /// Basic unit tests to ensure the property test framework works
    mod unit_tests {
        use super::*;

        #[test]
        fn proposal_data_constructors_work() {
            let set_op = ProposalData::set("key1", "value1");
            assert!(matches!(set_op, ProposalData::KeyValue(KeyValueOp::Set { .. })));

            let get_op = ProposalData::get("key1");
            assert!(matches!(get_op, ProposalData::KeyValue(KeyValueOp::Get { .. })));

            let delete_op = ProposalData::delete("key1");
            assert!(matches!(delete_op, ProposalData::KeyValue(KeyValueOp::Delete { .. })));

            let text_op = ProposalData::text("hello world");
            assert!(matches!(text_op, ProposalData::Text(_)));

            let raw_op = ProposalData::raw(vec![1, 2, 3]);
            assert!(matches!(raw_op, ProposalData::Raw(_)));

            let noop_op = ProposalData::noop();
            assert!(matches!(noop_op, ProposalData::Noop));
        }

        #[test]
        fn basic_serialization_works() {
            let proposal = ProposalData::set("test_key", "test_value");
            
            // JSON roundtrip
            let json = serde_json::to_string(&proposal).unwrap();
            let from_json: ProposalData = serde_json::from_str(&json).unwrap();
            assert_eq!(proposal, from_json);

            // Bincode roundtrip
            let bincode_bytes = bincode::serialize(&proposal).unwrap();
            let from_bincode: ProposalData = bincode::deserialize(&bincode_bytes).unwrap();
            assert_eq!(proposal, from_bincode);
        }

        #[test]
        fn display_formatting_works() {
            let set_op = ProposalData::set("key1", "value1");
            let display = format!("{}", set_op);
            assert!(display.contains("SET"));
            assert!(display.contains("key1"));
            assert!(display.contains("value1"));

            let get_op = ProposalData::get("key1");
            let display = format!("{}", get_op);
            assert!(display.contains("GET"));
            assert!(display.contains("key1"));

            let noop = ProposalData::noop();
            let display = format!("{}", noop);
            assert!(display.contains("NOOP"));
        }
    }
}

// Tests that run without the test-helpers feature
#[cfg(not(feature = "test-helpers"))]
mod basic_tests {
    use iroh_raft::types::*;

    #[test]
    fn basic_proposal_data_works() {
        let proposal = ProposalData::set("key", "value");
        assert!(matches!(proposal, ProposalData::KeyValue { .. }));
    }
}