//! blvm-onion - Censorship-resistant messaging via onion routing
//!
//! This module provides onion routing capabilities for censorship-resistant messaging
//! through Bitcoin nodes using the blvm-mesh infrastructure.

mod encryption;
mod messaging;
mod onion;

use anyhow::Result;
use blvm_mesh::{MeshClient, NodeId};
use blvm_mesh::MeshPacket;
use blvm_node::module::integration::ModuleIntegration;
use blvm_node::module::EventType;
use blvm_node::module::ipc::protocol::{EventPayload, ModuleMessage};
use clap::Parser;
use encryption::OnionEncryption;
use messaging::OnionMessaging;
use onion::{OnionConfig, OnionRouteBuilder};
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
        .unwrap_or_else(|| "blvm-onion".to_string());

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
        "blvm-onion module starting... (module_id: {}, socket: {:?})",
        module_id, socket_path
    );

    // Step 1: Connect to node using ModuleIntegration
    let mut integration = match ModuleIntegration::connect(
        socket_path.clone(),
        module_id.clone(),
        "blvm-onion".to_string(),
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

    // Subscribe to events we need
    let event_types = vec![
        EventType::PeerConnected,
        EventType::PeerDisconnected,
        EventType::MessageReceived,
        EventType::MeshPacketReceived,
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

    info!("Step 2 complete: MeshClient created (target: {})", mesh_module_id);

    // Step 3: Register protocol handler
    if let Err(e) = mesh_client
        .register_protocol_handler(
            &module_id,
            "onion-v1".to_string(),
            "handle_incoming_packet".to_string(),
        )
        .await
    {
        error!("Failed to register protocol handler: {}", e);
        return Err(anyhow::anyhow!("Protocol registration failed: {}", e));
    }

    info!("Step 3 complete: Protocol handler registered (onion-v1)");

    // Step 4: Initialize onion route builder
    let onion_config = OnionConfig::default();
    let route_builder = OnionRouteBuilder::new(onion_config);

    info!("Step 4 complete: Onion route builder initialized");

    // Step 5: Initialize onion encryption
    let onion_encryption = OnionEncryption::new();

    info!("Step 5 complete: Onion encryption initialized");

    // Get our node ID from mesh before creating messaging (which takes ownership)
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

    // Step 6: Initialize onion messaging (this gets node ID internally)
    let onion_messaging = Arc::new(
        OnionMessaging::new(
            mesh_client,
            route_builder,
            onion_encryption,
            module_id.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create onion messaging: {}", e))?,
    );

    info!("Step 6 complete: Onion messaging initialized");

    info!("blvm-onion module fully initialized and ready");

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
                    EventType::MessageReceived => {
                        info!("Message received event");
                    }
                    EventType::MeshPacketReceived => {
                        if let EventPayload::MeshPacketReceived {
                            packet_data,
                            peer_addr: _,
                        } = &event_msg.payload
                        {
                            // Try to deserialize as MeshPacket
                            if let Ok(packet) = bincode::deserialize::<MeshPacket>(packet_data) {
                                // Check if it's an onion-v1 packet
                                if let Some(ref metadata) = packet.metadata {
                                    if metadata.protocol.as_ref().map(|s| s == "onion-v1").unwrap_or(false) {
                                        // Handle onion packet
                                        match onion_messaging
                                            .handle_incoming_packet(packet.payload, my_node_id)
                                            .await
                                        {
                                            Ok(Some(decrypted_message)) => {
                                                info!(
                                                    "Onion message delivered: {} bytes",
                                                    decrypted_message.len()
                                                );
                                                // TODO: Deliver to application/user
                                            }
                                            Ok(None) => {
                                                debug!("Onion packet forwarded (intermediate node)");
                                            }
                                            Err(e) => {
                                                error!("Failed to handle onion packet: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // Ignore other events for now
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

