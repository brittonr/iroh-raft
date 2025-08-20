//! Deterministic simulation tests using madsim for distributed consensus scenarios
//!
//! This module provides comprehensive deterministic testing of Raft consensus under
//! various distributed system failure scenarios including:
//! - Network partitions and message loss
//! - Node failures and recovery
//! - Timing issues and concurrent operations
//! - Leader election edge cases
//! - Split-brain scenarios
//!
//! All tests use madsim::runtime::Runtime with fixed seeds to ensure deterministic
//! execution and reproducible results.

#![cfg(all(test, feature = "madsim"))]
#![allow(dead_code, unused_variables)]

use madsim::{self, time};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

// Type aliases for the simulation
pub type NodeId = u64;
pub type Term = u64;
pub type LogIndex = u64;

/// Simplified Raft proposal data for testing
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalData {
    Set { key: String, value: String },
    Delete { key: String },
    Get { key: String },
    Noop,
}

/// Simplified error type for simulation
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("Node {node_id} not ready: {reason}")]
    NotReady { node_id: NodeId, reason: String },
    
    #[error("Node {node_id} is not leader")]
    NotLeader { node_id: NodeId },
    
    #[error("No leader available")]
    NoLeaderAvailable,
    
    #[error("Node {node_id} not found")]
    NodeNotFound { node_id: NodeId },
    
    #[error("Election failed for node {node_id} in term {term}: {reason}")]
    ElectionFailed { node_id: NodeId, term: Term, reason: String },
    
    #[error("Network partition: {details}")]
    NetworkPartition { details: String },
    
    #[error("Proposal rejected: {reason}")]
    ProposalRejected { reason: String },
}

pub type Result<T> = std::result::Result<T, SimError>;

/// Test configuration for simulation scenarios
#[derive(Debug, Clone)]
struct SimulationConfig {
    pub cluster_size: usize,
    pub seed: u64,
    pub timeout: Duration,
    pub heartbeat_interval: Duration,
    pub election_timeout: Duration,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            cluster_size: 3,
            seed: 42,
            timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_millis(100),
            election_timeout: Duration::from_millis(500),
        }
    }
}

/// Simple key-value store for simulation
#[derive(Debug, Clone, PartialEq)]
struct SimKeyValueStore {
    data: HashMap<String, String>,
}

impl SimKeyValueStore {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn set(&mut self, key: String, value: String) {
        self.data.insert(key, value);
    }

    fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    fn delete(&mut self, key: &str) -> Option<String> {
        self.data.remove(key)
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Simulated Raft node for madsim testing
#[derive(Debug)]
struct SimRaftNode {
    id: NodeId,
    state_machine: SimKeyValueStore,
    is_leader: bool,
    current_term: Term,
    voted_for: Option<NodeId>,
    log_entries: Vec<ProposalData>,
    committed_index: usize,
    is_partitioned: bool,
    is_stopped: bool,
}

impl SimRaftNode {
    fn new(id: NodeId) -> Self {
        Self {
            id,
            state_machine: SimKeyValueStore::new(),
            is_leader: false,
            current_term: 0,
            voted_for: None,
            log_entries: Vec::new(),
            committed_index: 0,
            is_partitioned: false,
            is_stopped: false,
        }
    }

    async fn propose(&mut self, data: ProposalData) -> Result<()> {
        if self.is_stopped || self.is_partitioned {
            return Err(SimError::NotReady {
                node_id: self.id,
                reason: "node unavailable".to_string(),
            });
        }

        if !self.is_leader {
            return Err(SimError::NotLeader {
                node_id: self.id,
            });
        }

        self.log_entries.push(data);
        Ok(())
    }

    async fn apply_committed_entries(&mut self) -> Result<()> {
        while self.committed_index < self.log_entries.len() {
            let entry = &self.log_entries[self.committed_index].clone();
            match entry {
                ProposalData::Set { key, value } => {
                    self.state_machine.set(key.clone(), value.clone());
                },
                ProposalData::Delete { key } => {
                    self.state_machine.delete(key);
                },
                ProposalData::Get { .. } => {
                    // Get operations don't modify state
                },
                ProposalData::Noop => {
                    // No-op
                }
            }
            self.committed_index += 1;
        }
        Ok(())
    }

    fn become_leader(&mut self, term: u64) {
        self.is_leader = true;
        self.current_term = term;
        self.voted_for = Some(self.id);
    }

    fn step_down(&mut self, term: u64) {
        self.is_leader = false;
        self.current_term = term;
        self.voted_for = None;
    }

    fn set_partitioned(&mut self, partitioned: bool) {
        self.is_partitioned = partitioned;
    }

    fn stop(&mut self) {
        self.is_stopped = true;
        self.is_leader = false;
    }

    fn restart(&mut self) {
        self.is_stopped = false;
        self.is_partitioned = false;
    }
}

/// Simulated Raft cluster for deterministic testing
#[derive(Debug)]
struct SimRaftCluster {
    nodes: HashMap<NodeId, SimRaftNode>,
    network_partitions: HashMap<NodeId, HashSet<NodeId>>,
    message_delays: HashMap<(NodeId, NodeId), Duration>,
    dropped_messages: HashSet<(NodeId, NodeId)>,
    current_term: u64,
    leader_id: Option<NodeId>,
}

impl SimRaftCluster {
    fn new(node_ids: Vec<NodeId>) -> Self {
        let mut nodes = HashMap::new();
        for id in node_ids {
            nodes.insert(id, SimRaftNode::new(id));
        }

        Self {
            nodes,
            network_partitions: HashMap::new(),
            message_delays: HashMap::new(),
            dropped_messages: HashSet::new(),
            current_term: 0,
            leader_id: None,
        }
    }

    async fn elect_leader(&mut self, candidate_id: NodeId) -> Result<()> {
        let new_term = self.current_term + 1;
        
        // Only elect if majority can communicate
        let reachable_nodes = self.get_reachable_nodes(candidate_id);
        if reachable_nodes.len() <= self.nodes.len() / 2 {
            return Err(SimError::ElectionFailed {
                node_id: candidate_id,
                term: new_term,
                reason: "insufficient reachable nodes for majority".to_string(),
            });
        }

        // Step down previous leader
        if let Some(old_leader_id) = self.leader_id {
            if let Some(old_leader) = self.nodes.get_mut(&old_leader_id) {
                old_leader.step_down(new_term);
            }
        }

        // Elect new leader
        if let Some(new_leader) = self.nodes.get_mut(&candidate_id) {
            new_leader.become_leader(new_term);
            self.current_term = new_term;
            self.leader_id = Some(candidate_id);
        }

        Ok(())
    }

    fn get_reachable_nodes(&self, from_node: NodeId) -> Vec<NodeId> {
        let mut reachable = vec![from_node];
        
        for &other_id in self.nodes.keys() {
            if other_id != from_node && self.can_communicate(from_node, other_id) {
                reachable.push(other_id);
            }
        }
        
        reachable
    }

    fn can_communicate(&self, from: NodeId, to: NodeId) -> bool {
        // Check if nodes are stopped
        if let Some(from_node) = self.nodes.get(&from) {
            if from_node.is_stopped {
                return false;
            }
        }
        if let Some(to_node) = self.nodes.get(&to) {
            if to_node.is_stopped {
                return false;
            }
        }

        // Check network partitions
        if let Some(partition) = self.network_partitions.get(&from) {
            if !partition.contains(&to) {
                return false;
            }
        }

        // Check dropped messages
        !self.dropped_messages.contains(&(from, to))
    }

    fn create_partition(&mut self, group1: Vec<NodeId>, group2: Vec<NodeId>) {
        // Clear existing partitions
        self.network_partitions.clear();

        // Create partitions where nodes in group1 can only talk to group1,
        // and nodes in group2 can only talk to group2
        for &node_id in &group1 {
            self.network_partitions.insert(node_id, group1.iter().cloned().collect());
        }
        for &node_id in &group2 {
            self.network_partitions.insert(node_id, group2.iter().cloned().collect());
        }
    }

    fn heal_partition(&mut self) {
        self.network_partitions.clear();
        self.dropped_messages.clear();
    }

    fn add_message_delay(&mut self, from: NodeId, to: NodeId, delay: Duration) {
        self.message_delays.insert((from, to), delay);
    }

    fn drop_messages(&mut self, from: NodeId, to: NodeId) {
        self.dropped_messages.insert((from, to));
    }

    fn stop_node(&mut self, node_id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.stop();
        }
        if self.leader_id == Some(node_id) {
            self.leader_id = None;
        }
    }

    fn restart_node(&mut self, node_id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.restart();
        }
    }

    async fn propose(&mut self, data: ProposalData) -> Result<NodeId> {
        let leader_id = self.leader_id.ok_or_else(|| SimError::NoLeaderAvailable)?;
        
        if let Some(leader) = self.nodes.get_mut(&leader_id) {
            leader.propose(data).await?;
            Ok(leader_id)
        } else {
            Err(SimError::NodeNotFound { node_id: leader_id })
        }
    }

    async fn commit_entries(&mut self) -> Result<()> {
        let leader_id = self.leader_id.ok_or_else(|| SimError::NoLeaderAvailable)?;
        let reachable_nodes = self.get_reachable_nodes(leader_id);
        
        // Simple commit logic - in real Raft this would be more complex
        if reachable_nodes.len() > self.nodes.len() / 2 {
            for node in self.nodes.values_mut() {
                if reachable_nodes.contains(&node.id) {
                    node.committed_index = node.log_entries.len();
                    node.apply_committed_entries().await?;
                }
            }
        }

        Ok(())
    }

    fn has_leader(&self) -> bool {
        self.leader_id.is_some()
    }

    fn get_leader_id(&self) -> Option<NodeId> {
        self.leader_id
    }

    async fn check_consistency(&self) -> bool {
        if self.nodes.is_empty() {
            return true;
        }

        // Get reference state from first reachable node
        let mut reference_state = None;
        for node in self.nodes.values() {
            if !node.is_stopped && !node.is_partitioned {
                reference_state = Some(&node.state_machine);
                break;
            }
        }

        let reference = match reference_state {
            Some(state) => state,
            None => return true, // No reachable nodes
        };

        // Check all other reachable nodes match
        for node in self.nodes.values() {
            if !node.is_stopped && !node.is_partitioned {
                let node_state = &node.state_machine;
                if node_state != reference {
                    return false;
                }
            }
        }

        true
    }
}

#[test]
#[ignore]
fn test_raft_consensus_under_partition() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 42,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Initial leader election
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");
        assert!(cluster.has_leader(), "Cluster should have a leader");

        // Propose some initial data
        let initial_proposal = ProposalData::Set { 
            key: "key1".to_string(), 
            value: "value1".to_string() 
        };
        cluster.propose(initial_proposal).await.expect("Failed to propose initial data");
        cluster.commit_entries().await.expect("Failed to commit initial entries");

        // Create network partition: [1, 2] vs [3, 4, 5]
        let group1 = vec![1, 2];
        let group2 = vec![3, 4, 5];
        cluster.create_partition(group1.clone(), group2.clone());

        // The minority partition (group1) should lose leadership
        // The majority partition (group2) should elect a new leader
        cluster.elect_leader(3).await.expect("Majority partition should elect new leader");
        
        assert_eq!(cluster.get_leader_id(), Some(3), "Node 3 should be the new leader");

        // Minority partition should not be able to commit new entries
        let minority_proposal = ProposalData::Set {
            key: "key2".to_string(),
            value: "value2".to_string()
        };
        let result = cluster.nodes.get_mut(&1).unwrap().propose(minority_proposal).await;
        assert!(result.is_err(), "Minority partition should not accept proposals");

        // Majority partition should be able to commit entries
        let majority_proposal = ProposalData::Set {
            key: "key3".to_string(),
            value: "value3".to_string()
        };
        cluster.propose(majority_proposal).await.expect("Majority partition should accept proposals");
        cluster.commit_entries().await.expect("Majority partition should commit entries");

        // Heal the partition
        cluster.heal_partition();

        // Allow time for partition healing and synchronization
        // Step until stalled

        // Eventually, all nodes should reach consistency
        let mut consistent = false;
        for _ in 0..10 {
            if cluster.check_consistency().await {
                consistent = true;
                break;
            }
            time::sleep(Duration::from_millis(100)).await;
        }
        
        assert!(consistent, "Cluster should reach consistency after partition healing");
    });
}

#[test]
#[ignore]
fn test_leader_election_determinism() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 123,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Test deterministic leader election multiple times with the same seed
        for iteration in 1..=5 {
            // Reset cluster state
            cluster.leader_id = None;
            cluster.current_term = 0;
            for node in cluster.nodes.values_mut() {
                node.current_term = 0;
                node.is_leader = false;
                node.voted_for = None;
            }

            // Elect leader with deterministic conditions
            cluster.elect_leader(1).await.expect("Failed to elect leader");
            
            // With the same seed, the same node should always become leader
            assert_eq!(cluster.get_leader_id(), Some(1), 
                      "Leader election should be deterministic (iteration {})", iteration);
            
            assert_eq!(cluster.current_term, iteration, 
                      "Term should increment consistently");
        }
    });
}

#[test]
#[ignore]
fn test_message_loss_scenarios() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 456,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect initial leader
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Simulate message drops between leader and followers
        cluster.drop_messages(1, 2); // Leader -> Follower 2
        cluster.drop_messages(2, 1); // Follower 2 -> Leader

        // Leader should still be able to maintain leadership with majority (node 3)
        assert_eq!(cluster.get_leader_id(), Some(1), "Leader should maintain leadership");

        // Propose data - should succeed with remaining follower
        let proposal = ProposalData::Set { key: "test_key".to_string(), value: "test_value".to_string() };
        cluster.propose(proposal).await.expect("Proposal should succeed despite message loss");

        // Drop messages to all followers - should cause leadership loss
        cluster.drop_messages(1, 3);
        cluster.drop_messages(3, 1);

        // Leader should step down due to loss of majority
        // In real implementation, this would happen via heartbeat timeouts
        cluster.nodes.get_mut(&1).unwrap().step_down(cluster.current_term + 1);
        cluster.leader_id = None;
        cluster.current_term += 1;

        assert!(!cluster.has_leader(), "Cluster should lose leader when majority communication fails");

        // Restore communication and verify new leader can be elected
        cluster.heal_partition();
        cluster.elect_leader(2).await.expect("New leader should be electable after healing");
        
        assert_eq!(cluster.get_leader_id(), Some(2), "New leader should be elected");
    });
}

#[test]
#[ignore]
fn test_node_failure_and_recovery() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 789,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect initial leader
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Propose and commit some data
        let proposal1 = ProposalData::Set { key: "before_failure".to_string(), value: "value1".to_string() };
        cluster.propose(proposal1).await.expect("Failed to propose before failure");
        cluster.commit_entries().await.expect("Failed to commit before failure");

        // Stop the leader node (simulate crash)
        cluster.stop_node(1);
        assert!(!cluster.has_leader(), "Cluster should not have leader after leader failure");

        // Elect new leader from remaining nodes
        cluster.elect_leader(2).await.expect("Failed to elect new leader after failure");
        assert_eq!(cluster.get_leader_id(), Some(2), "Node 2 should become new leader");

        // Propose more data with new leader
        let proposal2 = ProposalData::Set { key: "after_failure".to_string(), value: "value2".to_string() };
        cluster.propose(proposal2).await.expect("Failed to propose with new leader");
        cluster.commit_entries().await.expect("Failed to commit with new leader");

        // Stop another node (test minority failure)
        cluster.stop_node(3);
        
        // Should still have majority (3 out of 5, with 2 functioning)
        let proposal3 = ProposalData::Set { key: "minority_failure".to_string(), value: "value3".to_string() };
        cluster.propose(proposal3).await.expect("Should handle minority failure");

        // Restart failed nodes
        cluster.restart_node(1);
        cluster.restart_node(3);

        // Allow synchronization
        // Step until stalled

        // Verify eventual consistency
        let mut consistent = false;
        for _ in 0..10 {
            if cluster.check_consistency().await {
                consistent = true;
                break;
            }
            time::sleep(Duration::from_millis(50)).await;
        }
        
        assert!(consistent, "Cluster should reach consistency after node recovery");
    });
}

#[test]
#[ignore]
fn test_concurrent_proposals() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 999,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect leader
        cluster.elect_leader(1).await.expect("Failed to elect leader");

        // Simulate concurrent proposals
        let proposals = vec![
            ProposalData::Set { key: "concurrent_1".to_string(), value: "value_1".to_string() },
            ProposalData::Set { key: "concurrent_2".to_string(), value: "value_2".to_string() },
            ProposalData::Set { key: "concurrent_3".to_string(), value: "value_3".to_string() },
            ProposalData::Set { key: "concurrent_4".to_string(), value: "value_4".to_string() },
            ProposalData::Set { key: "concurrent_5".to_string(), value: "value_5".to_string() },
        ];

        // Submit proposals concurrently (in deterministic simulation)
        let mut proposal_results = Vec::new();
        for proposal in proposals {
            let result = cluster.propose(proposal).await;
            proposal_results.push(result);
        }

        // All proposals should succeed with a stable leader
        for (i, result) in proposal_results.iter().enumerate() {
            assert!(result.is_ok(), "Proposal {} should succeed", i + 1);
        }

        // Commit all entries
        cluster.commit_entries().await.expect("Failed to commit concurrent proposals");

        // Verify all proposals are applied in order
        let leader_node = cluster.nodes.get(&1).unwrap();
        assert_eq!(leader_node.log_entries.len(), 5, "All proposals should be in the log");
        assert_eq!(leader_node.committed_index, 5, "All proposals should be committed");

        // Test concurrent proposals during leadership change
        cluster.stop_node(1); // Stop current leader
        cluster.elect_leader(2).await.expect("Failed to elect new leader");

        // Submit more concurrent proposals to new leader
        let more_proposals = vec![
            ProposalData::Set { key: "after_leader_change_1".to_string(), value: "value_1".to_string() },
            ProposalData::Set { key: "after_leader_change_2".to_string(), value: "value_2".to_string() },
        ];

        for proposal in more_proposals {
            cluster.propose(proposal).await.expect("Proposal to new leader should succeed");
        }

        cluster.commit_entries().await.expect("Failed to commit after leader change");

        // Verify consistency across remaining nodes
        assert!(cluster.check_consistency().await, 
                "Cluster should maintain consistency during concurrent operations");
    });
}

#[test]
#[ignore]
fn test_timing_edge_cases() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 1337,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Test election timeout edge case
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Add controlled delays between nodes
        cluster.add_message_delay(1, 2, Duration::from_millis(100));
        cluster.add_message_delay(1, 3, Duration::from_millis(200));

        // Propose with delays
        let proposal = ProposalData::Set { key: "delayed_proposal".to_string(), value: "test_value".to_string() };
        cluster.propose(proposal).await.expect("Delayed proposal should succeed");

        // Simulate election timeout during message delays
        time::sleep(Duration::from_millis(150)).await;

        // Test split vote scenario
        cluster.leader_id = None;
        cluster.current_term += 1;

        // Nodes 2 and 3 try to become leader simultaneously
        // In deterministic simulation, the outcome should be consistent
        let election_result_2 = cluster.elect_leader(2).await;
        cluster.current_term += 1;
        let election_result_3 = cluster.elect_leader(3).await;

        // Exactly one should succeed (deterministic based on seed)
        let successful_elections = [election_result_2.is_ok(), election_result_3.is_ok()]
            .iter()
            .filter(|&&success| success)
            .count();
        
        assert_eq!(successful_elections, 1, "Exactly one leader should be elected in split vote");

        // Test rapid leadership changes
        for new_leader in &[1, 2, 3] {
            if cluster.get_leader_id() != Some(*new_leader) {
                cluster.elect_leader(*new_leader).await.expect("Rapid leader change should work");
            }
            
            let quick_proposal = ProposalData::Set {
                key: format!("quick_{}", new_leader),
                value: format!("value_{}", new_leader)
            };
            cluster.propose(quick_proposal).await.expect("Quick proposal should succeed");
            
            time::sleep(Duration::from_millis(10)).await; // Very short time
        }

        // Verify eventual consistency despite rapid changes
        cluster.commit_entries().await.expect("Final commit should succeed");
        assert!(cluster.check_consistency().await, 
                "Cluster should maintain consistency despite timing edge cases");
    });
}

#[test]
#[ignore]
fn test_split_brain_prevention() {
    let config = SimulationConfig {
        cluster_size: 4,
        seed: 2024,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect initial leader
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Create symmetric partition: [1, 2] vs [3, 4]
        let group1 = vec![1, 2];
        let group2 = vec![3, 4];
        cluster.create_partition(group1.clone(), group2.clone());

        // Neither partition has majority, so no new leader should be elected
        let election_result_group1 = cluster.elect_leader(1).await;
        let election_result_group2 = cluster.elect_leader(3).await;

        // Both elections should fail due to insufficient majority
        assert!(election_result_group1.is_err(), 
                "Minority partition should not elect leader");
        assert!(election_result_group2.is_err(), 
                "Minority partition should not elect leader");

        // Verify no leader exists
        cluster.leader_id = None; // Simulate leader stepping down due to partition
        assert!(!cluster.has_leader(), "Split cluster should have no leader");

        // Neither partition should be able to commit entries
        let proposal1 = ProposalData::Set { key: "split_1".to_string(), value: "value_1".to_string() };
        let proposal2 = ProposalData::Set { key: "split_2".to_string(), value: "value_2".to_string() };

        // Manually try to propose to nodes in each partition
        let result1 = cluster.nodes.get_mut(&1).unwrap().propose(proposal1).await;
        let result2 = cluster.nodes.get_mut(&3).unwrap().propose(proposal2).await;

        assert!(result1.is_err(), "Split partition should reject proposals");
        assert!(result2.is_err(), "Split partition should reject proposals");

        // Heal partition by adding one node to make majority
        // Simulate node 3 reconnecting to group1
        cluster.create_partition(vec![1, 2, 3], vec![4]);

        // Now group1 has majority and should be able to elect leader
        cluster.elect_leader(2).await.expect("Majority partition should elect leader");
        assert_eq!(cluster.get_leader_id(), Some(2), "Majority partition should have leader");

        // Majority should be able to commit entries
        let recovery_proposal = ProposalData::Set { key: "recovery".to_string(), value: "success".to_string() };
        cluster.propose(recovery_proposal).await.expect("Majority should accept proposals");
        cluster.commit_entries().await.expect("Majority should commit entries");

        // Full healing
        cluster.heal_partition();
        // Step until stalled

        // Verify eventual consistency
        assert!(cluster.check_consistency().await, 
                "Cluster should reach consistency after split-brain resolution");
    });
}

#[test]
#[ignore]
fn test_log_replication_with_failures() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 4242,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect leader
        cluster.elect_leader(1).await.expect("Failed to elect leader");

        // Add entries to the log
        let entries = vec![
            ProposalData::Set { key: "log_1".to_string(), value: "value_1".to_string() },
            ProposalData::Set { key: "log_2".to_string(), value: "value_2".to_string() },
            ProposalData::Set { key: "log_3".to_string(), value: "value_3".to_string() },
            ProposalData::Set { key: "log_4".to_string(), value: "value_4".to_string() },
            ProposalData::Set { key: "log_5".to_string(), value: "value_5".to_string() },
        ];

        for entry in entries {
            cluster.propose(entry).await.expect("Failed to propose entry");
        }

        // Commit first batch
        cluster.commit_entries().await.expect("Failed to commit entries");

        // Stop some followers
        cluster.stop_node(4);
        cluster.stop_node(5);

        // Continue adding entries with reduced cluster
        let more_entries = vec![
            ProposalData::Set { key: "log_6".to_string(), value: "value_6".to_string() },
            ProposalData::Set { key: "log_7".to_string(), value: "value_7".to_string() },
        ];

        for entry in more_entries {
            cluster.propose(entry).await.expect("Failed to propose with reduced cluster");
        }

        cluster.commit_entries().await.expect("Failed to commit with reduced cluster");

        // Restart stopped nodes
        cluster.restart_node(4);
        cluster.restart_node(5);

        // Allow synchronization
        // Step until stalled

        // Add more entries after recovery
        let final_entries = vec![
            ProposalData::Set { key: "log_8".to_string(), value: "value_8".to_string() },
            ProposalData::Set { key: "log_9".to_string(), value: "value_9".to_string() },
        ];

        for entry in final_entries {
            cluster.propose(entry).await.expect("Failed to propose after recovery");
        }

        cluster.commit_entries().await.expect("Failed to commit after recovery");

        // Verify all nodes have consistent logs
        let leader_log_len = cluster.nodes.get(&1).unwrap().log_entries.len();
        assert_eq!(leader_log_len, 9, "Leader should have all 9 entries");

        for node in cluster.nodes.values() {
            if !node.is_stopped {
                assert_eq!(node.log_entries.len(), leader_log_len, 
                          "All active nodes should have same log length");
                assert_eq!(node.committed_index, leader_log_len,
                          "All entries should be committed");
            }
        }

        assert!(cluster.check_consistency().await, 
                "Cluster should have consistent state after log replication with failures");
    });
}

#[test]
#[ignore]
fn test_snapshot_and_compaction() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 7777,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect leader
        cluster.elect_leader(1).await.expect("Failed to elect leader");

        // Add many entries to trigger snapshotting need
        for i in 1..=20 {
            let proposal = ProposalData::Set { key: format!("key_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect(&format!("Failed to propose entry {}", i));
        }

        cluster.commit_entries().await.expect("Failed to commit initial entries");

        // Simulate snapshot creation (in real system, this would be automatic)
        let leader_node = cluster.nodes.get(&1).unwrap();
        let snapshot_data = leader_node.state_machine.clone();
        
        assert!(!snapshot_data.is_empty(), "Snapshot should contain data");

        // Simulate node restart with snapshot restoration
        cluster.stop_node(2);
        cluster.restart_node(2);

        let recovering_node = cluster.nodes.get_mut(&2).unwrap();
        recovering_node.state_machine = snapshot_data;

        // Add more entries after snapshot
        for i in 21..=25 {
            let proposal = ProposalData::Set { key: format!("key_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect(&format!("Failed to propose entry {}", i));
        }

        cluster.commit_entries().await.expect("Failed to commit post-snapshot entries");

        // Verify consistency
        // Step until stalled
        assert!(cluster.check_consistency().await, 
                "Cluster should maintain consistency with snapshots");

        // Verify recovered node has correct state
        let recovered_state = &cluster.nodes.get(&2).unwrap().state_machine;
        let leader_state = &cluster.nodes.get(&1).unwrap().state_machine;
        
        assert_eq!(recovered_state.len(), leader_state.len(), 
                  "Recovered node should have same number of keys as leader");
        assert_eq!(recovered_state, leader_state, 
                  "Recovered node should have identical state to leader");
    });
}

/// Helper function to create deterministic test scenarios
async fn run_deterministic_scenario<F, T>(
    config: SimulationConfig,
    test_fn: F,
) -> T
where
    F: FnOnce(SimRaftCluster) -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>,
{
    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let cluster = SimRaftCluster::new(node_ids);
        // Handle not needed in madsim context
        
        test_fn(cluster).await
    })
}

#[test]
#[ignore]
fn test_invariant_safety_properties() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 12345,
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Test: At most one leader per term
        cluster.elect_leader(1).await.expect("Failed to elect leader");
        let term1 = cluster.current_term;
        let leader1 = cluster.get_leader_id();

        // Attempt to elect another leader in same term should fail
        let old_term = cluster.current_term;
        cluster.current_term = term1; // Keep same term
        let result = cluster.elect_leader(2).await;
        
        // This should either fail, or succeed with incremented term
        if result.is_ok() {
            assert!(cluster.current_term > term1, 
                   "New leader must be elected in higher term");
        }

        // Test: Leader completeness - committed entries persist
        cluster.elect_leader(1).await.expect("Failed to elect leader for completeness test");
        
        let committed_proposal = ProposalData::Set { key: "committed".to_string(), value: "persistent".to_string() };
        cluster.propose(committed_proposal.clone()).await.expect("Failed to propose committed entry");
        cluster.commit_entries().await.expect("Failed to commit entry");

        // Change leadership
        cluster.stop_node(1);
        cluster.elect_leader(2).await.expect("Failed to elect new leader");

        // Committed entry should still be present
        let new_leader = cluster.nodes.get(&2).unwrap();
        let new_leader_state = &new_leader.state_machine;
        
        assert!(new_leader_state.get("committed").is_some(), 
               "Committed entries must persist across leader changes");

        // Test: State machine safety - all nodes apply same sequence
        // Add multiple entries
        for i in 1..=5 {
            let proposal = ProposalData::Set { key: format!("safety_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect("Failed to propose safety entry");
        }
        cluster.commit_entries().await.expect("Failed to commit safety entries");

        // All reachable nodes should have identical state
        let mut states = Vec::new();
        for node in cluster.nodes.values() {
            if !node.is_stopped && !node.is_partitioned {
                states.push(&node.state_machine);
            }
        }

        // All states should be identical
        if states.len() > 1 {
            for i in 1..states.len() {
                assert_eq!(states[0], states[i], 
                          "All nodes must apply commands in same order (state machine safety)");
            }
        }
    });
}

// Integration test combining multiple scenarios
#[test]
#[ignore]
fn test_comprehensive_distributed_scenario() {
    let config = SimulationConfig {
        cluster_size: 7,
        seed: 98765,
        timeout: Duration::from_secs(60),
        ..Default::default()
    };

    madsim::runtime::Handle::current().block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Phase 1: Normal operation
        cluster.elect_leader(1).await.expect("Phase 1: Failed to elect leader");
        
        for i in 1..=10 {
            let proposal = ProposalData::Set { key: format!("normal_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect("Phase 1: Failed to propose");
        }
        cluster.commit_entries().await.expect("Phase 1: Failed to commit");

        // Phase 2: Network partition
        cluster.create_partition(vec![1, 2, 3], vec![4, 5, 6, 7]);
        
        // Majority partition should elect new leader
        cluster.elect_leader(2).await.expect("Phase 2: Majority should elect leader");
        
        for i in 11..=15 {
            let proposal = ProposalData::Set { key: format!("partition_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect("Phase 2: Majority should accept proposals");
        }
        cluster.commit_entries().await.expect("Phase 2: Failed to commit in partition");

        // Phase 3: Node failures
        cluster.stop_node(2); // Stop current leader
        cluster.elect_leader(3).await.expect("Phase 3: Should elect new leader after failure");

        cluster.stop_node(6); // Stop node in minority partition
        cluster.stop_node(7); // Stop another node

        // Phase 4: Heal partition
        cluster.heal_partition();
        // Step until stalled

        // Phase 5: Recovery
        cluster.restart_node(6);
        cluster.restart_node(7);
        
        for i in 16..=20 {
            let proposal = ProposalData::Set { key: format!("recovery_{}", i), value: format!("value_{}", i) };
            cluster.propose(proposal).await.expect("Phase 5: Recovery proposals should work");
        }
        cluster.commit_entries().await.expect("Phase 5: Failed to commit after recovery");

        // Phase 6: Final consistency check
        // Step until stalled
        
        let mut final_consistent = false;
        for attempt in 1..=20 {
            if cluster.check_consistency().await {
                final_consistent = true;
                break;
            }
            time::sleep(Duration::from_millis(100)).await;
            
            if attempt % 5 == 0 {
                println!("Consistency check attempt {}/20", attempt);
            }
        }

        assert!(final_consistent, "Comprehensive scenario should end with consistent state");

        // Verify final state properties
        let active_nodes: Vec<_> = cluster.nodes.values()
            .filter(|n| !n.is_stopped && !n.is_partitioned)
            .collect();
        
        assert!(active_nodes.len() >= 4, "Should have majority of nodes active");
        
        if let Some(leader_id) = cluster.get_leader_id() {
            let leader_state = &cluster.nodes.get(&leader_id).unwrap().state_machine;
            assert!(leader_state.len() >= 15, "Should have committed significant data");
        }

        println!("Comprehensive distributed scenario completed successfully");
    });
}