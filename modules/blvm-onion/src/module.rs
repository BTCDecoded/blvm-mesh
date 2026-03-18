//! Onion module: unified CLI via #[module] macro.

use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::sync::Arc;

use crate::messaging::OnionMessaging;

#[derive(Clone)]
pub struct OnionModule {
    pub onion_messaging: Arc<OnionMessaging>,
}

#[module]
impl OnionModule {
    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok("blvm-onion module\nProtocol: onion-v1\nRunning: true".into())
    }

    #[command]
    fn list_circuits(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let messaging = Arc::clone(&self.onion_messaging);
        run_async(async move {
            let circuits = messaging.list_circuits().await;
            let lines: Vec<String> = circuits
                .iter()
                .map(|(id, route)| format!("  {}... hops={}", hex::encode(&id[..8]), route.len()))
                .collect();
            Ok(format!(
                "Circuits ({}):\n{}",
                lines.len(),
                if lines.is_empty() { "  (none)".into() } else { lines.join("\n") },
            ))
        })
    }

    #[command]
    fn create_circuit(&self, _ctx: &InvocationContext, destination_hex: String) -> Result<String, ModuleError> {
        let dest_hex = destination_hex.trim();
        if dest_hex.is_empty() || dest_hex.len() != 64 {
            return Err(ModuleError::Other("Usage: create-circuit <destination_node_id_hex_64chars>".into()));
        }
        let bytes = hex::decode(dest_hex).map_err(|_| ModuleError::Other("Invalid destination: must be 64 hex chars (32 bytes)".into()))?;
        if bytes.len() != 32 {
            return Err(ModuleError::Other("Invalid destination: must be 64 hex chars (32 bytes)".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let messaging = Arc::clone(&self.onion_messaging);
        run_async(async move {
            let circuit_id = messaging.create_circuit(arr, vec![]).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(format!("Circuit created: {}...", hex::encode(&circuit_id[..8])))
        })
    }
}
