//! blvm-mining-pool - Mining pool coordination via mesh routing
//!
//! Uses `run_module!` (same pattern as core `blvm-mesh`).

use anyhow::Result;
use blvm_mesh::MeshClient;
use blvm_mining_pool::{MiningPoolModule, PoolCoordinator};
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MODULE_NAME: &str = "blvm-mining-pool";

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
                    "mining-pool-v1".to_string(),
                    "handle_pool_message".to_string(),
                )
                .await?;

            let pool_coordinator: Arc<PoolCoordinator> = Arc::new(PoolCoordinator::new(
                mesh_client,
                bootstrap.module_id.clone(),
            ));
            let module = MiningPoolModule {
                pool_coordinator: Arc::clone(&pool_coordinator),
            };
            Ok((module.clone(), module))
        }
    };

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module_type: MiningPoolModule,
        cli_type: MiningPoolModule,
        db: db.as_db(),
        setup: setup,
        event_types: MiningPoolModule::event_types(),
    }?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
