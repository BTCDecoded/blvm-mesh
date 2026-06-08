//! External JSON-RPC handlers registered via blvm-node module RPC extender.

use std::sync::Arc;

use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::traits::ModuleError;
use serde_json::{json, Value};

use crate::api::{MeshModuleAPI, SendPacketResponse};
use crate::manager::{LocalDelivery, MeshManager};

fn parse_destination_hex(params: &Value) -> Result<[u8; 32], ModuleError> {
    let destination_hex = params
        .get("destination_hex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ModuleError::OperationError("destination_hex required".into()))?;
    let destination_bytes = hex::decode(destination_hex.trim())
        .map_err(|e| ModuleError::OperationError(format!("destination_hex decode failed: {e}")))?;
    destination_bytes
        .try_into()
        .map_err(|_| ModuleError::OperationError("destination_hex must be 32 bytes".into()))
}

/// Params: `{ "request_hex": "...", "mesh_module_id": "blvm-mesh" }` (`mesh_module_id` ignored).
pub fn meshsendpacket(manager: &Arc<MeshManager>, params: &Value) -> Result<Value, ModuleError> {
    let request_hex = params
        .get("request_hex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ModuleError::OperationError("meshsendpacket requires request_hex".into()))?;

    let request_bytes = hex::decode(request_hex.trim())
        .map_err(|e| ModuleError::OperationError(format!("request_hex decode failed: {e}")))?;
    if request_bytes.is_empty() {
        return Err(ModuleError::OperationError("request_hex is empty".into()));
    }

    let api = MeshModuleAPI::new(Arc::clone(manager));
    let response_bytes = blvm_sdk::module::runner::run_async(api.handle_request(
        "send_packet",
        &request_bytes,
        "rpc",
    ))?;

    let result: SendPacketResponse = bincode::deserialize(&response_bytes)
        .map_err(|e| ModuleError::OperationError(format!("send_packet response decode: {e}")))?;

    Ok(json!({
        "success": result.success,
        "packet_id": hex::encode(result.packet_id),
        "route_length": result.route_length,
        "estimated_cost_sats": result.estimated_cost_sats,
        "error": result.error,
    }))
}

/// Params: `{ "protocol_id": "...", "max_packets": 16, "mesh_module_id": "blvm-mesh" }`.
pub fn meshpollreceived(manager: &Arc<MeshManager>, params: &Value) -> Result<Value, ModuleError> {
    let protocol_id = params.get("protocol_id").and_then(|v| v.as_str());
    let max_packets = params
        .get("max_packets")
        .and_then(|v| v.as_u64())
        .unwrap_or(16)
        .clamp(1, 64) as usize;

    #[derive(serde::Serialize)]
    struct PollRequest {
        protocol_id: Option<String>,
        max_packets: Option<usize>,
    }

    let request = PollRequest {
        protocol_id: protocol_id.map(String::from),
        max_packets: Some(max_packets),
    };
    let request_bytes = bincode::serialize(&request)
        .map_err(|e| ModuleError::OperationError(format!("poll request encode: {e}")))?;

    let api = MeshModuleAPI::new(Arc::clone(manager));
    let response_bytes = blvm_sdk::module::runner::run_async(api.handle_request(
        "poll_local_deliveries",
        &request_bytes,
        "rpc",
    ))?;

    let deliveries: Vec<LocalDelivery> = bincode::deserialize(&response_bytes)
        .map_err(|e| ModuleError::OperationError(format!("poll response decode: {e}")))?;

    let packets: Vec<Value> = deliveries
        .into_iter()
        .map(|d| {
            json!({
                "protocol_id": d.protocol_id,
                "payload_hex": hex::encode(d.payload),
                "source_hex": hex::encode(d.source),
            })
        })
        .collect();

    Ok(json!({ "packets": packets }))
}

/// Params: `{ "destination_hex": "...", "base_fee_sats": 1 }`.
pub fn meshquoteroute(manager: &Arc<MeshManager>, params: &Value) -> Result<Value, ModuleError> {
    let destination = parse_destination_hex(params)?;
    let base_fee_sats = params
        .get("base_fee_sats")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    let fee_sats = manager.quote_route_fee_sats(destination, base_fee_sats);
    Ok(json!({ "fee_sats": fee_sats }))
}

/// Params: `{ "destination_hex": "...", "amount_msats": 1000, "expiry_seconds": 3600 }`.
pub fn meshrequesthopinvoice(
    manager: &Arc<MeshManager>,
    params: &Value,
) -> Result<Value, ModuleError> {
    let destination = parse_destination_hex(params)?;
    let amount_msats = params
        .get("amount_msats")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ModuleError::OperationError("amount_msats required".into()))?;
    let expiry_seconds = params
        .get("expiry_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);

    let manager = Arc::clone(manager);
    let response = blvm_sdk::module::runner::run_async(async move {
        manager
            .request_hop_invoice(destination, amount_msats, expiry_seconds)
            .await
            .map_err(|e| ModuleError::OperationError(e.to_string()))
    })?;

    Ok(json!({
        "invoice": response.invoice,
        "amount_msats": response.amount_msats,
        "expires_at": response.expires_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing_policy::MeshMode;
    use crate::test_support::TestNodeAPI;

    #[test]
    fn meshsendpacket_requires_request_hex() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mgr = rt.block_on(async {
            Arc::new(
                MeshManager::new_for_test(
                    false,
                    MeshMode::Open,
                    [0u8; 32],
                    Arc::new(TestNodeAPI::default()),
                )
                .await,
            )
        });
        let err = meshsendpacket(&mgr, &json!({})).unwrap_err();
        assert!(err.to_string().contains("request_hex"));
    }

    #[test]
    fn meshquoteroute_parses_destination() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mgr = rt.block_on(async {
            Arc::new(
                MeshManager::new_for_test(
                    false,
                    MeshMode::Open,
                    [0u8; 32],
                    Arc::new(TestNodeAPI::default()),
                )
                .await,
            )
        });
        let result = meshquoteroute(
            &mgr,
            &json!({
                "destination_hex": hex::encode([0xAB; 32]),
                "base_fee_sats": 5
            }),
        )
        .unwrap();
        assert_eq!(result["fee_sats"], 0);
    }

    #[test]
    fn meshrequesthopinvoice_requires_amount() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mgr = rt.block_on(async {
            Arc::new(
                MeshManager::new_for_test(
                    false,
                    MeshMode::Open,
                    [0u8; 32],
                    Arc::new(TestNodeAPI::default()),
                )
                .await,
            )
        });
        let err =
            meshrequesthopinvoice(&mgr, &json!({ "destination_hex": hex::encode([1u8; 32]) }))
                .unwrap_err();
        assert!(err.to_string().contains("amount_msats"));
    }
}
