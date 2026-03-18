//! Bridge service for various connectivity types

use blvm_mesh::{MeshClient, NodeId, PaymentProof};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Bridge mode/type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeMode {
    /// Satellite bridge (Blockstream Satellite, etc.)
    Satellite,
    /// Radio bridge (ham radio, LoRa, etc.)
    Radio,
    /// Internet bridge (VPN, tunnel, etc.)
    Internet,
    /// Custom bridge type
    Custom(String),
}

/// Bridge packet for relaying data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgePacket {
    pub source: NodeId,
    pub destination: NodeId,
    pub data: Vec<u8>,
    pub packet_type: BridgePacketType,
    pub timestamp: u64,
}

/// Bridge packet type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgePacketType {
    /// Bitcoin block
    Block,
    /// Bitcoin transaction
    Transaction,
    /// Mesh packet relay
    MeshRelay,
    /// General data
    Data,
}

/// Bridge service for various connectivity types
pub struct BridgeService {
    mesh_client: MeshClient,
    caller_module_id: String,
    bridge_mode: BridgeMode,
    relay_queue: Arc<RwLock<Vec<BridgePacket>>>,
    connected_bridges: Arc<RwLock<HashMap<NodeId, BridgeConnection>>>,
}

/// Bridge connection information
#[derive(Debug, Clone)]
pub struct BridgeConnection {
    pub node_id: NodeId,
    pub connection_type: BridgeMode,
    pub last_seen: u64,
    pub bandwidth_bps: u64, // Bytes per second
}

impl BridgeService {
    /// Create a new bridge service
    pub fn new(
        mesh_client: MeshClient,
        caller_module_id: String,
        bridge_mode: BridgeMode,
    ) -> Self {
        Self {
            mesh_client,
            caller_module_id,
            bridge_mode,
            relay_queue: Arc::new(RwLock::new(Vec::new())),
            connected_bridges: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a bridge connection
    pub async fn register_bridge(&self, connection: BridgeConnection) {
        info!(
            "Registering bridge: {:x?}, mode: {:?}, bandwidth: {} bps",
            &connection.node_id[..8],
            connection.connection_type,
            connection.bandwidth_bps
        );
        let mut bridges = self.connected_bridges.write().await;
        bridges.insert(connection.node_id, connection);
    }

    /// Relay data via bridge (when direct connection fails)
    ///
    /// This uses mesh routing as fallback when satellite/radio is unavailable.
    pub async fn relay_via_bridge(
        &self,
        destination: NodeId,
        data: Vec<u8>,
        packet_type: BridgePacketType,
        payment_proof: Option<PaymentProof>,
    ) -> Result<(), String> {
        info!(
            "Relaying {} bytes via bridge (mode: {:?}) to {:x?}",
            data.len(),
            &self.bridge_mode,
            &destination[..8]
        );

        // Get source node ID from mesh
        let source = self.mesh_client.get_node_id().await
            .map_err(|e| format!("Failed to get node ID: {}", e))?;

        // Create bridge packet
        let bridge_packet = BridgePacket {
            source,
            destination,
            data,
            packet_type,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Serialize bridge packet
        let serialized = bincode::serialize(&bridge_packet)
            .map_err(|e| format!("Failed to serialize bridge packet: {}", e))?;

        // Send via mesh (payment-gated for bridge relay)
        let response = self
            .mesh_client
            .send_packet(
                &self.caller_module_id,
                destination,
                serialized,
                payment_proof,
                Some("bridge-v1".to_string()),
            )
            .await
            .map_err(|e| format!("Failed to relay via mesh: {}", e))?;

        if !response.success {
            return Err(response
                .error
                .unwrap_or_else(|| "Unknown error".to_string()));
        }

        info!(
            "Bridge relay successful: {} hops, cost: {} sats",
            response.route_length,
            response.estimated_cost_sats
        );

        Ok(())
    }

    /// Handle incoming bridge packet
    pub async fn handle_bridge_packet(
        &self,
        packet_payload: Vec<u8>,
        my_node_id: NodeId,
    ) -> Result<Option<BridgePacket>, String> {
        debug!("Handling incoming bridge packet: {} bytes", packet_payload.len());

        if packet_payload.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(format!(
                "Bridge packet too large: {} bytes (max: {} bytes)",
                packet_payload.len(),
                blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE
            ));
        }
        // Deserialize bridge packet
        let bridge_packet: BridgePacket = bincode::deserialize(&packet_payload)
            .map_err(|e| format!("Failed to deserialize bridge packet: {}", e))?;

        // Check if packet is for us
        if bridge_packet.destination != my_node_id {
            // Not for us - might need to forward
            return Ok(None);
        }

        info!(
            "Bridge packet received from {:x?}: {} bytes, type: {:?}",
            &bridge_packet.source[..8],
            bridge_packet.data.len(),
            bridge_packet.packet_type
        );

        // Process based on packet type
        match bridge_packet.packet_type {
            BridgePacketType::Block => {
                debug!("Relaying block via bridge");
                // TODO: Inject block into node
            }
            BridgePacketType::Transaction => {
                debug!("Relaying transaction via bridge");
                // TODO: Inject transaction into mempool
            }
            BridgePacketType::MeshRelay => {
                debug!("Relaying mesh packet via bridge");
                // TODO: Forward to mesh
            }
            BridgePacketType::Data => {
                debug!("Relaying general data via bridge");
                // TODO: Handle general data
            }
        }

        Ok(Some(bridge_packet))
    }

    /// Get bridge statistics
    pub async fn get_stats(&self) -> BridgeStats {
        let bridges = self.connected_bridges.read().await;
        let queue = self.relay_queue.read().await;
        
        BridgeStats {
            bridge_mode: self.bridge_mode.clone(),
            connected_bridges: bridges.len(),
            queued_packets: queue.len(),
        }
    }

    /// List connected bridges
    pub async fn list_bridges(&self) -> Vec<BridgeConnection> {
        let bridges = self.connected_bridges.read().await;
        bridges.values().cloned().collect()
    }

    /// Add a bridge connection by node_id (hex) and connection type
    pub async fn add_bridge(
        &self,
        node_id_hex: &str,
        connection_type: &str,
        bandwidth_bps: u64,
    ) -> Result<(), String> {
        let node_id = hex::decode(node_id_hex)
            .map_err(|e| format!("Invalid node_id hex: {}", e))?;
        if node_id.len() != 32 {
            return Err("node_id must be 64 hex chars (32 bytes)".to_string());
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&node_id);
        let conn_type = match connection_type.to_lowercase().as_str() {
            "satellite" => BridgeMode::Satellite,
            "radio" => BridgeMode::Radio,
            "internet" => BridgeMode::Internet,
            other => BridgeMode::Custom(other.to_string()),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let connection = BridgeConnection {
            node_id: arr,
            connection_type: conn_type,
            last_seen: now,
            bandwidth_bps,
        };
        self.register_bridge(connection).await;
        Ok(())
    }
}

/// Bridge statistics
#[derive(Debug, Clone)]
pub struct BridgeStats {
    pub bridge_mode: BridgeMode,
    pub connected_bridges: usize,
    pub queued_packets: usize,
}

