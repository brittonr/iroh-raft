//! Simple example of using the iroh-raft crate
//!
//! This demonstrates basic usage of the simplified iroh-raft crate,
//! including configuration, storage, and the generic state machine components.

use iroh_raft::{
    config::{ConfigBuilder, ConfigError},
    error::RaftError,
    raft::{ExampleRaftStateMachine, GenericProposal, KvCommand, StateMachine},
    Result,
};
use snafu;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting iroh-raft simple example");

    // Create configuration
    let config = ConfigBuilder::new()
        .node_id(1)
        .data_dir("/tmp/iroh-raft-example")
        .log_level("info")
        .build()
        .map_err(|e: ConfigError| RaftError::InvalidConfiguration {
            component: "example".to_string(),
            message: format!("Config error: {}", e),
            backtrace: snafu::Backtrace::new(),
        })?;

    println!(
        "📋 Configuration created for node {}",
        config.node.id.unwrap_or(0)
    );

    // Create a generic state machine with the key-value store example
    let mut state_machine = ExampleRaftStateMachine::new_kv_store();
    println!("🔧 State machine initialized");

    // Demonstrate creating commands for the key-value store
    let set_command = KvCommand::Set {
        key: "example_key".to_string(),
        value: "example_value".to_string(),
    };
    let proposal = GenericProposal::new(set_command.clone())
        .with_originator("simple_example".to_string())
        .with_priority(1);

    println!("📝 Created proposal: {:?}", proposal.command);
    println!("   Metadata: {:?}", proposal.metadata);

    // Simulate applying the command to the state machine
    // (In a real system, this would happen through Raft consensus)
    state_machine
        .inner_mut()
        .apply_command(set_command)
        .await?;

    println!("✅ Command applied to state machine");

    // Check the current state
    let current_state = state_machine.inner().get_current_state();
    println!("📊 Current state:");
    println!("   Keys: {}", current_state.len());
    println!("   Operations: {}", current_state.metadata.operation_count);
    println!("   Version: {}", current_state.metadata.version);

    if let Some(value) = current_state.get("example_key") {
        println!("   example_key = {}", value);
    }

    // Demonstrate creating a snapshot
    let snapshot_data = state_machine.create_snapshot().await?;
    println!("📸 Created snapshot ({} bytes)", snapshot_data.len());

    // Create a new state machine and restore from snapshot
    let mut restored_state_machine = ExampleRaftStateMachine::new_kv_store();
    restored_state_machine
        .restore_from_snapshot(&snapshot_data)
        .await?;

    println!("🔄 Restored state machine from snapshot");

    let restored_state = restored_state_machine.inner().get_current_state();
    println!("📊 Restored state:");
    println!("   Keys: {}", restored_state.len());
    println!("   Operations: {}", restored_state.metadata.operation_count);

    if let Some(value) = restored_state.get("example_key") {
        println!("   example_key = {}", value);
    }

    // Demonstrate more commands
    let delete_command = KvCommand::Delete {
        key: "example_key".to_string(),
    };
    restored_state_machine
        .inner_mut()
        .apply_command(delete_command)
        .await?;

    let final_state = restored_state_machine.inner().get_current_state();
    println!("🗑️  After deletion:");
    println!("   Keys: {}", final_state.len());
    println!("   Operations: {}", final_state.metadata.operation_count);

    println!("🎉 Example completed successfully!");

    Ok(())
}