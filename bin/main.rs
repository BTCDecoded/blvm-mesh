//! blvm-mesh - Commons Mesh networking module
//!
//! When spawned by the node: reads MODULE_ID, SOCKET_PATH, DATA_DIR from env.
//! For manual testing: blvm-mesh --module-id <id> --socket-path <path> --data-dir <dir>

use anyhow::Result;
use blvm_mesh::api::MeshModuleAPI;
use blvm_mesh::config::MeshConfig;
use blvm_mesh::manager::MeshManager;
use blvm_mesh::storage::up_v1;
use blvm_mesh::MeshModule;
use blvm_sdk::migrations;
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use std::sync::Arc;
use tracing::{error, warn};

const MODULE_NAME: &str = "blvm-mesh";

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = ModuleBootstrap::init_module(MODULE_NAME);
    let db = ModuleDb::open_with_migrations(&bootstrap.data_dir, migrations!(1 => up_v1))?;

    let setup = |node_api: Arc<dyn blvm_node::module::traits::NodeAPI>,
                 _db: Arc<dyn blvm_node::storage::database::Database>,
                 data_dir: &std::path::Path| {
        let bootstrap = bootstrap.clone();
        let data_dir = data_dir.to_path_buf();
        async move {
            let (ctx, _config) = bootstrap.context_with_config::<MeshConfig>(&data_dir);
            let manager = MeshManager::new(&ctx, Arc::clone(&node_api))
                .await
                .map_err(|e| blvm_node::module::traits::ModuleError::Other(format!("Failed to create mesh manager: {}", e)))?;
            if let Err(e) = manager.start().await {
                error!("Failed to start mesh manager: {}", e);
                return Err(blvm_node::module::traits::ModuleError::Other(format!("Mesh manager startup failed: {}", e)));
            }
            let manager = Arc::new(manager);
            let mesh_api = Arc::new(MeshModuleAPI::new(Arc::clone(&manager)));
            if let Err(e) = node_api.register_module_api(mesh_api).await {
                error!("Failed to register mesh module API: {}", e);
            }
            tracing::info!("Mesh module initialized and API registered");
            let module = MeshModule {
                manager: Arc::clone(&manager),
                data_dir,
            };
            Ok((module.clone(), module))
        }
    };

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module_type: MeshModule,
        cli_type: MeshModule,
        db: db.as_db(),
        setup: setup,
        event_types: MeshModule::event_types(),
    }?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
