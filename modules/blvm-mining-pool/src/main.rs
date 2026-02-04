//! blvm-mining-pool - Mining pool coordination via mesh routing
//!
//! This module provides mining pool coordination capabilities using blvm-mesh
//! for reliable block template distribution and pool member communication.

pub mod pool;

use anyhow::Result;
use blvm_mesh::client::ModuleClient;
use blvm_mesh::nodeapi_ipc;
use blvm_mesh::MeshClient;
use blvm_node::module::EventType;
use blvm_node::module::ipc::protocol::ModuleMessage;
use clap::Parser;
use pool::PoolCoordinator;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

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

    // Get module ID
    let module_id = args
        .module_id
        .or_else(|| std::env::var("MODULE_NAME").ok())
        .unwrap_or_else(|| "blvm-mining-pool".to_string());

    // Get socket path
    let socket_path = args
        .socket_path
        .or_else(|| std::env::var("BLVM_MODULE_SOCKET").ok().map(PathBuf::from))
        .or_else(|| {
            std::env::var("MODULE_SOCKET_DIR")
                .ok()
                .map(|d| PathBuf::from(d).join("modules.sock"))
        })
        .unwrap_or_else(|| PathBuf::from("data/modules/modules.sock"));

    info!(
        "blvm-mining-pool module starting... (module_id: {}, socket: {:?})",
        module_id, socket_path
    );

    // Step 1: Connect to node
    let mut client = match ModuleClient::connect(
        socket_path.clone(),
        module_id.clone(),
        "blvm-mining-pool".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )
    .await
    {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to connect to node: {}", e);
            return Err(anyhow::anyhow!("Connection failed: {}", e));
        }
    };

    // Subscribe to events
    let event_types = vec![
        EventType::PeerConnected,
        EventType::PeerDisconnected,
        EventType::NewBlock,
        EventType::MempoolTransactionAdded,
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

    info!("Step 1 complete: Connected to node");

    // Step 2: Create MeshClient
    let mesh_module_id = std::env::var("MESH_MODULE_ID")
        .unwrap_or_else(|_| "blvm-mesh".to_string());
    
    let mesh_client = MeshClient::new(
        Arc::clone(&node_api),
        mesh_module_id.clone(),
    );

    info!("Step 2 complete: MeshClient created");

    // Step 3: Register protocol handler
    if let Err(e) = mesh_client
        .register_protocol_handler(
            &module_id,
            "mining-pool-v1".to_string(),
            "handle_pool_message".to_string(),
        )
        .await
    {
        error!("Failed to register protocol handler: {}", e);
        return Err(anyhow::anyhow!("Protocol registration failed: {}", e));
    }

    info!("Step 3 complete: Protocol handler registered (mining-pool-v1)");

    // Step 4: Initialize pool coordinator
    let pool_coordinator = Arc::new(PoolCoordinator::new(
        mesh_client,
        module_id.clone(),
    ));

    info!("Step 4 complete: Pool coordinator initialized");

    info!("blvm-mining-pool module fully initialized and ready");

    // Event processing loop
    let mut event_receiver = client.event_receiver();
    loop {
        match event_receiver.recv().await {
            Some(ModuleMessage::Event(event_msg)) => {
                match event_msg.event_type {
                    EventType::PeerConnected => {
                        info!("Peer connected event received");
                    }
                    EventType::PeerDisconnected => {
                        info!("Peer disconnected event received");
                    }
                    EventType::NewBlock => {
                        info!("New block event - pool may need to update templates");
                        // When new block arrives, pool operator should create new template
                        // This would typically trigger template update via RPC or internal logic
                    }
                    EventType::MempoolTransactionAdded => {
                        // Mempool changes may affect block templates
                        // TODO: Update block templates if needed
                    }
                    _ => {
                        // Ignore other events
                    }
                }
            }
            Some(_) => {
                // Not an event message
            }
            None => {
                warn!("Event channel closed, module shutting down");
                break;
            }
        }
    }

    Ok(())
}

