//! Example key-value store implementation using the generic state machine
//!
//! This module provides a simple key-value store as an example of how to
//! implement the generic StateMachine trait. This can serve as a reference
//! for other applications wanting to use iroh-raft.

use super::generic_state_machine::StateMachine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// Commands that can be applied to the key-value store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvCommand {
    /// Set a key to a value
    Set { 
        /// The key to set
        key: String, 
        /// The value to set for the key
        value: String 
    },
    /// Delete a key
    Delete { 
        /// The key to delete
        key: String 
    },
    /// Clear all keys
    Clear,
}

/// Response type for KV commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvResponse {
    /// Command executed successfully
    Ok,
    /// Value retrieved from get operation
    Value(Option<String>),
    /// Keys returned from list operation
    Keys(Vec<String>),
}

/// Query type for read-only KV operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvQuery {
    /// Get a value by key
    Get { key: String },
    /// List all keys with optional prefix
    List { prefix: Option<String> },
    /// Check if key exists
    Exists { key: String },
    /// Get store size
    Size,
}

/// Response type for KV queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvQueryResponse {
    /// Value retrieved
    Value(Option<String>),
    /// Keys returned
    Keys(Vec<String>),
    /// Boolean result
    Boolean(bool),
    /// Numeric result
    Number(usize),
}

/// State of the key-value store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvState {
    /// The key-value storage
    pub data: HashMap<String, String>,
    /// Metadata about the state
    pub metadata: KvMetadata,
}

/// Metadata about the key-value store state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvMetadata {
    /// Total number of operations applied
    pub operation_count: u64,
    /// When the state was last updated
    pub last_updated: chrono::DateTime<chrono::Utc>,
    /// Version of the state (incremented on each change)
    pub version: u64,
}

impl Default for KvState {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            metadata: KvMetadata {
                operation_count: 0,
                last_updated: chrono::Utc::now(),
                version: 0,
            },
        }
    }
}

impl KvState {
    /// Create a new empty key-value state
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the number of keys in the store
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    /// Get all keys
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
    }

    /// Get all key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.data.iter()
    }
}

/// Simple key-value store implementation
pub struct KeyValueStore {
    state: KvState,
}

impl KeyValueStore {
    /// Create a new key-value store
    pub fn new() -> Self {
        Self {
            state: KvState::new(),
        }
    }

    /// Create a new key-value store with initial state
    pub fn with_state(state: KvState) -> Self {
        Self { state }
    }

    /// Update the metadata after applying a command
    fn update_metadata(&mut self) {
        self.state.metadata.operation_count += 1;
        self.state.metadata.last_updated = chrono::Utc::now();
        self.state.metadata.version += 1;
    }
}

impl Default for KeyValueStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateMachine for KeyValueStore {
    type Command = KvCommand;
    type State = KvState;
    type Response = KvResponse;
    type Query = KvQuery;
    type QueryResponse = KvQueryResponse;
    async fn apply_command(&mut self, command: Self::Command) -> Result<Self::Response, crate::error::RaftError> {
        debug!("Applying KV command: {:?}", command);

        match command {
            KvCommand::Set { key, value } => {
                let old_value = self.state.data.insert(key.clone(), value.clone());
                self.update_metadata();
                
                if let Some(old) = old_value {
                    info!("Updated key '{}': '{}' -> '{}'", key, old, value);
                } else {
                    info!("Set key '{}' to '{}'", key, value);
                }
            }
            
            KvCommand::Delete { key } => {
                if let Some(old_value) = self.state.data.remove(&key) {
                    self.update_metadata();
                    info!("Deleted key '{}' (was '{}')", key, old_value);
                } else {
                    info!("Attempted to delete non-existent key '{}'", key);
                    // Still update metadata to track the operation attempt
                    self.update_metadata();
                }
            }
            
            KvCommand::Clear => {
                let old_count = self.state.data.len();
                self.state.data.clear();
                self.update_metadata();
                info!("Cleared {} keys from store", old_count);
            }
        }

        Ok(KvResponse::Ok)
    }

    async fn create_snapshot(&self) -> Result<Self::State, crate::error::RaftError> {
        debug!("Creating KV snapshot with {} keys", self.state.len());
        Ok(self.state.clone())
    }

    async fn restore_from_snapshot(&mut self, snapshot: Self::State) -> Result<(), crate::error::RaftError> {
        debug!(
            "Restoring KV state from snapshot: {} keys, version {}",
            snapshot.len(), 
            snapshot.metadata.version
        );
        
        self.state = snapshot;
        
        info!(
            "Restored KV state: {} keys, {} operations, version {}",
            self.state.len(),
            self.state.metadata.operation_count,
            self.state.metadata.version
        );
        
        Ok(())
    }

    fn get_current_state(&self) -> &Self::State {
        &self.state
    }

    async fn execute_query(&self, query: Self::Query) -> Result<Self::QueryResponse, crate::error::RaftError> {
        debug!("Executing KV query: {:?}", query);

        match query {
            KvQuery::Get { key } => {
                let value = self.state.data.get(&key).cloned();
                Ok(KvQueryResponse::Value(value))
            }
            
            KvQuery::List { prefix } => {
                let keys: Vec<String> = if let Some(prefix) = prefix {
                    self.state.data.keys()
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect()
                } else {
                    self.state.data.keys().cloned().collect()
                };
                Ok(KvQueryResponse::Keys(keys))
            }
            
            KvQuery::Exists { key } => {
                let exists = self.state.data.contains_key(&key);
                Ok(KvQueryResponse::Boolean(exists))
            }
            
            KvQuery::Size => {
                let size = self.state.data.len();
                Ok(KvQueryResponse::Number(size))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kv_store_basic_operations() {
        let mut kv = KeyValueStore::new();
        
        // Test set operation
        kv.apply_command(KvCommand::Set {
            key: "key1".to_string(),
            value: "value1".to_string(),
        }).await.unwrap();
        
        assert_eq!(kv.get_current_state().get("key1"), Some(&"value1".to_string()));
        assert_eq!(kv.get_current_state().len(), 1);
        assert_eq!(kv.get_current_state().metadata.operation_count, 1);
        
        // Test update operation
        kv.apply_command(KvCommand::Set {
            key: "key1".to_string(),
            value: "updated_value".to_string(),
        }).await.unwrap();
        
        assert_eq!(kv.get_current_state().get("key1"), Some(&"updated_value".to_string()));
        assert_eq!(kv.get_current_state().len(), 1);
        assert_eq!(kv.get_current_state().metadata.operation_count, 2);
        
        // Test delete operation
        kv.apply_command(KvCommand::Delete {
            key: "key1".to_string(),
        }).await.unwrap();
        
        assert_eq!(kv.get_current_state().get("key1"), None);
        assert_eq!(kv.get_current_state().len(), 0);
        assert_eq!(kv.get_current_state().metadata.operation_count, 3);
    }

    #[tokio::test]
    async fn test_kv_store_clear() {
        let mut kv = KeyValueStore::new();
        
        // Add some data
        kv.apply_command(KvCommand::Set {
            key: "key1".to_string(),
            value: "value1".to_string(),
        }).await.unwrap();
        
        kv.apply_command(KvCommand::Set {
            key: "key2".to_string(),
            value: "value2".to_string(),
        }).await.unwrap();
        
        assert_eq!(kv.get_current_state().len(), 2);
        
        // Clear all data
        kv.apply_command(KvCommand::Clear).await.unwrap();
        
        assert_eq!(kv.get_current_state().len(), 0);
        assert!(kv.get_current_state().is_empty());
        assert_eq!(kv.get_current_state().metadata.operation_count, 3);
    }

    #[tokio::test]
    async fn test_snapshot_and_restore() {
        let mut kv1 = KeyValueStore::new();
        
        // Add some data to first store
        kv1.apply_command(KvCommand::Set {
            key: "key1".to_string(),
            value: "value1".to_string(),
        }).await.unwrap();
        
        kv1.apply_command(KvCommand::Set {
            key: "key2".to_string(),
            value: "value2".to_string(),
        }).await.unwrap();
        
        // Create snapshot
        let snapshot = kv1.create_snapshot().await.unwrap();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot.metadata.operation_count, 2);
        
        // Create second store and restore from snapshot
        let mut kv2 = KeyValueStore::new();
        kv2.restore_from_snapshot(snapshot).await.unwrap();
        
        // Verify restored state
        assert_eq!(kv2.get_current_state().len(), 2);
        assert_eq!(kv2.get_current_state().get("key1"), Some(&"value1".to_string()));
        assert_eq!(kv2.get_current_state().get("key2"), Some(&"value2".to_string()));
        assert_eq!(kv2.get_current_state().metadata.operation_count, 2);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_key() {
        let mut kv = KeyValueStore::new();
        
        // Delete non-existent key should not fail
        kv.apply_command(KvCommand::Delete {
            key: "nonexistent".to_string(),
        }).await.unwrap();
        
        assert_eq!(kv.get_current_state().len(), 0);
        assert_eq!(kv.get_current_state().metadata.operation_count, 1);
    }
}