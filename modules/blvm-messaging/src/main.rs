//! blvm-messaging - P2P messaging via payment-gated mesh
//!
//! Uses `run_module!` (same pattern as core `blvm-mesh`): connect, subscribe, CLI, events.

use anyhow::Result;
use blvm_mesh::MeshClient;
use blvm_messaging::{MessagingModule, MessagingService};
use blvm_node::module::traits::ModuleError;
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MODULE_NAME: &str = "blvm-messaging";

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
                    "messaging-v1".to_string(),
                    "handle_message".to_string(),
                )
                .await?;

            let messaging_service = Arc::new(
                MessagingService::new(mesh_client, bootstrap.module_id.clone())
                    .await
                    .map_err(|e| ModuleError::Other(e.into()))?,
            );
            let module = MessagingModule {
                messaging_service: Arc::clone(&messaging_service),
            };
            Ok((module.clone(), module))
        }
    };

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module_type: MessagingModule,
        cli_type: MessagingModule,
        db: db.as_db(),
        setup: setup,
        event_types: MessagingModule::event_types(),
    }?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
