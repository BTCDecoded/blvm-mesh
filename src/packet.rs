//! Mesh packet structure and serialization
//!
//! Defines the packet format for mesh networking, including headers,
//! routing information, and payment proofs.

use crate::payment_proof::PaymentProof;
use crate::routing::NodeId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Mesh packet magic bytes
pub const MESH_PACKET_MAGIC: [u8; 4] = [0x4D, 0x45, 0x53, 0x48]; // "MESH"

/// Mesh packet version
pub const MESH_PACKET_VERSION: u8 = 1;

/// Maximum packet size (1MB)
pub const MAX_PACKET_SIZE: usize = 1_000_000;

/// Mesh packet type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PacketType {
    /// Bitcoin P2P protocol message
    BitcoinP2P,
    /// Commons governance message
    CommonsGovernance,
    /// Stratum V2 message
    StratumV2,
    /// Paid mesh packet (arbitrary data, messaging, IPFS)
    Paid,
}

/// Mesh packet for routing through the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPacket {
    /// Packet version
    pub version: u8,
    /// Packet type
    pub packet_type: PacketType,
    /// Source node ID (32 bytes)
    pub source: NodeId,
    /// Destination node ID (32 bytes)
    pub destination: NodeId,
    /// Route path (list of node IDs, including source and destination)
    pub route: Vec<NodeId>,
    /// Sequence number (for ordering and duplicate detection)
    pub sequence: u64,
    /// Timestamp (Unix epoch seconds)
    pub timestamp: u64,
    /// Payment proof (required for Paid packets)
    pub payment_proof: Option<PaymentProof>,
    /// Packet payload
    pub payload: Vec<u8>,
    /// Optional metadata (protocol-specific)
    pub metadata: Option<PacketMetadata>,
}

/// Packet metadata (optional, protocol-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketMetadata {
    /// Protocol identifier (e.g., "bitcoin-p2p", "commons-governance", "stratum-v2", "mesh-packet")
    pub protocol: Option<String>,
    /// Additional protocol-specific fields
    pub fields: std::collections::HashMap<String, String>,
}

impl MeshPacket {
    /// Create a new mesh packet
    pub fn new(
        packet_type: PacketType,
        source: NodeId,
        destination: NodeId,
        payload: Vec<u8>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            version: MESH_PACKET_VERSION,
            packet_type,
            source,
            destination,
            route: vec![source], // Initial route starts with source
            sequence: 0, // Will be set by sender
            timestamp: now,
            payment_proof: None,
            payload,
            metadata: None,
        }
    }

    /// Create a paid mesh packet (with payment proof)
    pub fn new_paid(
        source: NodeId,
        destination: NodeId,
        payload: Vec<u8>,
        payment_proof: PaymentProof,
    ) -> Self {
        let mut packet = Self::new(PacketType::Paid, source, destination, payload);
        packet.payment_proof = Some(payment_proof);
        packet
    }

    /// Validate packet structure
    pub fn validate(&self) -> Result<(), String> {
        // Check version
        if self.version != MESH_PACKET_VERSION {
            return Err(format!("Invalid packet version: {}", self.version));
        }

        // Check size
        let size = self.serialized_size();
        if size > MAX_PACKET_SIZE {
            return Err(format!(
                "Packet size exceeds maximum: {} > {}",
                size, MAX_PACKET_SIZE
            ));
        }

        // Check route
        if self.route.is_empty() {
            return Err("Route cannot be empty".to_string());
        }

        // Check source matches route start
        if self.route[0] != self.source {
            return Err("Route must start with source node".to_string());
        }

        // Check destination matches route end
        if self.route.last() != Some(&self.destination) {
            return Err("Route must end with destination node".to_string());
        }

        // Check payment proof for paid packets
        if self.packet_type == PacketType::Paid && self.payment_proof.is_none() {
            return Err("Paid packets require payment proof".to_string());
        }

        Ok(())
    }

    /// Calculate serialized size
    pub fn serialized_size(&self) -> usize {
        // Header: version (1) + packet_type (1) + source (32) + destination (32) + sequence (8) + timestamp (8) = 82 bytes
        // Route: route.len() * 32
        // Payment proof: variable (if present)
        // Payload: payload.len()
        // Metadata: variable (if present)
        
        let mut size = 82;
        size += self.route.len() * 32;
        
        if let Some(ref proof) = self.payment_proof {
            // Estimate payment proof size (Lightning: ~500 bytes, CTV: ~200 bytes)
            size += 500; // Conservative estimate
        }
        
        size += self.payload.len();
        
        if let Some(ref metadata) = self.metadata {
            // Estimate metadata size
            size += 100; // Conservative estimate
        }
        
        size
    }

    /// Check if packet is for this node
    pub fn is_for_me(&self, my_node_id: &NodeId) -> bool {
        self.destination == *my_node_id
    }

    /// Check if packet should be forwarded
    pub fn should_forward(&self, my_node_id: &NodeId) -> bool {
        // If destination is this node, don't forward
        if self.is_for_me(my_node_id) {
            return false;
        }

        // If this node is in the route, forward
        self.route.contains(my_node_id)
    }

    /// Get next hop in route
    pub fn get_next_hop(&self, my_node_id: &NodeId) -> Option<NodeId> {
        // Find this node in the route
        let my_index = self.route.iter().position(|&id| id == *my_node_id)?;
        
        // Get next node in route
        if my_index + 1 < self.route.len() {
            Some(self.route[my_index + 1])
        } else {
            None
        }
    }

    /// Add this node to route (when forwarding)
    pub fn add_to_route(&mut self, node_id: NodeId) {
        // Only add if not already in route
        if !self.route.contains(&node_id) {
            // Insert before destination
            let dest_index = self.route.len() - 1;
            self.route.insert(dest_index, node_id);
        }
    }
}

