//! ModuleAPI implementation for blvm-mesh
//!
//! Exposes mesh networking functionality to other modules via inter-module IPC.

use crate::manager::MeshManager;
use crate::packet::MeshPacket;
use crate::payment_proof::PaymentProof;
use crate::routing::NodeId;
use async_trait::async_trait;
use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::traits::ModuleError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{debug, info};

/// Request to send a packet through the mesh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendPacketRequest {
    pub destination: NodeId,
    pub payload: Vec<u8>,
    pub payment_proof: Option<PaymentProof>,
    pub protocol_id: Option<String>,
    pub ttl: Option<u64>,
}

/// Response from sending a packet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendPacketResponse {
    pub success: bool,
    pub packet_id: [u8; 32],
    pub route_length: usize,
    pub estimated_cost_sats: u64,
    pub error: Option<String>,
}

/// Request to discover a route
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverRouteRequest {
    pub destination: NodeId,
    pub max_hops: Option<u8>,
    pub timeout_seconds: Option<u64>,
}

/// Response from route discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverRouteResponse {
    pub route: Option<Vec<NodeId>>,
    pub route_cost_sats: u64,
    pub discovery_time_ms: u64,
}

/// Relay-issued hop invoice for mesh route payment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HopInvoiceResponse {
    pub invoice: String,
    pub amount_msats: u64,
    pub expires_at: u64,
}

/// Request to register a protocol handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterProtocolRequest {
    pub protocol_id: String,
    pub handler_method: String, // Method name in calling module
}

/// Response from protocol registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterProtocolResponse {
    pub success: bool,
}

/// Peer entry for get_peer_list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    pub node_id_hex: String,
    pub address: String,
}

/// Mesh Module API implementation
pub struct MeshModuleAPI {
    manager: Arc<MeshManager>,
    protocol_handlers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
}

impl MeshModuleAPI {
    pub fn new(manager: Arc<MeshManager>) -> Self {
        Self {
            manager,
            protocol_handlers: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get node ID from manager (helper method)
    fn get_node_id(&self) -> NodeId {
        self.manager.node_id()
    }

    /// Calculate packet hash (for packet ID)
    fn packet_hash(packet: &MeshPacket) -> [u8; 32] {
        let serialized = bincode::serialize(packet).unwrap_or_default();
        let hash = Sha256::digest(&serialized);
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }
}

#[async_trait]
impl ModuleAPI for MeshModuleAPI {
    async fn handle_request(
        &self,
        method: &str,
        params: &[u8],
        caller_module_id: &str,
    ) -> Result<Vec<u8>, ModuleError> {
        debug!(
            "Mesh API call: {}::{} from {}",
            method, caller_module_id, caller_module_id
        );

        match method {
            "send_packet" => {
                if params.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
                    return Err(ModuleError::OperationError(format!(
                        "Request too large: {} bytes (max: {} bytes)",
                        params.len(),
                        crate::packet::MAX_BINCODE_PAYLOAD_SIZE
                    )));
                }
                let req: SendPacketRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;

                // Get node ID from manager
                let node_id = self.get_node_id();

                // Create mesh packet
                let mut packet = if let Some(ref proof) = req.payment_proof {
                    MeshPacket::new_paid(
                        node_id,
                        req.destination,
                        req.payload.clone(),
                        proof.clone(),
                    )
                } else {
                    MeshPacket::new(
                        crate::packet::PacketType::Paid,
                        node_id,
                        req.destination,
                        req.payload,
                    )
                };

                // Set protocol metadata if provided
                if let Some(protocol_id) = req.protocol_id {
                    packet.metadata = Some(crate::packet::PacketMetadata {
                        protocol: Some(protocol_id),
                        fields: std::collections::HashMap::new(),
                    });
                }

                // Route the packet
                match self.manager.route_packet(&packet).await {
                    Ok(_) => {
                        let packet_id = Self::packet_hash(&packet);
                        let response = SendPacketResponse {
                            success: true,
                            packet_id,
                            route_length: packet.route.len(),
                            estimated_cost_sats: req
                                .payment_proof
                                .as_ref()
                                .map(|p| p.amount_sats())
                                .unwrap_or(0),
                            error: None,
                        };
                        Ok(bincode::serialize(&response)?)
                    }
                    Err(e) => {
                        let response = SendPacketResponse {
                            success: false,
                            packet_id: [0u8; 32],
                            route_length: 0,
                            estimated_cost_sats: 0,
                            error: Some(e.to_string()),
                        };
                        Ok(bincode::serialize(&response)?)
                    }
                }
            }

            "discover_route" => {
                if params.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
                    return Err(ModuleError::OperationError(format!(
                        "Request too large: {} bytes (max: {} bytes)",
                        params.len(),
                        crate::packet::MAX_BINCODE_PAYLOAD_SIZE
                    )));
                }
                let req: DiscoverRouteRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;

                let start = std::time::Instant::now();

                let route = self
                    .manager
                    .discover_route(req.destination)
                    .await
                    .map_err(|e| {
                        ModuleError::OperationError(format!("Route discovery failed: {e}"))
                    })?;

                let discovery_time = start.elapsed().as_millis() as u64;

                let response = DiscoverRouteResponse {
                    route: route.clone(),
                    route_cost_sats: route
                        .as_ref()
                        .map(|r| r.len() as u64 * 10) // 10 sats per hop estimate
                        .unwrap_or(0),
                    discovery_time_ms: discovery_time,
                };

                Ok(bincode::serialize(&response)?)
            }

            "register_protocol_handler" => {
                // Deprecated: prefer MeshPacketReceived events + MeshAppTransport.
                if params.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
                    return Err(ModuleError::OperationError(format!(
                        "Request too large: {} bytes (max: {} bytes)",
                        params.len(),
                        crate::packet::MAX_BINCODE_PAYLOAD_SIZE
                    )));
                }
                let req: RegisterProtocolRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;

                let mut handlers = self.protocol_handlers.write().await;
                handlers.insert(
                    req.protocol_id.clone(),
                    format!("{}::{}", caller_module_id, req.handler_method),
                );

                info!(
                    "Registered protocol handler: {} -> {}::{}",
                    req.protocol_id, caller_module_id, req.handler_method
                );

                let response = RegisterProtocolResponse { success: true };
                Ok(bincode::serialize(&response)?)
            }

            "get_routing_stats" => {
                let stats = self.manager.get_stats().await;
                Ok(bincode::serialize(&stats)?)
            }

            "get_peer_list" => {
                let peers: Vec<PeerEntry> = self
                    .manager
                    .list_direct_peers()
                    .into_iter()
                    .map(|(node_id, addr)| PeerEntry {
                        node_id_hex: hex::encode(node_id),
                        address: addr,
                    })
                    .collect();
                Ok(bincode::serialize(&peers)?)
            }

            "get_route_stats" => {
                let stats = self.manager.get_stats().await;
                Ok(bincode::serialize(&stats.routing)?)
            }

            "get_network_stats" => {
                let stats = self.manager.get_stats().await;
                Ok(bincode::serialize(&stats)?)
            }

            "get_node_id" => {
                let node_id = self.manager.node_id();
                Ok(bincode::serialize(&node_id)?)
            }

            "handle_mesh_packet" => {
                let (packet_data, peer_addr): (Vec<u8>, String) = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;
                self.manager
                    .handle_mesh_packet_received(&packet_data, &peer_addr)
                    .await
                    .map_err(|e| ModuleError::OperationError(e.to_string()))?;
                Ok(bincode::serialize(&true)?)
            }

            "quote_route_fee" => {
                #[derive(Deserialize)]
                struct QuoteRequest {
                    destination: NodeId,
                    base_fee_sats: u64,
                }
                let req: QuoteRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;
                let fee = self
                    .manager
                    .quote_route_fee_sats(req.destination, req.base_fee_sats);
                Ok(bincode::serialize(&fee)?)
            }

            "request_hop_invoice" => {
                #[derive(Deserialize)]
                struct HopInvoiceRequest {
                    destination: NodeId,
                    amount_msats: u64,
                    expiry_seconds: Option<u64>,
                }
                let req: HopInvoiceRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {e}")))?;
                let expiry = req.expiry_seconds.unwrap_or(3600);
                let response = self
                    .manager
                    .request_hop_invoice(req.destination, req.amount_msats, expiry)
                    .await
                    .map_err(|e| ModuleError::OperationError(e.to_string()))?;
                Ok(bincode::serialize(&response)?)
            }

            "poll_local_deliveries" => {
                #[derive(Deserialize)]
                struct PollRequest {
                    protocol_id: Option<String>,
                    max_packets: Option<usize>,
                }
                let req: PollRequest = bincode::deserialize(params).map_err(|e| {
                    ModuleError::OperationError(format!("Invalid poll request: {e}"))
                })?;
                let max = req.max_packets.unwrap_or(16);
                let deliveries = self
                    .manager
                    .poll_local_deliveries(req.protocol_id.as_deref(), max)
                    .await;
                Ok(bincode::serialize(&deliveries)?)
            }

            _ => Err(ModuleError::OperationError(format!(
                "Unknown method: {method}"
            ))),
        }
    }

    fn list_methods(&self) -> Vec<String> {
        vec![
            "send_packet".to_string(),
            "discover_route".to_string(),
            "register_protocol_handler".to_string(),
            "get_routing_stats".to_string(),
            "get_peer_list".to_string(),
            "get_route_stats".to_string(),
            "get_network_stats".to_string(),
            "get_node_id".to_string(),
            "handle_mesh_packet".to_string(),
            "quote_route_fee".to_string(),
            "request_hop_invoice".to_string(),
            "poll_local_deliveries".to_string(),
        ]
    }

    fn api_version(&self) -> u32 {
        1
    }
}
