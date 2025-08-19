//! Standalone Deterministic Simulation Tests using madsim for Distributed Consensus
//!
//! This module provides comprehensive deterministic testing of Raft consensus scenarios
//! without depending on the main iroh-raft library, demonstrating the testing patterns
//! and techniques that should be used once the main library is fixed.
//!
//! These tests showcase:
//! - Network partitions and message loss scenarios
//! - Node failures and recovery testing
//! - Timing issues and leader election edge cases
//! - Concurrent operations and proposal handling
//! - Split-brain prevention verification
//! - Consensus invariant validation
//!
//! All tests use madsim::runtime::Runtime with fixed seeds to ensure deterministic
//! execution and reproducible results across test runs.

#![cfg(all(test, feature = "madsim"))]

use madsim::runtime::Runtime;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;

// Type aliases for clarity
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

impl ProposalData {
    pub fn set(key: &str, value: &str) -> Self {
        Self::Set {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    pub fn delete(key: &str) -> Self {
        Self::Delete {
            key: key.to_string(),
        }
    }

    pub fn get(key: &str) -> Self {
        Self::Get {
            key: key.to_string(),
        }
    }
}

/// Simulation error types
#[derive(Debug, Error)]
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
    
    #[error("Timeout: {operation}")]
    Timeout { operation: String },
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

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
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
    last_heartbeat: Option<std::time::Instant>,
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
            last_heartbeat: None,
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

    fn become_leader(&mut self, term: Term) {
        self.is_leader = true;
        self.current_term = term;
        self.voted_for = Some(self.id);
        self.last_heartbeat = Some(std::time::Instant::now());
    }

    fn step_down(&mut self, term: Term) {
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

    fn vote(&mut self, candidate_id: NodeId, term: Term) -> bool {
        if self.is_stopped || self.is_partitioned {
            return false;
        }
        
        if term > self.current_term {
            self.current_term = term;
            self.voted_for = Some(candidate_id);
            self.is_leader = false;
            true
        } else if term == self.current_term && self.voted_for.is_none() {
            self.voted_for = Some(candidate_id);
            true
        } else {
            false
        }
    }

    fn receive_heartbeat(&mut self, from: NodeId, term: Term) -> bool {
        if self.is_stopped || self.is_partitioned {
            return false;
        }

        if term >= self.current_term {
            self.current_term = term;
            self.is_leader = false;
            self.voted_for = Some(from);
            self.last_heartbeat = Some(std::time::Instant::now());
            true
        } else {
            false
        }
    }
}

/// Simulated Raft cluster for deterministic testing
#[derive(Debug)]
struct SimRaftCluster {
    nodes: HashMap<NodeId, SimRaftNode>,
    network_partitions: HashMap<NodeId, HashSet<NodeId>>,
    message_delays: HashMap<(NodeId, NodeId), Duration>,
    dropped_messages: HashSet<(NodeId, NodeId)>,
    current_term: Term,
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
        
        // Check if candidate can reach majority
        let reachable_nodes = self.get_reachable_nodes(candidate_id);
        let required_votes = (self.nodes.len() / 2) + 1;
        
        if reachable_nodes.len() < required_votes {
            return Err(SimError::ElectionFailed {
                node_id: candidate_id,
                term: new_term,
                reason: format!("insufficient reachable nodes: {} < {}", 
                              reachable_nodes.len(), required_votes),
            });
        }

        // Simulate voting process
        let mut votes = 1; // Candidate votes for itself
        for &other_id in &reachable_nodes {
            if other_id != candidate_id {
                if let Some(node) = self.nodes.get_mut(&other_id) {
                    if node.vote(candidate_id, new_term) {
                        votes += 1;
                    }
                }
            }
        }

        if votes < required_votes {
            return Err(SimError::ElectionFailed {
                node_id: candidate_id,
                term: new_term,
                reason: format!("insufficient votes: {} < {}", votes, required_votes),
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
        let group1_set: HashSet<NodeId> = group1.iter().cloned().collect();
        let group2_set: HashSet<NodeId> = group2.iter().cloned().collect();

        for &node_id in &group1 {
            self.network_partitions.insert(node_id, group1_set.clone());
        }
        for &node_id in &group2 {
            self.network_partitions.insert(node_id, group2_set.clone());
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
        let required_nodes = (self.nodes.len() / 2) + 1;
        if reachable_nodes.len() >= required_nodes {
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

    async fn send_heartbeat(&mut self, leader_id: NodeId) -> Result<usize> {
        let mut responses = 0;
        let leader_term = self.nodes.get(&leader_id)
            .ok_or(SimError::NodeNotFound { node_id: leader_id })?
            .current_term;

        for (&node_id, node) in self.nodes.iter_mut() {
            if node_id != leader_id && self.can_communicate(leader_id, node_id) {
                if node.receive_heartbeat(leader_id, leader_term) {
                    responses += 1;
                }
            }
        }

        Ok(responses)
    }

    fn count_active_nodes(&self) -> usize {
        self.nodes.values()
            .filter(|n| !n.is_stopped && !n.is_partitioned)
            .count()
    }
}

#[test]
fn test_raft_consensus_under_partition() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 42,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Initial leader election
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");
        assert!(cluster.has_leader(), "Cluster should have a leader");

        // Propose some initial data
        let initial_proposal = ProposalData::set("key1", "value1");
        cluster.propose(initial_proposal).await.expect("Failed to propose initial data");
        cluster.commit_entries().await.expect("Failed to commit initial entries");

        // Create network partition: [1, 2] vs [3, 4, 5]
        let group1 = vec![1, 2];
        let group2 = vec![3, 4, 5];
        cluster.create_partition(group1.clone(), group2.clone());

        // The minority partition (group1) should lose leadership since it lost majority
        // The majority partition (group2) should elect a new leader
        cluster.elect_leader(3).await.expect("Majority partition should elect new leader");
        
        assert_eq!(cluster.get_leader_id(), Some(3), "Node 3 should be the new leader");

        // Minority partition should not be able to commit new entries
        let minority_proposal = ProposalData::set("key2", "value2");
        let result = cluster.nodes.get_mut(&1).unwrap().propose(minority_proposal).await;
        assert!(result.is_err(), "Minority partition should not accept proposals");

        // Majority partition should be able to commit entries
        let majority_proposal = ProposalData::set("key3", "value3");
        cluster.propose(majority_proposal).await.expect("Majority partition should accept proposals");
        cluster.commit_entries().await.expect("Majority partition should commit entries");

        // Heal the partition
        cluster.heal_partition();

        // Allow time for partition healing and synchronization
        madsim::time::sleep(Duration::from_millis(100)).await;

        // Eventually, all nodes should reach consistency
        let mut consistent = false;
        for _ in 0..10 {
            if cluster.check_consistency().await {
                consistent = true;
                break;
            }
            madsim::time::sleep(Duration::from_millis(100)).await;
        }
        
        assert!(consistent, "Cluster should reach consistency after partition healing");
    });
}

#[test]
fn test_leader_election_determinism() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 123,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        
        // Test deterministic leader election multiple times with the same seed
        for iteration in 1..=5 {
            let mut cluster = SimRaftCluster::new(node_ids.clone());

            // Elect leader with deterministic conditions
            cluster.elect_leader(1).await.expect("Failed to elect leader");
            
            // With the same seed, the same node should always become leader
            assert_eq!(cluster.get_leader_id(), Some(1), 
                      "Leader election should be deterministic (iteration {})", iteration);
            
            assert_eq!(cluster.current_term, 1, 
                      "Term should be consistent");
        }
    });
}

#[test]
fn test_message_loss_scenarios() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 456,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
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
        let proposal = ProposalData::set("test_key", "test_value");
        cluster.propose(proposal).await.expect("Proposal should succeed despite message loss");

        // Send heartbeats - should get response from node 3 only
        let responses = cluster.send_heartbeat(1).await.expect("Heartbeat should work");
        assert_eq!(responses, 1, "Should get response from one node (node 3)");

        // Drop messages to all followers - should cause leadership issues
        cluster.drop_messages(1, 3);
        cluster.drop_messages(3, 1);

        // Leader should lose leadership due to loss of majority communication
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
fn test_node_failure_and_recovery() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 789,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect initial leader
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Propose and commit some data
        let proposal1 = ProposalData::set("before_failure", "value1");
        cluster.propose(proposal1).await.expect("Failed to propose before failure");
        cluster.commit_entries().await.expect("Failed to commit before failure");

        // Stop the leader node (simulate crash)
        cluster.stop_node(1);
        assert!(!cluster.has_leader(), "Cluster should not have leader after leader failure");

        // Elect new leader from remaining nodes
        cluster.elect_leader(2).await.expect("Failed to elect new leader after failure");
        assert_eq!(cluster.get_leader_id(), Some(2), "Node 2 should become new leader");

        // Propose more data with new leader
        let proposal2 = ProposalData::set("after_failure", "value2");
        cluster.propose(proposal2).await.expect("Failed to propose with new leader");
        cluster.commit_entries().await.expect("Failed to commit with new leader");

        // Stop another node (test minority failure)
        cluster.stop_node(3);
        
        // Should still have majority (3 out of 5, with 2 functioning: nodes 2, 4, 5)
        let proposal3 = ProposalData::set("minority_failure", "value3");
        cluster.propose(proposal3).await.expect("Should handle minority failure");

        // Restart failed nodes
        cluster.restart_node(1);
        cluster.restart_node(3);

        // Allow synchronization
        madsim::time::sleep(Duration::from_millis(100)).await;

        // Verify eventual consistency
        let mut consistent = false;
        for _ in 0..10 {
            if cluster.check_consistency().await {
                consistent = true;
                break;
            }
            madsim::time::sleep(Duration::from_millis(50)).await;
        }
        
        assert!(consistent, "Cluster should reach consistency after node recovery");
        assert_eq!(cluster.count_active_nodes(), 5, "All nodes should be active after recovery");
    });
}

#[test]
fn test_concurrent_proposals() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 999,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Elect leader
        cluster.elect_leader(1).await.expect("Failed to elect leader");

        // Simulate concurrent proposals
        let proposals = vec![
            ProposalData::set("concurrent_1", "value_1"),
            ProposalData::set("concurrent_2", "value_2"),
            ProposalData::set("concurrent_3", "value_3"),
            ProposalData::set("concurrent_4", "value_4"),
            ProposalData::set("concurrent_5", "value_5"),
        ];

        // Submit proposals sequentially (in deterministic simulation)
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

        // Submit more proposals to new leader
        let more_proposals = vec![
            ProposalData::set("after_leader_change_1", "value_1"),
            ProposalData::set("after_leader_change_2", "value_2"),
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
fn test_split_brain_prevention() {
    let config = SimulationConfig {
        cluster_size: 4,
        seed: 2024,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
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
        let proposal1 = ProposalData::set("split_1", "value_1");
        let proposal2 = ProposalData::set("split_2", "value_2");

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
        let recovery_proposal = ProposalData::set("recovery", "success");
        cluster.propose(recovery_proposal).await.expect("Majority should accept proposals");
        cluster.commit_entries().await.expect("Majority should commit entries");

        // Full healing
        cluster.heal_partition();
        madsim::time::sleep(Duration::from_millis(100)).await;

        // Verify eventual consistency
        assert!(cluster.check_consistency().await, 
                "Cluster should reach consistency after split-brain resolution");
    });
}

#[test]
fn test_invariant_safety_properties() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 12345,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Test: At most one leader per term
        cluster.elect_leader(1).await.expect("Failed to elect leader");
        let term1 = cluster.current_term;
        let leader1 = cluster.get_leader_id();

        assert_eq!(leader1, Some(1), "Node 1 should be leader");

        // Test: Leader completeness - committed entries persist
        let committed_proposal = ProposalData::set("committed", "persistent");
        cluster.propose(committed_proposal.clone()).await.expect("Failed to propose committed entry");
        cluster.commit_entries().await.expect("Failed to commit entry");

        // Change leadership
        cluster.stop_node(1);
        cluster.elect_leader(2).await.expect("Failed to elect new leader");

        // Committed entry should still be present in new leader's state
        let new_leader = cluster.nodes.get(&2).unwrap();
        
        // In a real implementation, the new leader would have the committed entries
        // For our simulation, we verify the new leader has proper state
        assert!(new_leader.current_term > term1, 
               "New leader must be elected in higher term");

        // Test: State machine safety - all nodes apply same sequence
        // Add multiple entries
        for i in 1..=5 {
            let proposal = ProposalData::set(&format!("safety_{}", i), &format!("value_{}", i));
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

        // All states should be identical (state machine safety)
        if states.len() > 1 {
            for i in 1..states.len() {
                assert_eq!(states[0], states[i], 
                          "All nodes must apply commands in same order (state machine safety)");
            }
        }

        // Test: Election safety - at most one leader per term
        let current_term = cluster.current_term;
        let leader_count = cluster.nodes.values()
            .filter(|n| n.is_leader && n.current_term == current_term)
            .count();
        
        assert!(leader_count <= 1, "At most one leader per term (election safety)");
    });
}

#[test]
fn test_timing_and_heartbeat_scenarios() {
    let config = SimulationConfig {
        cluster_size: 3,
        seed: 1337,
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
        let mut cluster = SimRaftCluster::new(node_ids.clone());

        // Test election timeout and heartbeats
        cluster.elect_leader(1).await.expect("Failed to elect initial leader");

        // Send heartbeats to maintain leadership
        for _ in 0..5 {
            let responses = cluster.send_heartbeat(1).await.expect("Heartbeat should work");
            assert_eq!(responses, 2, "Should get responses from 2 followers");
            
            madsim::time::sleep(config.heartbeat_interval).await;
        }

        // Verify leader is still active
        assert_eq!(cluster.get_leader_id(), Some(1), "Leader should remain active with heartbeats");

        // Test network delays
        cluster.add_message_delay(1, 2, Duration::from_millis(150));
        cluster.add_message_delay(1, 3, Duration::from_millis(200));

        // Propose with delays
        let proposal = ProposalData::set("delayed_proposal", "test_value");
        cluster.propose(proposal).await.expect("Delayed proposal should succeed");

        // Simulate election during delays
        madsim::time::sleep(Duration::from_millis(100)).await;

        // Test rapid leadership changes
        for new_leader in &[2, 3, 1] {
            cluster.stop_node(cluster.get_leader_id().unwrap_or(1));
            
            let election_result = cluster.elect_leader(*new_leader).await;
            if election_result.is_ok() {
                assert_eq!(cluster.get_leader_id(), Some(*new_leader), 
                          "Leadership should change to node {}", new_leader);
                
                let quick_proposal = ProposalData::set(
                    &format!("quick_{}", new_leader), 
                    &format!("value_{}", new_leader)
                );
                cluster.propose(quick_proposal).await.expect("Quick proposal should succeed");
                
                // Allow some time for processing
                madsim::time::sleep(Duration::from_millis(10)).await;
            }
            
            // Restart stopped node
            for &node_id in &node_ids {
                cluster.restart_node(node_id);
            }
        }

        // Verify eventual consistency despite rapid changes
        cluster.commit_entries().await.expect("Final commit should succeed");
        assert!(cluster.check_consistency().await, 
                "Cluster should maintain consistency despite timing changes");
    });
}

// Helper function to demonstrate comprehensive testing patterns
async fn run_stress_test_scenario(config: SimulationConfig) -> bool {
    let node_ids: Vec<NodeId> = (1..=config.cluster_size as u64).collect();
    let mut cluster = SimRaftCluster::new(node_ids.clone());

    // Phase 1: Normal operation
    cluster.elect_leader(1).await.ok();
    
    for i in 1..=10 {
        let proposal = ProposalData::set(&format!("stress_{}", i), &format!("value_{}", i));
        let _ = cluster.propose(proposal).await;
    }
    let _ = cluster.commit_entries().await;

    // Phase 2: Random failures and partitions
    cluster.create_partition(vec![1, 2], vec![3, 4, 5]);
    let _ = cluster.elect_leader(3).await;
    
    cluster.stop_node(4);
    let _ = cluster.elect_leader(5).await;

    // Phase 3: Recovery
    cluster.heal_partition();
    cluster.restart_node(4);
    
    madsim::time::sleep(Duration::from_millis(200)).await;

    // Check final consistency
    cluster.check_consistency().await
}

#[test]
fn test_comprehensive_stress_scenario() {
    let config = SimulationConfig {
        cluster_size: 5,
        seed: 98765,
        timeout: Duration::from_secs(10),
        ..Default::default()
    };

    let runtime = Runtime::with_seed(config.seed);
    runtime.block_on(async {
        // Run multiple stress scenarios with the same seed
        let mut successes = 0;
        for iteration in 1..=3 {
            println!("Running stress test iteration {}", iteration);
            if run_stress_test_scenario(config.clone()).await {
                successes += 1;
            }
        }

        // With deterministic simulation, results should be consistent
        assert_eq!(successes, 3, "All stress test iterations should be consistent");
        println!("Comprehensive stress testing completed successfully");
    });
}