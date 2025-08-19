//! Generic proposal types for iroh-raft
//!
//! This module defines generic proposal types that can be used by any application
//! implementing distributed consensus with iroh-raft. The proposals are
//! application-agnostic and focus on the generic operations that most
//! distributed systems need.

use serde::{Deserialize, Serialize};

/// Metadata associated with a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalMetadata {
    /// Unique identifier for this proposal
    pub proposal_id: String,
    /// When the proposal was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Optional identifier of the client/node that created the proposal
    pub originator: Option<String>,
    /// Optional correlation ID for tracking related operations
    pub correlation_id: Option<String>,
    /// Priority level of the proposal (higher numbers = higher priority)
    pub priority: u8,
}

impl Default for ProposalMetadata {
    fn default() -> Self {
        Self {
            proposal_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            originator: None,
            correlation_id: None,
            priority: 0,
        }
    }
}

/// Generic proposal that wraps application-specific commands
/// 
/// This allows applications to define their own command types while
/// still benefiting from common distributed system patterns like
/// batching and metadata tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericProposal<Command> {
    /// The application-specific command to execute
    pub command: Command,
    /// Optional metadata about the proposal
    pub metadata: ProposalMetadata,
}

impl<Command> GenericProposal<Command>
where
    Command: Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new proposal with a command and default metadata
    pub fn new(command: Command) -> Self {
        Self {
            command,
            metadata: ProposalMetadata::default(),
        }
    }

    /// Create a new proposal with a command and custom metadata
    pub fn with_metadata(command: Command, metadata: ProposalMetadata) -> Self {
        Self { command, metadata }
    }

    /// Set the originator of the proposal
    pub fn with_originator(mut self, originator: String) -> Self {
        self.metadata.originator = Some(originator);
        self
    }

    /// Set the correlation ID for tracking related operations
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.metadata.correlation_id = Some(correlation_id);
        self
    }

    /// Set the priority of the proposal
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.metadata.priority = priority;
        self
    }
}

/// Batch proposal for efficiently processing multiple operations together
/// 
/// Batching can improve throughput by reducing the number of Raft consensus
/// rounds needed for multiple related operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProposal<Command> {
    /// The proposals in this batch
    pub proposals: Vec<GenericProposal<Command>>,
    /// Metadata for the batch itself
    pub batch_metadata: ProposalMetadata,
}

impl<Command> BatchProposal<Command>
where
    Command: Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new batch proposal
    pub fn new(proposals: Vec<GenericProposal<Command>>) -> Self {
        Self {
            proposals,
            batch_metadata: ProposalMetadata::default(),
        }
    }

    /// Create an empty batch proposal
    pub fn empty() -> Self {
        Self {
            proposals: Vec::new(),
            batch_metadata: ProposalMetadata::default(),
        }
    }

    /// Add a proposal to the batch
    pub fn add_proposal(mut self, proposal: GenericProposal<Command>) -> Self {
        self.proposals.push(proposal);
        self
    }

    /// Add multiple proposals to the batch
    pub fn add_proposals(mut self, mut proposals: Vec<GenericProposal<Command>>) -> Self {
        self.proposals.append(&mut proposals);
        self
    }

    /// Get the number of proposals in the batch
    pub fn len(&self) -> usize {
        self.proposals.len()
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.proposals.is_empty()
    }
}

/// Builder for creating batch proposals
pub struct BatchProposalBuilder<Command>
where
    Command: Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    proposals: Vec<GenericProposal<Command>>,
    batch_metadata: ProposalMetadata,
}

impl<Command> BatchProposalBuilder<Command>
where
    Command: Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new batch proposal builder
    pub fn new() -> Self {
        Self {
            proposals: Vec::new(),
            batch_metadata: ProposalMetadata::default(),
        }
    }

    /// Add a command to the batch
    pub fn add_command(mut self, command: Command) -> Self {
        self.proposals.push(GenericProposal::new(command));
        self
    }

    /// Add a proposal to the batch
    pub fn add_proposal(mut self, proposal: GenericProposal<Command>) -> Self {
        self.proposals.push(proposal);
        self
    }

    /// Set the originator for the batch
    pub fn with_originator(mut self, originator: String) -> Self {
        self.batch_metadata.originator = Some(originator);
        self
    }

    /// Set the correlation ID for the batch
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.batch_metadata.correlation_id = Some(correlation_id);
        self
    }

    /// Set the priority for the batch
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.batch_metadata.priority = priority;
        self
    }

    /// Build the batch proposal
    pub fn build(self) -> BatchProposal<Command> {
        BatchProposal {
            proposals: self.proposals,
            batch_metadata: self.batch_metadata,
        }
    }
}

impl<Command> Default for BatchProposalBuilder<Command>
where
    Command: Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    enum TestCommand {
        Set { key: String, value: String },
        Delete { key: String },
    }

    #[test]
    fn test_generic_proposal_creation() {
        let command = TestCommand::Set {
            key: "test".to_string(),
            value: "value".to_string(),
        };

        let proposal = GenericProposal::new(command.clone());
        assert_eq!(proposal.metadata.priority, 0);
        assert!(proposal.metadata.originator.is_none());

        let proposal_with_originator = GenericProposal::new(command)
            .with_originator("node-1".to_string())
            .with_priority(5);
        
        assert_eq!(proposal_with_originator.metadata.originator, Some("node-1".to_string()));
        assert_eq!(proposal_with_originator.metadata.priority, 5);
    }

    #[test]
    fn test_batch_proposal_builder() {
        let batch = BatchProposalBuilder::new()
            .add_command(TestCommand::Set {
                key: "key1".to_string(),
                value: "value1".to_string(),
            })
            .add_command(TestCommand::Set {
                key: "key2".to_string(),
                value: "value2".to_string(),
            })
            .with_originator("batch-creator".to_string())
            .build();

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert_eq!(batch.batch_metadata.originator, Some("batch-creator".to_string()));
    }

    #[test]
    fn test_empty_batch() {
        let batch = BatchProposal::<TestCommand>::empty();
        assert_eq!(batch.len(), 0);
        assert!(batch.is_empty());
    }
}