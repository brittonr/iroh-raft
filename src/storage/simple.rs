//! Simple redb-based storage implementation for Raft
//!
//! This module provides a clean, simplified storage backend that implements
//! the raft::Storage trait without dependencies on blixard-core types.

use crate::error::{RaftError as Error, Result};
use crate::storage::codec::*;

use raft::prelude::{ConfState, Entry, HardState, Snapshot};
use raft::{GetEntriesContext, RaftState, Storage, StorageError};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;
use tracing::debug;

// Table definitions
const RAFT_LOG_TABLE: TableDefinition<'_, u64, &[u8]> = TableDefinition::new("raft_log");
const HARD_STATE_TABLE: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("hard_state");
const CONF_STATE_TABLE: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("conf_state");
const SNAPSHOT_TABLE: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("snapshot");

const HARD_STATE_KEY: &str = "hard_state";
const CONF_STATE_KEY: &str = "conf_state";
const SNAPSHOT_KEY: &str = "snapshot";

/// Simple Raft storage implementation using redb
#[derive(Clone)]
pub struct RaftStorage {
    database: Arc<Database>,
}

impl RaftStorage {
    /// Create a new storage instance
    pub async fn new<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        tokio::fs::create_dir_all(data_dir).await
            .map_err(|e| Error::Io { 
                operation: "create data directory".to_string(), 
                source: e,
                backtrace: snafu::Backtrace::new(),
            })?;

        let db_path = data_dir.join("raft.db");
        let database = Arc::new(
            Database::create(&db_path)
                .map_err(|e| Error::storage_error("create database", e))?
        );

        // Initialize tables
        let write_txn = database.begin_write()
            .map_err(|e| Error::storage_error("begin write transaction", e))?;

        {
            let _ = write_txn.open_table(RAFT_LOG_TABLE)
                .map_err(|e| Error::storage_error("open raft_log table", e))?;
            let _ = write_txn.open_table(HARD_STATE_TABLE)
                .map_err(|e| Error::storage_error("open hard_state table", e))?;
            let _ = write_txn.open_table(CONF_STATE_TABLE)
                .map_err(|e| Error::storage_error("open conf_state table", e))?;
            let _ = write_txn.open_table(SNAPSHOT_TABLE)
                .map_err(|e| Error::storage_error("open snapshot table", e))?;
        }

        write_txn.commit()
            .map_err(|e| Error::storage_error("commit table initialization", e))?;

        Ok(Self { database })
    }

    /// Get the applied index
    pub fn applied_index(&self) -> Result<u64> {
        let read_txn = self.database.begin_read()
            .map_err(|e| Error::storage_error("begin read transaction", e))?;

        let hard_state_table = read_txn.open_table(HARD_STATE_TABLE)
            .map_err(|e| Error::storage_error("open hard_state table", e))?;

        if let Some(data) = hard_state_table.get(HARD_STATE_KEY)
            .map_err(|e| Error::storage_error("get hard state", e))? {
            let hard_state: HardState = deserialize_hard_state(data.value())
                .map_err(|e| e)?;
            Ok(hard_state.commit)
        } else {
            Ok(0)
        }
    }

    /// Save hard state
    pub fn save_hard_state(&self, hard_state: &HardState) -> Result<()> {
        let write_txn = self.database.begin_write()
            .map_err(|e| Error::storage_error("begin write transaction", e))?;

        {
            let mut table = write_txn.open_table(HARD_STATE_TABLE)
                .map_err(|e| Error::storage_error("open hard_state table", e))?;

            let data = serialize_hard_state(hard_state)
                .map_err(|e| e)?;

            table.insert(HARD_STATE_KEY, &data[..])
                .map_err(|e| Error::storage_error("insert hard state", e))?;
        }

        write_txn.commit()
            .map_err(|e| Error::storage_error("commit hard state", e))?;

        debug!("Saved hard state: term={}, vote={}, commit={}", 
               hard_state.term, hard_state.vote, hard_state.commit);
        Ok(())
    }

    /// Append entries to the log
    pub fn append(&self, entries: &[Entry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let write_txn = self.database.begin_write()
            .map_err(|e| Error::storage_error("begin write transaction", e))?;

        {
            let mut log_table = write_txn.open_table(RAFT_LOG_TABLE)
                .map_err(|e| Error::storage_error("open raft_log table", e))?;

            for entry in entries {
                let data = serialize_entry(entry)
                    .map_err(|e| e)?;

                log_table.insert(entry.index, &data[..])
                    .map_err(|e| Error::storage_error("insert log entry", e))?;
            }
        }

        write_txn.commit()
            .map_err(|e| Error::storage_error("commit entries", e))?;

        let last_index = entries.last()
            .map(|entry| entry.index)
            .unwrap_or(0);
        debug!("Appended {} entries, first_index={}, last_index={}", 
               entries.len(), entries[0].index, last_index);
        Ok(())
    }

    /// Apply a snapshot
    pub fn apply_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        let write_txn = self.database.begin_write()
            .map_err(|e| Error::storage_error("begin write transaction", e))?;

        // Clear existing log entries that are covered by the snapshot
        if let Some(metadata) = &snapshot.metadata {
            let mut log_table = write_txn.open_table(RAFT_LOG_TABLE)
                .map_err(|e| Error::storage_error("open raft_log table", e))?;

            // Remove all entries up to and including the snapshot index
            let entries_to_remove: Vec<u64> = log_table.range(..=metadata.index)
                .map_err(|e| Error::storage_error("range log entries", e))?
                .map(|result| result.map(|(key, _)| key.value()).map_err(|e| Error::storage_error("read log key", e)))
                .collect::<Result<Vec<_>>>()?;

            for index in entries_to_remove {
                log_table.remove(index)
                    .map_err(|e| Error::storage_error("remove log entry", e))?;
            }

            // Save the snapshot
            let mut snapshot_table = write_txn.open_table(SNAPSHOT_TABLE)
                .map_err(|e| Error::storage_error("open snapshot table", e))?;

            let data = serialize_snapshot(&snapshot)
                .map_err(|e| e)?;

            snapshot_table.insert(SNAPSHOT_KEY, &data[..])
                .map_err(|e| Error::storage_error("insert snapshot", e))?;

            // Update conf state if present
            if let Some(conf_state) = &metadata.conf_state {
                let mut conf_table = write_txn.open_table(CONF_STATE_TABLE)
                    .map_err(|e| Error::storage_error("open conf_state table", e))?;

                let conf_data = serialize_conf_state(conf_state)
                    .map_err(|e| e)?;

                conf_table.insert(CONF_STATE_KEY, &conf_data[..])
                    .map_err(|e| Error::storage_error("insert conf state", e))?;
            }
        }

        write_txn.commit()
            .map_err(|e| Error::storage_error("commit snapshot", e))?;

        debug!("Applied snapshot at index={}", 
               snapshot.metadata.as_ref().map(|m| m.index).unwrap_or(0));
        Ok(())
    }

    /// Compact log entries up to the given index
    pub fn compact(&self, compact_index: u64) -> Result<()> {
        let write_txn = self.database.begin_write()
            .map_err(|e| Error::storage_error("begin write transaction", e))?;

        {
            let mut log_table = write_txn.open_table(RAFT_LOG_TABLE)
                .map_err(|e| Error::storage_error("open raft_log table", e))?;

            // Remove all entries up to (but not including) compact_index
            let entries_to_remove: Vec<u64> = log_table.range(..compact_index)
                .map_err(|e| Error::storage_error("range log entries", e))?
                .map(|result| result.map(|(key, _)| key.value()).map_err(|e| Error::storage_error("read log key", e)))
                .collect::<Result<Vec<_>>>()?;

            for index in entries_to_remove {
                log_table.remove(index)
                    .map_err(|e| Error::storage_error("remove log entry", e))?;
            }
        }

        write_txn.commit()
            .map_err(|e| Error::storage_error("commit compaction", e))?;

        debug!("Compacted log up to index {}", compact_index);
        Ok(())
    }
}

impl Storage for RaftStorage {
    fn initial_state(&self) -> raft::Result<RaftState> {
        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        // Read hard state
        let hard_state = {
            let hard_state_table = read_txn.open_table(HARD_STATE_TABLE)
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            if let Some(data) = hard_state_table.get(HARD_STATE_KEY)
                .map_err(|e| StorageError::Other(Box::new(e)))? {
                deserialize_hard_state(data.value())
                    .map_err(|e| StorageError::Other(Box::new(e)))?
            } else {
                HardState::default()
            }
        };

        // Read conf state
        let conf_state = {
            let conf_state_table = read_txn.open_table(CONF_STATE_TABLE)
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            if let Some(data) = conf_state_table.get(CONF_STATE_KEY)
                .map_err(|e| StorageError::Other(Box::new(e)))? {
                deserialize_conf_state(data.value())
                    .map_err(|e| StorageError::Other(Box::new(e)))?
            } else {
                ConfState::default()
            }
        };

        Ok(RaftState { hard_state, conf_state })
    }

    fn entries(&self, low: u64, high: u64, max_size: impl Into<Option<u64>>, _context: GetEntriesContext) -> raft::Result<Vec<Entry>> {
        let max_size = max_size.into().unwrap_or(u64::MAX);
        let mut entries = Vec::new();
        let mut total_size = 0u64;

        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        let log_table = read_txn.open_table(RAFT_LOG_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        for index in low..high {
            if let Some(data) = log_table.get(index)
                .map_err(|e| StorageError::Other(Box::new(e)))? {
                
                let entry_data = data.value();
                let entry_size = entry_data.len() as u64;

                if total_size + entry_size > max_size {
                    break;
                }

                let entry = deserialize_entry(entry_data)
                    .map_err(|e| StorageError::Other(Box::new(e)))?;

                entries.push(entry);
                total_size += entry_size;
            } else {
                return Err(raft::Error::Store(StorageError::Unavailable).into());
            }
        }

        Ok(entries)
    }

    fn term(&self, index: u64) -> raft::Result<u64> {
        if index == 0 {
            return Ok(0);
        }

        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        // Check snapshot first
        let snapshot_table = read_txn.open_table(SNAPSHOT_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(data) = snapshot_table.get(SNAPSHOT_KEY)
            .map_err(|e| StorageError::Other(Box::new(e)))? {
            
            let snapshot = deserialize_snapshot(data.value())
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            if let Some(metadata) = &snapshot.metadata {
                if index == metadata.index {
                    return Ok(metadata.term);
                }
            }
        }

        // Check log entries
        let log_table = read_txn.open_table(RAFT_LOG_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(data) = log_table.get(index)
            .map_err(|e| StorageError::Other(Box::new(e)))? {
            
            let entry = deserialize_entry(data.value())
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            Ok(entry.term)
        } else {
            Err(raft::Error::Store(StorageError::Unavailable).into())
        }
    }

    fn first_index(&self) -> raft::Result<u64> {
        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        // Check snapshot first
        let snapshot_table = read_txn.open_table(SNAPSHOT_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(data) = snapshot_table.get(SNAPSHOT_KEY)
            .map_err(|e| StorageError::Other(Box::new(e)))? {
            
            let snapshot = deserialize_snapshot(data.value())
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            if let Some(metadata) = &snapshot.metadata {
                return Ok(metadata.index + 1);
            }
        }

        // Check log entries
        let log_table = read_txn.open_table(RAFT_LOG_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(first) = log_table.range::<u64>(..)
            .map_err(|e| StorageError::Other(Box::new(e)))?
            .next() {
            
            let (key, _) = first.map_err(|e| StorageError::Other(Box::new(e)))?;
            Ok(key.value())
        } else {
            Ok(1)
        }
    }

    fn last_index(&self) -> raft::Result<u64> {
        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        let log_table = read_txn.open_table(RAFT_LOG_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(last) = log_table.range::<u64>(..)
            .map_err(|e| StorageError::Other(Box::new(e)))?
            .next_back() {
            
            let (key, _) = last.map_err(|e| StorageError::Other(Box::new(e)))?;
            Ok(key.value())
        } else {
            // Check if we have a snapshot
            let snapshot_table = read_txn.open_table(SNAPSHOT_TABLE)
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            if let Some(data) = snapshot_table.get(SNAPSHOT_KEY)
                .map_err(|e| StorageError::Other(Box::new(e)))? {
                
                let snapshot = deserialize_snapshot(data.value())
                    .map_err(|e| StorageError::Other(Box::new(e)))?;

                if let Some(metadata) = &snapshot.metadata {
                    return Ok(metadata.index);
                }
            }

            Ok(0)
        }
    }

    fn snapshot(&self, request_index: u64, _to: u64) -> raft::Result<Snapshot> {
        let read_txn = self.database.begin_read()
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        let snapshot_table = read_txn.open_table(SNAPSHOT_TABLE)
            .map_err(|e| StorageError::Other(Box::new(e)))?;

        if let Some(data) = snapshot_table.get(SNAPSHOT_KEY)
            .map_err(|e| StorageError::Other(Box::new(e)))? {
            
            let snapshot = deserialize_snapshot(data.value())
                .map_err(|e| StorageError::Other(Box::new(e)))?;

            // Check if the snapshot is recent enough
            if let Some(metadata) = &snapshot.metadata {
                if metadata.index >= request_index {
                    return Ok(snapshot);
                }
            }
        }

        // Return temporary unavailable - snapshot will be created later
        Err(raft::Error::Store(StorageError::SnapshotTemporarilyUnavailable).into())
    }
}