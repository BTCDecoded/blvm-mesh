//! Network integration for mesh packet forwarding
//!
//! This module provides integration points with the node's network layer
//! for sending and receiving mesh packets.

use crate::error::MeshError;
use crate::packet::{MeshPacket, MESH_PACKET_MAGIC};
use bincode;
use tracing::{debug, warn};

/// Check if data is a mesh packet
pub fn is_mesh_packet(data: &[u8]) -> bool {
    // Check for mesh packet magic bytes
    data.len() >= 4 && data[0..4] == MESH_PACKET_MAGIC
}

/// Deserialize mesh packet from bytes
pub fn deserialize_mesh_packet(data: &[u8]) -> Result<MeshPacket, MeshError> {
    // Check magic bytes first
    if !is_mesh_packet(data) {
        return Err(MeshError::InvalidPacket(
            "Not a mesh packet (invalid magic bytes)".to_string(),
        ));
    }
    
    // Deserialize packet (skip magic bytes if they're part of the data)
    // In production, magic bytes might be stripped by network layer
    let packet: MeshPacket = bincode::deserialize(data)
        .map_err(|e| MeshError::InvalidPacket(format!("Failed to deserialize packet: {}", e)))?;
    
    Ok(packet)
}

/// Serialize mesh packet to bytes
pub fn serialize_mesh_packet(packet: &MeshPacket) -> Result<Vec<u8>, MeshError> {
    // Validate packet before serialization
    packet.validate()
        .map_err(|e| MeshError::InvalidPacket(e))?;
    
    // Serialize packet
    let mut data = bincode::serialize(packet)
        .map_err(|e| MeshError::InvalidPacket(format!("Failed to serialize packet: {}", e)))?;
    
    // Prepend magic bytes (if not already included)
    // In production, network layer might handle magic bytes
    let mut packet_with_magic = MESH_PACKET_MAGIC.to_vec();
    packet_with_magic.extend_from_slice(&data);
    
    Ok(packet_with_magic)
}

/// Extract mesh packet from network message
///
/// This function checks if a network message contains a mesh packet
/// and extracts it if found.
pub fn extract_mesh_packet(data: &[u8]) -> Option<Result<MeshPacket, MeshError>> {
    if is_mesh_packet(data) {
        Some(deserialize_mesh_packet(data))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::PacketType;
    use crate::routing::NodeId;
    
    #[test]
    fn test_is_mesh_packet() {
        let mut data = vec![0u8; 100];
        data[0..4].copy_from_slice(&MESH_PACKET_MAGIC);
        assert!(is_mesh_packet(&data));
        
        let not_mesh = vec![0u8; 100];
        assert!(!is_mesh_packet(&not_mesh));
    }
    
    #[test]
    fn test_serialize_deserialize() {
        let packet = MeshPacket::new(
            PacketType::Paid,
            [1u8; 32],
            [2u8; 32],
            vec![1, 2, 3, 4],
        );
        
        let serialized = serialize_mesh_packet(&packet).unwrap();
        assert!(is_mesh_packet(&serialized));
        
        let deserialized = deserialize_mesh_packet(&serialized).unwrap();
        assert_eq!(packet.source, deserialized.source);
        assert_eq!(packet.destination, deserialized.destination);
    }
}

