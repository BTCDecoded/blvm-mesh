//! Bridge module: unified CLI via #[module] macro.

use blvm_node::module::ipc::protocol::EventMessage;
use blvm_node::module::traits::EventType;
use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::sync::Arc;
use tracing::info;

use crate::bridge::BridgeService;

#[derive(Clone)]
pub struct BridgeModule {
    pub bridge_service: Arc<BridgeService>,
    pub bridge_mode: String,
    /// Display copy of `BRIDGE_EDGE_TRANSPORT` (see `blvm_mesh::edge_transport`).
    pub edge_transport: String,
}

#[module]
impl BridgeModule {
    #[on_event(PeerConnected, PeerDisconnected, NewBlock, MempoolTransactionAdded)]
    async fn on_bridge_event(
        &self,
        event: &EventMessage,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let _ = ctx;
        match event.event_type {
            EventType::PeerConnected => info!("Peer connected (bridge)"),
            EventType::PeerDisconnected => info!("Peer disconnected (bridge)"),
            EventType::NewBlock => info!("New block — bridge may relay"),
            EventType::MempoolTransactionAdded => {}
            _ => {}
        }
        Ok(())
    }

    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok(format!(
            "blvm-bridge module\nMode: {}\nEdge transport: {}\nRunning: true",
            self.bridge_mode, self.edge_transport
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
                        "  {}... type={:?} edge={:?} bandwidth={} bps last_seen={}",
                        hex::encode(&b.node_id[..8]),
                        b.connection_type,
                        b.edge_transport,
                        b.bandwidth_bps,
                        b.last_seen
                    )
                })
                .collect();
            Ok::<_, String>(format!(
                "Connected bridges ({}):\n{}",
                bridges.len(),
                if lines.is_empty() {
                    "  (none)".into()
                } else {
                    lines.join("\n")
                },
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
            return Err(ModuleError::Other(
                "Usage: add-bridge <node_id_hex_64chars> [type] [bandwidth]".into(),
            ));
        }
        let conn_type = connection_type.as_deref().unwrap_or("internet").to_string();
        let bandwidth = bandwidth.unwrap_or(0);
        let service = Arc::clone(&self.bridge_service);
        run_async(async move {
            service
                .add_bridge(node_id_hex, &conn_type, bandwidth)
                .await?;
            Ok::<_, String>(format!("Added bridge: {}...", &node_id_hex[..16]))
        })
    }
}
