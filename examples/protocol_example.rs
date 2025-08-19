//! Example demonstrating the Iroh Raft protocol implementation

use anyhow::Result;
use iroh::endpoint::{self, Connection, Endpoint};
use iroh_raft::transport::{RaftProtocolHandler, RAFT_ALPN};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create an endpoint with a secret key
    let secret_key = iroh::SecretKey::generate();
    let endpoint = Endpoint::builder()
        .secret_key(secret_key.clone())
        .alpns(vec![RAFT_ALPN.to_vec()])
        .bind()
        .await?;

    println!("Node ID: {}", endpoint.node_id());
    println!("Listening on: {:?}", endpoint.bound_sockets());

    // Create a channel for receiving Raft messages
    let (raft_tx, mut raft_rx) = mpsc::unbounded_channel();

    // Create the Raft protocol handler
    let node_id = 1; // This would be your Raft node ID
    let handler = RaftProtocolHandler::new(node_id, raft_tx);

    // Accept incoming connections
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let connecting = match incoming.accept() {
                Ok(connecting) => connecting,
                Err(e) => {
                    tracing::warn!("Failed to accept connection: {}", e);
                    continue;
                }
            };

            let handler = handler.clone();
            tokio::spawn(async move {
                match connecting.await {
                    Ok(connection) => {
                        tracing::info!("New connection from: {}", connection.remote_node_id());
                        if let Err(e) = handler.accept(connection).await {
                            tracing::warn!("Protocol handler error: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to establish connection: {}", e);
                    }
                }
            });
        }
    });

    // Process received Raft messages
    while let Some((from, message)) = raft_rx.recv().await {
        println!("Received Raft message from node {}: {:?}", from, message.msg_type());
    }

    Ok(())
}