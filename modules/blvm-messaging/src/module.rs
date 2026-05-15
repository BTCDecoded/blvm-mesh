//! Messaging module: unified CLI via #[module] macro.

use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::message::MessagingService;

#[derive(Clone)]
pub struct MessagingModule {
    pub messaging_service: Arc<MessagingService>,
}

#[module]
impl MessagingModule {
    #[on_event(MeshPacketReceived)]
    async fn on_mesh_packet(
        &self,
        packet_data: &[u8],
        peer_addr: &str,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let _ = ctx;
        if packet_data.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
            debug!(
                peer = %peer_addr,
                bytes = packet_data.len(),
                "dropping oversized mesh packet",
            );
            return Ok(());
        }
        let packet: blvm_mesh::MeshPacket = match bincode::deserialize(packet_data) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let Some(ref metadata) = packet.metadata else {
            return Ok(());
        };
        if !metadata
            .protocol
            .as_ref()
            .map(|s| s == "messaging-v1")
            .unwrap_or(false)
        {
            return Ok(());
        }
        let my_node_id = self.messaging_service.local_node_id();
        match self
            .messaging_service
            .handle_incoming_message(packet.payload, my_node_id)
            .await
        {
            Ok(Some(message)) => {
                info!("Message received: {} bytes", message.len());
            }
            Ok(None) => {
                debug!("Message processed (intermediate)");
            }
            Err(e) => {
                error!("Failed to handle message: {}", e);
            }
        }
        Ok(())
    }

    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok("blvm-messaging module\nProtocol: messaging-v1\nRunning: true".into())
    }

    #[command]
    fn list_conversations(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let service = Arc::clone(&self.messaging_service);
        run_async(async move {
            let convs = service.list_conversations().await;
            let lines: Vec<String> = convs
                .iter()
                .map(|(id, ts, dir)| {
                    format!(
                        "  {}... last_seen={} dir={}",
                        hex::encode(&id[..8]),
                        ts,
                        dir
                    )
                })
                .collect();
            Ok::<_, String>(format!(
                "Conversations ({}):\n{}",
                convs.len(),
                if lines.is_empty() {
                    "  (none)".into()
                } else {
                    lines.join("\n")
                },
            ))
        })
    }

    #[command]
    fn send_message(
        &self,
        _ctx: &InvocationContext,
        recipient_hex: String,
        content: String,
    ) -> Result<String, ModuleError> {
        let recipient_hex = recipient_hex.trim();
        if recipient_hex.is_empty() || recipient_hex.len() != 64 {
            return Err(ModuleError::Other(
                "Usage: send-message <recipient_node_id_hex_64chars> <content>".into(),
            ));
        }
        let bytes = hex::decode(recipient_hex).map_err(|_| {
            ModuleError::Other("Invalid recipient: must be 64 hex chars (32 bytes)".into())
        })?;
        if bytes.len() != 32 {
            return Err(ModuleError::Other(
                "Invalid recipient: must be 64 hex chars (32 bytes)".into(),
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let service = Arc::clone(&self.messaging_service);
        run_async(async move {
            let msg_id = service
                .send_message(arr, content.into_bytes(), None)
                .await?;
            Ok::<_, String>(format!("Message sent, id={}", hex::encode(&msg_id[..8])))
        })
    }
}
