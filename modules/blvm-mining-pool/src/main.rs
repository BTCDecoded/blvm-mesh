//! blvm-mining-pool - Mining pool coordination via mesh routing
//!
//! This module provides mining pool coordination capabilities using blvm-mesh
//! for reliable block template distribution and pool member communication.

pub mod pool;

use anyhow::Result;
use blvm_mesh::MeshClient;
use blvm_mining_pool::MiningPoolModule;
use blvm_mining_pool::PoolCoordinator;
use blvm_node::module::integration::ModuleIntegration;
use blvm_node::module::ipc::protocol::{
    InvocationResultMessage, InvocationResultPayload, InvocationType, ModuleMessage,
};
use blvm_node::module::EventType;
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::sync::Arc;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bootstrap = ModuleBootstrap::from_env_or_defaults(
        "blvm-mining-pool",
        "data/modules/blvm-mining-pool.sock",
        "data/modules/blvm-mining-pool",
    );

    info!(
        "blvm-mining-pool module starting... (module_id: {}, socket: {:?})",
        bootstrap.module_id, bootstrap.socket_path
    );

    let mut integration = ModuleIntegration::connect(
        bootstrap.socket_path.clone(),
        bootstrap.module_id.clone(),
        "blvm-mining-pool".into(),
        env!("CARGO_PKG_VERSION").into(),
        Some(MiningPoolModule::cli_spec()),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Connection failed: {}", e))?;

    // Subscribe to events
    let event_types = vec![
        EventType::PeerConnected,
        EventType::PeerDisconnected,
        EventType::NewBlock,
        EventType::MempoolTransactionAdded,
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

    info!("Step 2 complete: MeshClient created");

    // Step 3: Register protocol handler
    if let Err(e) = mesh_client
        .register_protocol_handler(
            &bootstrap.module_id,
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
    let pool_coordinator: Arc<PoolCoordinator> = Arc::new(PoolCoordinator::new(
        mesh_client,
        bootstrap.module_id.clone(),
    ));

    info!("Step 4 complete: Pool coordinator initialized");

    info!("blvm-mining-pool module fully initialized and ready");

    let db = ModuleDb::open(&bootstrap.data_dir)?;
    let invocation_ctx = blvm_sdk::module::runner::InvocationContext::new(db.as_db());
    let module = MiningPoolModule {
        pool_coordinator: Arc::clone(&pool_coordinator),
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

