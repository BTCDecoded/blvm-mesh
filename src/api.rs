//! ModuleAPI implementation for blvm-mesh
//!
//! Exposes mesh networking functionality to other modules via inter-module IPC.

use crate::manager::MeshManager;
use crate::packet::MeshPacket;
use crate::payment_proof::PaymentProof;
use crate::routing::NodeId;
use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::traits::ModuleError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, error, info};
use sha2::{Digest, Sha256};

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

/// Request to register a protocol handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterProtocolRequest {
    pub protocol_id: String,
    pub handler_method: String,  // Method name in calling module
}

/// Response from protocol registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterProtocolResponse {
    pub success: bool,
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
        debug!("Mesh API call: {}::{} from {}", method, caller_module_id, caller_module_id);
        
        match method {
            "send_packet" => {
                let req: SendPacketRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {}", e)))?;
                
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
                            estimated_cost_sats: req.payment_proof.as_ref()
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
                let req: DiscoverRouteRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {}", e)))?;
                
                let start = std::time::Instant::now();
                
                let route = self.manager
                    .discover_route(req.destination)
                    .await
                    .map_err(|e| ModuleError::OperationError(format!("Route discovery failed: {}", e)))?;
                
                let discovery_time = start.elapsed().as_millis() as u64;
                
                let response = DiscoverRouteResponse {
                    route: route.clone(),
                    route_cost_sats: route.as_ref()
                        .map(|r| r.len() as u64 * 10) // 10 sats per hop estimate
                        .unwrap_or(0),
                    discovery_time_ms: discovery_time,
                };
                
                Ok(bincode::serialize(&response)?)
            }
            
            "register_protocol_handler" => {
                let req: RegisterProtocolRequest = bincode::deserialize(params)
                    .map_err(|e| ModuleError::OperationError(format!("Invalid request: {}", e)))?;
                
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
            
            "get_node_id" => {
                let node_id = self.manager.node_id();
                Ok(bincode::serialize(&node_id)?)
            }
            
            _ => Err(ModuleError::OperationError(format!("Unknown method: {}", method)))
        }
    }
    
    fn list_methods(&self) -> Vec<String> {
        vec![
            "send_packet".to_string(),
            "discover_route".to_string(),
            "register_protocol_handler".to_string(),
            "get_routing_stats".to_string(),
            "get_node_id".to_string(),
        ]
    }
    
    fn api_version(&self) -> u32 {
        1
    }
}

