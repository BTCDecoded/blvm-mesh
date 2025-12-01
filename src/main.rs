//! bllvm-mesh - Commons Mesh networking module
//!
//! This module provides Commons Mesh networking capabilities for bllvm-node,
//! including payment-gated routing, traffic classification, and fee distribution.

use anyhow::Result;
use bllvm_node::module::ipc::protocol::{EventMessage, EventPayload, EventType, LogLevel, ModuleMessage};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

mod manager;
mod routing_policy;
mod routing;
mod verifier;
mod payment_proof;
mod replay;
mod packet;
mod discovery;
mod network;
mod error;
mod client;
mod nodeapi_ipc;

use error::MeshError;
use manager::MeshManager;
use client::ModuleClient;

/// Command-line arguments for the module
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Module ID (provided by node)
    #[arg(long)]
    module_id: Option<String>,

    /// IPC socket path (provided by node)
    #[arg(long)]
    socket_path: Option<PathBuf>,

    /// Data directory (provided by node)
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Get module ID (from args or environment)
    let module_id = args.module_id
        .or_else(|| std::env::var("MODULE_NAME").ok())
        .unwrap_or_else(|| "bllvm-mesh".to_string());

    // Get socket path (from args, env, or default)
    let socket_path = args.socket_path
        .or_else(|| std::env::var("BLLVM_MODULE_SOCKET").ok().map(PathBuf::from))
        .or_else(|| std::env::var("MODULE_SOCKET_DIR").ok().map(|d| PathBuf::from(d).join("modules.sock")))
        .unwrap_or_else(|| PathBuf::from("data/modules/modules.sock"));

    info!("bllvm-mesh module starting... (module_id: {}, socket: {:?})", module_id, socket_path);

    // Connect to node
    let mut client = match ModuleClient::connect(
        socket_path.clone(),
        module_id.clone(),
        "bllvm-mesh".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    ).await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to connect to node: {}", e);
            return Err(anyhow::anyhow!("Connection failed: {}", e));
        }
    };

    // Subscribe to network and payment events (46+ events as per plan)
    let event_types = vec![
        // Network events
        EventType::PeerConnected,
        EventType::PeerDisconnected,
        EventType::MessageReceived,
        EventType::MessageSent,
        // Payment events
        EventType::PaymentRequestCreated,
        EventType::PaymentVerified,
        EventType::PaymentSettled,
        // Chain events
        EventType::NewBlock,
        EventType::ChainReorg,
        // Mempool events
        EventType::MempoolTransactionAdded,
        EventType::FeeRateChanged,
        // Add more events as needed for mesh routing
    ];

    if let Err(e) = client.subscribe_events(event_types).await {
        error!("Failed to subscribe to events: {}", e);
        return Err(anyhow::anyhow!("Subscription failed: {}", e));
    }

    // Create NodeAPI IPC wrapper
    let node_api = Arc::new(nodeapi_ipc::NodeApiIpc::new(
        Arc::clone(&client.ipc_client()),
        module_id.clone(),
    ));

    // Create mesh manager
    let ctx = bllvm_node::module::traits::ModuleContext {
        module_id: module_id.clone(),
        config: std::collections::HashMap::new(),
        data_dir: args.data_dir.unwrap_or_else(|| PathBuf::from("data/modules/bllvm-mesh")),
        socket_path: socket_path.to_string_lossy().to_string(),
    };

    let manager = MeshManager::new(&ctx, Arc::clone(&node_api))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create mesh manager: {}", e))?;

    // Start mesh manager
    if let Err(e) = manager.start().await {
        error!("Failed to start mesh manager: {}", e);
        return Err(anyhow::anyhow!("Mesh manager startup failed: {}", e));
    }

    info!("Mesh module initialized and running");

    // Event processing loop with parallel batch processing
    let mut event_receiver = client.event_receiver();
    loop {
        // Collect batch of events (up to 10) for parallel processing
        let mut event_batch = Vec::with_capacity(10);
        for _ in 0..10 {
            match event_receiver.try_recv() {
                Ok(event) => event_batch.push(event),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    warn!("Event channel disconnected");
                    return Ok(());
                }
            }
        }
        
        // If no events in batch, wait for next event
        if event_batch.is_empty() {
            if let Some(event) = event_receiver.recv().await {
                event_batch.push(event);
            } else {
                break; // Channel closed
            }
        }
        
        // Process events in parallel
        let futures: Vec<_> = event_batch
            .iter()
            .map(|event| async {
                match event {
                    ModuleMessage::Event(event_msg) => {
                        match event_msg.event_type {
                            EventType::PeerConnected => {
                                info!("Peer connected event received");
                            }
                            EventType::PeerDisconnected => {
                                info!("Peer disconnected event received");
                            }
                            EventType::MessageReceived => {
                                info!("Message received event");
                            }
                            EventType::PaymentRequestCreated => {
                                info!("Payment request created event received");
                            }
                            EventType::NewBlock => {
                                info!("New block event received");
                            }
                            _ => {
                                // Ignore other events for now
                            }
                        }
                    }
                    _ => {
                        // Not an event message
                    }
                }
            })
            .collect();
        
        // Wait for all events in batch to be processed
        futures::future::join_all(futures).await;
    }

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
