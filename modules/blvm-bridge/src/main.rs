//! blvm-bridge - Bridge service for various connectivity types
//!
//! This module provides connectivity bridging for Bitcoin nodes
//! using various bridge types (satellite, radio, etc.), with mesh routing as fallback.

pub mod bridge;

use anyhow::Result;
use blvm_node::module::integration::ModuleIntegration;
use blvm_mesh::MeshClient;
use blvm_node::module::EventType;
use blvm_node::module::ipc::protocol::ModuleMessage;
use bridge::BridgeService;
use clap::Parser;
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

    /// Bridge mode: "satellite", "radio", "internet", or custom
    #[arg(long, default_value = "satellite")]
    bridge_mode: String,
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
        .unwrap_or_else(|| "blvm-bridge".to_string());

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
        "blvm-bridge module starting... (module_id: {}, mode: {}, socket: {:?})",
        module_id, args.bridge_mode, socket_path
    );

    // Step 1: Connect to node using ModuleIntegration
    let mut integration = match ModuleIntegration::connect(
        socket_path.clone(),
        module_id.clone(),
        "blvm-bridge".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )
    .await
    {
        Ok(integration) => integration,
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

    if let Err(e) = integration.subscribe_events(event_types).await {
        error!("Failed to subscribe to events: {}", e);
        return Err(anyhow::anyhow!("Subscription failed: {}", e));
    }

    // Get NodeAPI from integration
    let node_api = integration.node_api();

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
            "bridge-v1".to_string(),
            "handle_bridge_packet".to_string(),
        )
        .await
    {
        error!("Failed to register protocol handler: {}", e);
        return Err(anyhow::anyhow!("Protocol registration failed: {}", e));
    }

    info!("Step 3 complete: Protocol handler registered (bridge-v1)");

    // Step 4: Initialize bridge service
    let bridge_mode = match args.bridge_mode.as_str() {
        "satellite" => bridge::BridgeMode::Satellite,
        "radio" => bridge::BridgeMode::Radio,
        "internet" => bridge::BridgeMode::Internet,
        custom => bridge::BridgeMode::Custom(custom.to_string()),
    };

    let bridge_mode_clone = bridge_mode.clone();
    let bridge_service = Arc::new(BridgeService::new(
        mesh_client,
        module_id.clone(),
        bridge_mode,
    ));

    info!("Step 4 complete: Bridge service initialized (mode: {:?})", bridge_mode_clone);

    info!("blvm-bridge module fully initialized and ready");

    // Event processing loop
    let mut event_receiver = integration.event_receiver();
    loop {
        match event_receiver.recv().await {
            Ok(ModuleMessage::Event(event_msg)) => {
                match event_msg.event_type {
                    EventType::PeerConnected => {
                        info!("Peer connected event received");
                    }
                    EventType::PeerDisconnected => {
                        info!("Peer disconnected event received");
                    }
                    EventType::NewBlock => {
                        info!("New block event - bridge may need to relay");
                        // TODO: Relay block via bridge if direct connection fails
                    }
                    EventType::MempoolTransactionAdded => {
                        // Mempool changes may need bridging
                        // TODO: Relay transactions if needed
                    }
                    _ => {
                        // Ignore other events
                    }
                }
            }
            Ok(_) => {
                // Not an event message
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!("Event receiver lagged by {} messages", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                warn!("Event channel closed, module shutting down");
                break;
            }
        }
    }

    Ok(())
}

