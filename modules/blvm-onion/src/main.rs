//! blvm-onion - Censorship-resistant messaging via onion routing
//!
//! Uses `run_module!` (same pattern as core `blvm-mesh`).

use anyhow::Result;
use blvm_mesh::MeshClient;
use blvm_node::module::traits::ModuleError;
use blvm_onion::{OnionConfig, OnionEncryption, OnionMessaging, OnionModule, OnionRouteBuilder};
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MODULE_NAME: &str = "blvm-onion";

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
                    "onion-v1".to_string(),
                    "handle_incoming_packet".to_string(),
                )
                .await?;

            let route_builder = OnionRouteBuilder::new(OnionConfig::default());
            let onion_encryption = OnionEncryption::new();
            let onion_messaging = Arc::new(
                OnionMessaging::new(
                    mesh_client,
                    route_builder,
                    onion_encryption,
                    bootstrap.module_id.clone(),
                )
                .await
                .map_err(|e| ModuleError::Other(e.into()))?,
            );
            let module = OnionModule {
                onion_messaging: Arc::clone(&onion_messaging),
            };
            Ok((module.clone(), module))
        }
    };

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module_type: OnionModule,
        cli_type: OnionModule,
        db: db.as_db(),
        setup: setup,
        event_types: OnionModule::event_types(),
    }?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
