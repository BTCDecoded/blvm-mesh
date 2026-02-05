//! blvm-messaging - P2P messaging via payment-gated mesh
//!
//! This module provides P2P messaging capabilities using blvm-mesh
//! for decentralized, payment-gated messaging.

mod message;

use anyhow::Result;
use blvm_node::module::integration::ModuleIntegration;
use blvm_mesh::{MeshClient, MeshPacket, NodeId};
use blvm_node::module::EventType;
use blvm_node::module::ipc::protocol::{EventPayload, ModuleMessage};
use clap::Parser;
use message::MessagingService;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

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
        .unwrap_or_else(|| "blvm-messaging".to_string());

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
        "blvm-messaging module starting... (module_id: {}, socket: {:?})",
        module_id, socket_path
    );

    // Step 1: Connect to node
    let mut integration = match ModuleIntegration::connect(
        socket_path.clone(),
        module_id.clone(),
        "blvm-messaging".to_string(),
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
        EventType::MeshPacketReceived,
    ];

    if let Err(e) = integration.subscribe_events(event_types).await {
        error!("Failed to subscribe to events: {}", e);
        return Err(anyhow::anyhow!("Subscription failed: {}", e));
    }

    // Create NodeAPI IPC wrapper
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
            "messaging-v1".to_string(),
            "handle_message".to_string(),
        )
        .await
    {
        error!("Failed to register protocol handler: {}", e);
        return Err(anyhow::anyhow!("Protocol registration failed: {}", e));
    }

    info!("Step 3 complete: Protocol handler registered (messaging-v1)");

    // Get our node ID from mesh before creating service (which takes ownership)
    let my_node_id = match mesh_client.get_node_id().await {
        Ok(id) => {
            info!("Retrieved node ID from mesh: {:x?}", &id[..8]);
            id
        }
        Err(e) => {
            warn!("Failed to get node ID from mesh: {}, using placeholder", e);
            [0u8; 32] // Fallback placeholder
        }
    };

    // Step 4: Initialize messaging service
    let messaging_service = Arc::new(
        MessagingService::new(
            mesh_client,
            module_id.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create messaging service: {}", e))?,
    );

    info!("Step 4 complete: Messaging service initialized");

    info!("blvm-messaging module fully initialized and ready");

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
                    EventType::MeshPacketReceived => {
                        if let EventPayload::MeshPacketReceived {
                            packet_data,
                            peer_addr: _,
                        } = &event_msg.payload
                        {
                            // Try to deserialize as MeshPacket
                            if let Ok(packet) = bincode::deserialize::<MeshPacket>(packet_data) {
                                // Check if it's a messaging-v1 packet
                                if let Some(ref metadata) = packet.metadata {
                                    if metadata.protocol.as_ref().map(|s| s == "messaging-v1").unwrap_or(false) {
                                        // Handle message
                                        match messaging_service
                                            .handle_incoming_message(packet.payload, my_node_id)
                                            .await
                                        {
                                            Ok(Some(message)) => {
                                                info!("Message received: {} bytes", message.len());
                                                // TODO: Deliver to user/application
                                            }
                                            Ok(None) => {
                                                debug!("Message processed (intermediate)");
                                            }
                                            Err(e) => {
                                                error!("Failed to handle message: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
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

