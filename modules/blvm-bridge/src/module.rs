//! Bridge module: unified CLI via #[module] macro.

use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::sync::Arc;

use crate::bridge::BridgeService;

#[derive(Clone)]
pub struct BridgeModule {
    pub bridge_service: Arc<BridgeService>,
    pub bridge_mode: String,
}

#[module]
impl BridgeModule {
    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok(format!(
            "blvm-bridge module\nMode: {}\nRunning: true",
            self.bridge_mode
        ))
    }

    #[command]
    fn list_bridges(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let service = Arc::clone(&self.bridge_service);
        run_async(async move {
            let bridges = service.list_bridges().await;
            let lines: Vec<String> = bridges
                .iter()
                .map(|b| {
                    format!(
                        "  {}... type={:?} bandwidth={} bps last_seen={}",
                        hex::encode(&b.node_id[..8]),
                        b.connection_type,
                        b.bandwidth_bps,
                        b.last_seen
                    )
                })
                .collect();
            Ok(format!(
                "Connected bridges ({}):\n{}",
                bridges.len(),
                if lines.is_empty() { "  (none)".into() } else { lines.join("\n") },
            ))
        })
    }

    #[command]
    fn add_bridge(
        &self,
        _ctx: &InvocationContext,
        node_id_hex: String,
        connection_type: Option<String>,
        bandwidth: Option<u64>,
    ) -> Result<String, ModuleError> {
        let node_id_hex = node_id_hex.trim();
        if node_id_hex.is_empty() || node_id_hex.len() != 64 {
            return Err(ModuleError::Other("Usage: add-bridge <node_id_hex_64chars> [type] [bandwidth]".into()));
        }
        let conn_type = connection_type.as_deref().unwrap_or("internet").to_string();
        let bandwidth = bandwidth.unwrap_or(0);
        let service = Arc::clone(&self.bridge_service);
        run_async(async move {
            service.add_bridge(node_id_hex, &conn_type, bandwidth).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(format!("Added bridge: {}...", &node_id_hex[..16]))
        })
    }
}
