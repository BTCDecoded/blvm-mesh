//! Messaging functionality for blvm-onion

use blvm_mesh::{MeshClient, NodeId};
use crate::encryption::OnionEncryption;
use crate::onion::OnionRouteBuilder;
use blvm_mesh::PaymentProof;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Onion messaging handler
pub struct OnionMessaging {
    mesh_client: MeshClient,
    route_builder: OnionRouteBuilder,
    encryption: OnionEncryption,
    caller_module_id: String,
    node_id: NodeId, // Cache node ID
    circuits: Arc<RwLock<HashMap<[u8; 32], Vec<NodeId>>>>, // circuit_id -> route
}

impl OnionMessaging {
    /// Create a new onion messaging handler
    pub async fn new(
        mesh_client: MeshClient,
        route_builder: OnionRouteBuilder,
        encryption: OnionEncryption,
        caller_module_id: String,
    ) -> Result<Self, String> {
        // Get node ID from mesh
        let node_id = mesh_client.get_node_id().await
            .map_err(|e| format!("Failed to get node ID: {}", e))?;

        Ok(Self {
            mesh_client,
            route_builder,
            encryption,
            caller_module_id,
            node_id,
            circuits: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a circuit to destination (route is built and stored)
    pub async fn create_circuit(
        &self,
        destination: NodeId,
        available_nodes: Vec<NodeId>,
    ) -> Result<[u8; 32], String> {
        let route = self
            .route_builder
            .build_route(self.node_id, destination, available_nodes)
            .await?;
        self.route_builder.validate_route(&route)?;
        let mut hasher = Sha256::new();
        hasher.update(&route[0]);
        hasher.update(&route[route.len() - 1]);
        hasher.update(&std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_le_bytes());
        let hash = hasher.finalize();
        let mut circuit_id = [0u8; 32];
        circuit_id.copy_from_slice(&hash);
        let mut circuits = self.circuits.write().await;
        circuits.insert(circuit_id, route.clone());
        info!("Created circuit {:x?} ({} hops)", &circuit_id[..8], route.len());
        Ok(circuit_id)
    }

    /// List active circuits
    pub async fn list_circuits(&self) -> Vec<([u8; 32], Vec<NodeId>)> {
        let circuits = self.circuits.read().await;
        circuits.iter().map(|(id, route)| (*id, route.clone())).collect()
    }

    /// Send a message via onion routing
    ///
    /// This method:
    /// 1. Builds an onion route to the destination
    /// 2. Encrypts the message with onion layers
    /// 3. Sends via blvm-mesh with payment proof
    pub async fn send_message(
        &self,
        destination: NodeId,
        message: Vec<u8>,
        available_nodes: Vec<NodeId>,
        payment_proof: Option<PaymentProof>,
    ) -> Result<(), String> {
        info!("Sending onion message to {:x?}", &destination[..8]);

        // Step 1: Use cached node ID
        let source = self.node_id;

        // Step 2: Build onion route
        let route = self
            .route_builder
            .build_route(source, destination, available_nodes)
            .await?;

        // Validate route
        self.route_builder.validate_route(&route)?;

        info!(
            "Built onion route: {} hops (source -> {} intermediates -> destination)",
            route.len(),
            route.len() - 2
        );

        // Step 3: Encrypt with onion layers
        let onion_message = self.encryption.encrypt_onion(message, &route)?;

        // Serialize onion message
        let encrypted_payload = bincode::serialize(&onion_message)
            .map_err(|e| format!("Failed to serialize onion message: {}", e))?;

        debug!(
            "Encrypted message: {} bytes (original + {} onion layers)",
            encrypted_payload.len(),
            route.len() - 1
        );

        // Step 4: Send via blvm-mesh
        let response = self
            .mesh_client
            .send_packet(
                &self.caller_module_id,
                destination,
                encrypted_payload,
                payment_proof,
                Some("onion-v1".to_string()),
            )
            .await
            .map_err(|e| format!("Failed to send packet via mesh: {}", e))?;

        if !response.success {
            return Err(response
                .error
                .unwrap_or_else(|| "Unknown error".to_string()));
        }

        info!(
            "Onion message sent successfully: {} hops, cost: {} sats",
            response.route_length, response.estimated_cost_sats
        );

        Ok(())
    }

    /// Handle an incoming onion packet
    ///
    /// This is called when a packet with protocol_id "onion-v1" arrives.
    /// The packet payload contains an OnionMessage that needs to be decrypted.
    pub async fn handle_incoming_packet(
        &self,
        packet_payload: Vec<u8>,
        my_node_id: NodeId,
    ) -> Result<Option<Vec<u8>>, String> {
        debug!("Handling incoming onion packet: {} bytes", packet_payload.len());

        if packet_payload.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(format!(
                "Onion packet too large: {} bytes (max: {} bytes)",
                packet_payload.len(),
                blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE
            ));
        }
        // Deserialize onion message
        let onion_message: crate::encryption::OnionMessage = bincode::deserialize(&packet_payload)
            .map_err(|e| format!("Failed to deserialize onion message: {}", e))?;

        // Decrypt one layer
        let (next_hop, inner_message) = self
            .encryption
            .decrypt_layer(&onion_message, my_node_id)?;

        if let Some(next_hop_id) = next_hop {
            // We're an intermediate node - forward the inner message
            debug!("Forwarding onion packet to next hop: {:x?}", &next_hop_id[..8]);
            
            // Create new OnionMessage with inner payload
            let inner_onion = crate::encryption::OnionMessage {
                encrypted_payload: inner_message,
                route_hint: None,
            };
            
            // Serialize and forward
            let forward_payload = bincode::serialize(&inner_onion)
                .map_err(|e| format!("Failed to serialize inner message: {}", e))?;
            
            // Forward via mesh (no payment proof needed for forwarding)
            let _ = self
                .mesh_client
                .send_packet(
                    &self.caller_module_id,
                    next_hop_id,
                    forward_payload,
                    None, // Forwarding doesn't require payment
                    Some("onion-v1".to_string()),
                )
                .await
                .map_err(|e| format!("Failed to forward packet: {}", e))?;
            
            // Return None to indicate we forwarded, not delivered
            return Ok(None);
        }
        
        // We're the destination - return the decrypted message
        debug!("Onion message delivered to destination");
        Ok(Some(inner_message))
    }
}

