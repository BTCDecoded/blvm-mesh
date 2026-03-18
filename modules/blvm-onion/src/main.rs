//! blvm-onion - Censorship-resistant messaging via onion routing
//!
//! This module provides onion routing capabilities for censorship-resistant messaging
//! through Bitcoin nodes using the blvm-mesh infrastructure.

use anyhow::Result;
use blvm_mesh::{MeshClient, MeshPacket};
use blvm_node::module::integration::ModuleIntegration;
use blvm_node::module::ipc::protocol::{
    EventPayload, InvocationResultMessage, InvocationResultPayload, InvocationType, ModuleMessage,
};
use blvm_node::module::EventType;
use blvm_onion::OnionModule;
use blvm_onion::{OnionConfig, OnionEncryption, OnionMessaging, OnionRouteBuilder};
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bootstrap = ModuleBootstrap::from_env_or_defaults(
        "blvm-onion",
        "data/modules/blvm-onion.sock",
        "data/modules/blvm-onion",
    );

    info!(
        "blvm-onion module starting... (module_id: {}, socket: {:?})",
        bootstrap.module_id, bootstrap.socket_path
    );

    let mut integration = ModuleIntegration::connect(
        bootstrap.socket_path.clone(),
        bootstrap.module_id.clone(),
        "blvm-onion".into(),
        env!("CARGO_PKG_VERSION").into(),
        Some(OnionModule::cli_spec()),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Connection failed: {}", e))?;

    // Subscribe to events we need
    let event_types = vec![
        EventType::PeerConnected,
        EventType::PeerDisconnected,
        EventType::MessageReceived,
        EventType::MeshPacketReceived,
    ];

    integration.subscribe_events(event_types).await.map_err(|e| anyhow::anyhow!("Subscription failed: {}", e))?;

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
            &bootstrap.module_id,
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
            bootstrap.module_id.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create onion messaging: {}", e))?,
    );

    info!("Step 6 complete: Onion messaging initialized");

    info!("blvm-onion module fully initialized and ready");

    let db = ModuleDb::open(&bootstrap.data_dir)?;
    let invocation_ctx = blvm_sdk::module::runner::InvocationContext::new(db.as_db());
    let module = OnionModule {
        onion_messaging: Arc::clone(&onion_messaging),
    };

    let mut event_receiver = integration.event_receiver();
    let mut invocation_rx = integration.invocation_receiver().expect("CLI spec provided");

    loop {
        tokio::select! {
            inv = invocation_rx.recv() => {
                if let Some((invocation, result_tx)) = inv {
                    let result = match &invocation.invocation_type {
                        InvocationType::Cli { subcommand, args } => {
                            match module.dispatch_cli(&invocation_ctx, subcommand, args) {
                                Ok(stdout) => InvocationResultMessage {
                                    correlation_id: invocation.correlation_id,
                                    success: true,
                                    payload: Some(InvocationResultPayload::Cli {
                                        stdout,
                                        stderr: String::new(),
                                        exit_code: 0,
                                    }),
                                    error: None,
                                },
                                Err(e) => InvocationResultMessage {
                                    correlation_id: invocation.correlation_id,
                                    success: false,
                                    payload: None,
                                    error: Some(e.to_string()),
                                },
                            }
                        }
                        _ => InvocationResultMessage {
                            correlation_id: invocation.correlation_id,
                            success: false,
                            payload: None,
                            error: Some("RPC not implemented".to_string()),
                        },
                    };
                    let _ = result_tx.send(result);
                } else {
                    info!("Invocation channel closed, module unloading");
                    break;
                }
            }
            ev = event_receiver.recv() => {
                match ev {
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
                            // Enforce payload size limit before deserialization (S-012)
                            if packet_data.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
                                tracing::debug!("Dropping oversized mesh packet: {} bytes", packet_data.len());
                            } else if let Ok(packet) = bincode::deserialize::<MeshPacket>(packet_data) {
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
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Event receiver lagged by {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    warn!("Event channel closed, module shutting down");
                    break;
                }
                }
            }
        }
    }

    Ok(())
}

