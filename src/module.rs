//! Mesh module: unified CLI via #[module] macro.

use blvm_node::module::ipc::protocol::{EventMessage, ModuleMessage};
use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::path::PathBuf;
use std::sync::Arc;

use crate::manager::MeshManager;

/// Mesh module: manager + CLI in one struct.
#[derive(Clone)]
pub struct MeshModule {
    pub manager: Arc<MeshManager>,
    pub data_dir: PathBuf,
}

#[module]
impl MeshModule {
    #[on_event(MeshPacketReceived)]
    async fn on_mesh_packet_received(
        &self,
        packet_data: &[u8],
        peer_addr: &str,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let _ = ctx;
        self.manager
            .handle_mesh_packet_received(packet_data, peer_addr)
            .await
            .map_err(|e| ModuleError::Other(e.to_string()))
    }

    #[on_event(
        PeerConnected,
        PeerDisconnected,
        MessageReceived,
        MessageSent,
        PaymentRequestCreated,
        PaymentVerified,
        PaymentSettled,
        NewBlock,
        ChainReorg,
        MempoolTransactionAdded,
        FeeRateChanged
    )]
    async fn on_mesh_event(
        &self,
        event: &EventMessage,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let node_api = ctx
            .node_api()
            .ok_or_else(|| ModuleError::Other("NodeAPI not available".into()))?;
        let msg = ModuleMessage::Event(event.clone());
        self.manager
            .handle_event(&msg, node_api.as_ref())
            .await
            .map_err(|e| ModuleError::Other(e.to_string()))
    }

    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let manager = Arc::clone(&self.manager);
        run_async(async move {
            let stats = manager.get_stats().await;
            Ok::<_, String>(format!(
                "Mesh module\n\
                 Enabled: {}\n\
                 Mode: {:?}\n\
                 Node ID: {}\n\
                 Direct peers: {}\n\
                 Total routes: {}\n\
                 Cached routes: {}\n\
                 Tracked peers (replay): {}",
                stats.enabled,
                stats.mode,
                hex::encode(manager.node_id()),
                stats.routing.direct_peers,
                stats.routing.total_routes,
                stats.routing.cached_routes,
                stats.replay.tracked_peers,
            ))
        })
    }

    #[command]
    fn peers(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let manager = Arc::clone(&self.manager);
        run_async(async move {
            let stats = manager.get_stats().await;
            Ok::<_, String>(format!(
                "Direct peers: {}\nTotal routes: {}",
                stats.routing.direct_peers, stats.routing.total_routes,
            ))
        })
    }

    #[command]
    fn list_routes(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let routes = self.manager.list_routes();
        let lines: Vec<String> = routes
            .iter()
            .map(|(node_id, is_direct, hops, cost)| {
                let node_hex = hex::encode(&node_id[..8]);
                let kind = if *is_direct { "direct" } else { "hop" };
                format!(
                    "  {node_hex}... {kind} hops={hops} cost={cost} sats"
                )
            })
            .collect();
        Ok(format!(
            "Routes ({}):\n{}",
            lines.len(),
            if lines.is_empty() {
                "  (none)".into()
            } else {
                lines.join("\n")
            },
        ))
    }

    #[command]
    fn config_path(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok(self.data_dir.join("config.toml").display().to_string())
    }

    #[command]
    fn add_peer(
        &self,
        _ctx: &InvocationContext,
        address: String,
        node_id_hex: String,
    ) -> Result<String, ModuleError> {
        let addr = address.trim();
        if addr.is_empty() {
            return Err(ModuleError::Other(
                "Usage: add-peer <address> [node_id_hex]".into(),
            ));
        }
        let node_id = if node_id_hex.trim().is_empty() {
            None
        } else {
            Some(
                MeshManager::parse_node_id_hex(&node_id_hex)
                    .map_err(|e| ModuleError::Other(e.to_string()))?,
            )
        };
        self.manager
            .add_peer_with_id(addr, node_id)
            .map_err(|e| ModuleError::Other(e.to_string()))?;
        let id_note = node_id
            .map(|id| format!(", node_id={}", hex::encode(&id[..8])))
            .unwrap_or_default();
        Ok(format!("Added peer: {addr}{id_note}"))
    }

    #[command]
    fn remove_peer(
        &self,
        _ctx: &InvocationContext,
        address: String,
    ) -> Result<String, ModuleError> {
        let addr = address.trim();
        if addr.is_empty() {
            return Err(ModuleError::Other("Usage: remove-peer <address>".into()));
        }
        self.manager
            .remove_peer(addr)
            .map_err(|e| ModuleError::Other(e.to_string()))?;
        Ok(format!("Removed peer: {addr}"))
    }

    #[command]
    fn set_mode(&self, _ctx: &InvocationContext, mode: String) -> Result<String, ModuleError> {
        let mode_str = mode.trim();
        if mode_str.is_empty() {
            return Err(ModuleError::Other(
                "Usage: set-mode <bitcoin_only|payment_gated|open>".into(),
            ));
        }
        let mesh_mode = crate::routing_policy::MeshMode::from(mode_str);
        self.manager
            .set_mode(mesh_mode)
            .map_err(|e| ModuleError::Other(e.to_string()))?;
        Ok(format!("Mode set to: {mode_str}"))
    }

    #[command]
    fn send_packet(
        &self,
        _ctx: &InvocationContext,
        destination: String,
        payload: String,
        protocol_id: String,
    ) -> Result<String, ModuleError> {
        let dest_hex = destination.trim().trim_start_matches("0x");
        if dest_hex.len() != 64 || !dest_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ModuleError::Other(format!(
                "Destination must be 64 hex chars (NodeId). Got: {destination}"
            )));
        }
        let dest_bytes = hex::decode(dest_hex).map_err(|e| ModuleError::Other(e.to_string()))?;
        let mut dest_id = [0u8; 32];
        dest_id.copy_from_slice(&dest_bytes);
        let payload_bytes = if payload.trim().starts_with("0x")
            || payload.trim().chars().all(|c| c.is_ascii_hexdigit())
        {
            hex::decode(payload.trim().trim_start_matches("0x"))
                .unwrap_or_else(|_| payload.into_bytes())
        } else {
            payload.into_bytes()
        };
        let protocol = protocol_id.trim();
        let protocol = if protocol.is_empty() {
            None
        } else {
            Some(protocol.to_string())
        };
        let manager = Arc::clone(&self.manager);
        run_async(async move {
            let mut packet = crate::packet::MeshPacket::new(
                crate::packet::PacketType::Paid,
                manager.node_id(),
                dest_id,
                payload_bytes,
            );
            if let Some(protocol_id) = protocol {
                packet.metadata = Some(crate::packet::PacketMetadata {
                    protocol: Some(protocol_id),
                    fields: std::collections::HashMap::new(),
                });
            }
            manager
                .route_packet(&packet)
                .await
                .map(|_| format!("Packet sent to {}", hex::encode(&dest_id[..8])))
                .map_err(|e| anyhow::anyhow!("Failed to send packet: {}", e))
        })
    }
}
