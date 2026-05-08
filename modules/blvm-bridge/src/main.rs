//! blvm-bridge - Bridge service for satellite / radio / internet connectivity
//!
//! Uses `run_module!` (same pattern as core `blvm-mesh`).
//! `BRIDGE_MODE`: `satellite` | `radio` | `internet` | custom string (default: `satellite`).
//! `BRIDGE_EDGE_TRANSPORT`: optional `meshtastic` | `meshtastic-mqtt` | `reticulum` | `generic-radio`
//! (see `blvm_mesh::edge_transport`).

use anyhow::Result;
use blvm_bridge::{BridgeMode, BridgeModule, BridgeService};
use blvm_mesh::{parse_edge_transport, MeshClient};
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MODULE_NAME: &str = "blvm-bridge";

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = ModuleBootstrap::init_module(MODULE_NAME);
    let db = ModuleDb::open(&bootstrap.data_dir)?;

    let setup = |node_api: Arc<dyn blvm_node::module::traits::NodeAPI>,
                 _db: Arc<dyn blvm_node::storage::database::Database>,
                 _data_dir: &Path| {
        let bootstrap = bootstrap.clone();
        async move {
            let mesh_module_id =
                std::env::var("MESH_MODULE_ID").unwrap_or_else(|_| "blvm-mesh".to_string());
            let mesh_client = MeshClient::new(Arc::clone(&node_api), mesh_module_id);
            mesh_client
                .register_protocol_handler(
                    &bootstrap.module_id,
                    "bridge-v1".to_string(),
                    "handle_bridge_packet".to_string(),
                )
                .await?;

            let bridge_mode = match std::env::var("BRIDGE_MODE")
                .unwrap_or_else(|_| "satellite".into())
                .as_str()
            {
                "satellite" => BridgeMode::Satellite,
                "radio" => BridgeMode::Radio,
                "internet" => BridgeMode::Internet,
                custom => BridgeMode::Custom(custom.to_string()),
            };
            let bridge_mode_label = format!("{:?}", bridge_mode);
            let edge = std::env::var("BRIDGE_EDGE_TRANSPORT").ok().and_then(|s| {
                let t = s.trim();
                if t.is_empty() {
                    return None;
                }
                parse_edge_transport(t).or_else(|| {
                    tracing::warn!(
                        "BRIDGE_EDGE_TRANSPORT={:?} unrecognized; expected meshtastic|meshtastic-mqtt|reticulum|generic-radio",
                        t
                    );
                    None
                })
            });
            let edge_label = edge
                .map(|e| e.to_string())
                .unwrap_or_else(|| "none".to_string());
            let bridge_service = Arc::new(BridgeService::new(
                mesh_client,
                bootstrap.module_id.clone(),
                bridge_mode,
                edge,
            ));
            let module = BridgeModule {
                bridge_service: Arc::clone(&bridge_service),
                bridge_mode: bridge_mode_label,
                edge_transport: edge_label,
            };
            Ok((module.clone(), module))
        }
    };

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module_type: BridgeModule,
        cli_type: BridgeModule,
        db: db.as_db(),
        setup: setup,
        event_types: BridgeModule::event_types(),
    }?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
