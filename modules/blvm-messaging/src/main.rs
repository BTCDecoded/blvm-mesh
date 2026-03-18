//! blvm-messaging - P2P messaging via payment-gated mesh
//!
//! This module provides P2P messaging capabilities using blvm-mesh
//! for decentralized, payment-gated messaging.

use anyhow::Result;
use blvm_mesh::{MeshClient, MeshPacket, NodeId};
use blvm_messaging::MessagingModule;
use blvm_messaging::MessagingService;
use blvm_node::module::integration::ModuleIntegration;
use blvm_node::module::ipc::protocol::{
    EventPayload, InvocationResultMessage, InvocationResultPayload, InvocationType, ModuleMessage,
};
use blvm_node::module::EventType;
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bootstrap = ModuleBootstrap::from_env_or_defaults(
        "blvm-messaging",
        "data/modules/blvm-messaging.sock",
        "data/modules/blvm-messaging",
    );

    info!(
        "blvm-messaging module starting... (module_id: {}, socket: {:?})",
        bootstrap.module_id, bootstrap.socket_path
    );

    let mut integration = ModuleIntegration::connect(
        bootstrap.socket_path.clone(),
        bootstrap.module_id.clone(),
        "blvm-messaging".into(),
        env!("CARGO_PKG_VERSION").into(),
        Some(MessagingModule::cli_spec()),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Connection failed: {}", e))?;

    integration
        .subscribe_events(vec![
            EventType::PeerConnected,
            EventType::PeerDisconnected,
            EventType::MeshPacketReceived,
        ])
        .await
        .map_err(|e| anyhow::anyhow!("Subscription failed: {}", e))?;

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
            &bootstrap.module_id,
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
            bootstrap.module_id.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create messaging service: {}", e))?,
    );

    info!("Step 4 complete: Messaging service initialized");

    info!("blvm-messaging module fully initialized and ready");

    let db = ModuleDb::open(&bootstrap.data_dir)?;
    let invocation_ctx = blvm_sdk::module::runner::InvocationContext::new(db.as_db());
    let module = MessagingModule {
        messaging_service: Arc::clone(&messaging_service),
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
                    EventType::MeshPacketReceived => {
                        if let EventPayload::MeshPacketReceived {
                            packet_data,
                            peer_addr: _,
                        } = &event_msg.payload
                        {
                            // Enforce payload size limit before deserialization (S-012)
                            if packet_data.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
                                debug!("Dropping oversized mesh packet: {} bytes", packet_data.len());
                            } else if let Ok(packet) = bincode::deserialize::<MeshPacket>(packet_data) {
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

