//! Minimal property-based tests for iroh-raft
//!
//! This module contains basic property-based tests that work with the current
//! state of the codebase, focusing on core types and generators.

#[cfg(feature = "test-helpers")]
mod tests {
    use iroh_raft::test_helpers::generators::*;
    use iroh_raft::types::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    /// Test that basic generators work correctly
    proptest! {
        #[test]
        fn node_id_strategy_generates_valid_ids(node_id in node_id_strategy()) {
            prop_assert!(node_id >= 1);
            prop_assert!(node_id <= 1000);
        }

        #[test]
        fn cluster_size_strategy_is_reasonable(size in cluster_size_strategy()) {
            prop_assert!(size >= 1);
            prop_assert!(size <= 7);
        }

        #[test]
        fn duration_strategy_generates_reasonable_durations(duration in duration_strategy()) {
            use std::time::Duration;
            prop_assert!(duration >= Duration::from_millis(1));
            prop_assert!(duration <= Duration::from_millis(300));
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
    }

    /// Test ProposalData serialization and invariants
    proptest! {
        #[test]
        fn proposal_data_json_roundtrip(data in proposal_data_strategy()) {
            let serialized = serde_json::to_string(&data).expect("Failed to serialize");
            let deserialized: ProposalData = serde_json::from_str(&serialized)
                .expect("Failed to deserialize");
            
            prop_assert_eq!(data, deserialized);
        }

        #[test]
        fn proposal_data_display_never_panics(data in proposal_data_strategy()) {
            let _display_string = format!("{}", data);
            prop_assert!(true); // If we reach here, no panic occurred
        }

        #[test]
        fn key_value_operations_maintain_invariants(
            op in prop_oneof![
                (string_regex("[a-zA-Z0-9_]{1,20}").unwrap(), any::<String>())
                    .prop_map(|(k, v)| ProposalData::set(k, v)),
                string_regex("[a-zA-Z0-9_]{1,20}").unwrap()
                    .prop_map(ProposalData::get),
                string_regex("[a-zA-Z0-9_]{1,20}").unwrap()
                    .prop_map(ProposalData::delete),
            ]
        ) {
            match op {
                ProposalData::KeyValue { op: KeyValueOp::Set, key, value } => {
                    prop_assert!(value.is_some(), "Set operations must have a value");
                    prop_assert!(!key.is_empty(), "Keys cannot be empty");
                }
                ProposalData::KeyValue { op: KeyValueOp::Get, key, value } => {
                    prop_assert!(value.is_none(), "Get operations should not have a value");
                    prop_assert!(!key.is_empty(), "Keys cannot be empty");
                }
                ProposalData::KeyValue { op: KeyValueOp::Delete, key, value } => {
                    prop_assert!(value.is_none(), "Delete operations should not have a value");
                    prop_assert!(!key.is_empty(), "Keys cannot be empty");
                }
                _ => {} // Other variants are fine
            }
        }

        #[test]
        fn raw_data_preserves_bytes(bytes in vec(any::<u8>(), 0..100)) {
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
    }

    /// Test malformed input handling
    proptest! {
        #[test]
        fn json_deserialize_invalid_proposal_data(invalid_json in "[^{}\"\\[\\]]{1,50}") {
            let result: Result<ProposalData, _> = serde_json::from_str(&invalid_json);
            prop_assert!(result.is_err());
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
            prop_assert!(result.is_err());
        }
    }

    /// Test edge cases
    proptest! {
        #[test]
        fn large_strings_handled(large_text in string_regex(".{1000,5000}").unwrap()) {
            let proposal = ProposalData::text(large_text.clone());
            
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
        fn unicode_strings_handled(unicode_text in "\\PC{10,100}") {
            let proposal = ProposalData::text(unicode_text.clone());
            
            let serialized = serde_json::to_string(&proposal).expect("Failed to serialize");
            let deserialized: ProposalData = serde_json::from_str(&serialized)
                .expect("Failed to deserialize");
            
            if let ProposalData::Text(restored_text) = deserialized {
                prop_assert_eq!(unicode_text, restored_text);
            } else {
                prop_assert!(false, "Expected Text variant");
            }
        }
    }
}

#[cfg(not(feature = "test-helpers"))]
mod basic_tests {
    use iroh_raft::types::*;

    #[test]
    fn basic_proposal_data_works() {
        let proposal = ProposalData::set("key", "value");
        assert!(matches!(proposal, ProposalData::KeyValue { .. }));
    }

    #[test]
    fn proposal_data_constructors() {
        let set_op = ProposalData::set("key1", "value1");
        assert!(matches!(set_op, ProposalData::KeyValue { op: KeyValueOp::Set, .. }));

        let get_op = ProposalData::get("key1");
        assert!(matches!(get_op, ProposalData::KeyValue { op: KeyValueOp::Get, .. }));

        let delete_op = ProposalData::delete("key1");
        assert!(matches!(delete_op, ProposalData::KeyValue { op: KeyValueOp::Delete, .. }));

        let text_op = ProposalData::text("hello world");
        assert!(matches!(text_op, ProposalData::Text(_)));

        let raw_op = ProposalData::raw(vec![1, 2, 3]);
        assert!(matches!(raw_op, ProposalData::Raw(_)));

        let noop_op = ProposalData::noop();
        assert!(matches!(noop_op, ProposalData::Noop));
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