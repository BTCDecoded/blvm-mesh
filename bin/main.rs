//! blvm-mesh - Commons Mesh networking module
//!
//! When spawned by the node: reads MODULE_ID, SOCKET_PATH, DATA_DIR from env.
//! For manual testing: blvm-mesh --module-id <id> --socket-path <path> --data-dir <dir>

use anyhow::Result;
use blvm_mesh::MeshModule;
use blvm_mesh::api::MeshModuleAPI;
use blvm_mesh::config::MeshConfig;
use blvm_mesh::manager::MeshManager;
use blvm_mesh::storage::up_v1;
use blvm_node::module::ipc::protocol::{
    InvocationMessage, InvocationResultMessage, InvocationResultPayload, InvocationType,
};
use blvm_sdk::migrations;
use blvm_sdk::module::runner::{InvocationContext, run_module_with_setup_and_api};
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
            let (ctx, config) = bootstrap.context_with_config::<MeshConfig>(&data_dir);
            let manager = MeshManager::new(&ctx, Arc::clone(&node_api), Some(&config))
                .await
                .map_err(|e| {
                    blvm_node::module::traits::ModuleError::Other(format!(
                        "Failed to create mesh manager: {e}"
                    ))
                })?;
            if let Err(e) = manager.load_configured_peers(&config) {
                error!("Failed to load configured peers: {}", e);
            }
            if let Err(e) = manager.start().await {
                error!("Failed to start mesh manager: {}", e);
                return Err(blvm_node::module::traits::ModuleError::Other(format!(
                    "Mesh manager startup failed: {e}"
                )));
            }
            let manager = Arc::new(manager);
            let mesh_api: Arc<dyn blvm_node::module::inter_module::api::ModuleAPI> =
                Arc::new(MeshModuleAPI::new(Arc::clone(&manager)));
            let module = MeshModule {
                manager: Arc::clone(&manager),
                data_dir,
            };
            Ok((module.clone(), module, mesh_api))
        }
    };

    let dispatch = |invocation: InvocationMessage,
                    ctx: InvocationContext,
                    module: &MeshModule,
                    cli: &MeshModule| {
        let (success, payload, error) = match &invocation.invocation_type {
            InvocationType::Cli { subcommand, args } => {
                let args: Vec<String> = args.clone();
                match cli.dispatch_cli(&ctx, subcommand, &args) {
                    Ok(stdout) => (
                        true,
                        Some(InvocationResultPayload::Cli {
                            stdout,
                            stderr: String::new(),
                            exit_code: 0,
                        }),
                        None,
                    ),
                    Err(e) => (false, None, Some(e.to_string())),
                }
            }
            InvocationType::Rpc { method, params } => {
                let db_ref = ctx.db();
                match module.dispatch_rpc(method, params, db_ref) {
                    Ok(v) => (true, Some(InvocationResultPayload::Rpc(v)), None),
                    Err(e) => (false, None, Some(e.to_string())),
                }
            }
            InvocationType::ModuleApi { .. } => {
                // Handled by run_module_with_setup_and_api
                (
                    false,
                    None,
                    Some("ModuleApi dispatch should be handled by runner".to_string()),
                )
            }
        };
        InvocationResultMessage {
            correlation_id: invocation.correlation_id,
            success,
            payload,
            error,
        }
    };

    let on_event = |e, m: &MeshModule, ctx: &InvocationContext| {
        let m = m.clone();
        let ctx = ctx.clone();
        async move { m.dispatch_event(e, &ctx).await }
    };

    run_module_with_setup_and_api(
        bootstrap.socket_path.clone(),
        &bootstrap.module_id,
        MODULE_NAME,
        env!("CARGO_PKG_VERSION"),
        MeshModule::cli_spec(),
        MeshModule::rpc_method_names().as_slice(),
        MeshModule::event_types(),
        dispatch,
        on_event,
        setup,
        db.as_db(),
        bootstrap.data_dir.as_path(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
