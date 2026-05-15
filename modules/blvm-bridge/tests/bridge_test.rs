//! Tests for blvm-bridge

#[cfg(test)]
mod tests {
    use blvm_bridge::bridge::{BridgePacket, BridgePacketType};
    use blvm_mesh::NodeId;

    fn create_test_node_id(seed: u8) -> NodeId {
        let mut id = [0u8; 32];
        id[0] = seed;
        id
    }

    #[test]
    fn test_bridge_packet_serialization() {
        let source = create_test_node_id(1);
        let dest = create_test_node_id(2);

        let packet = BridgePacket {
            source,
            destination: dest,
            data: b"test data".to_vec(),
            packet_type: BridgePacketType::Block,
            timestamp: 1234567890,
        };

        let serialized = bincode::serialize(&packet).expect("Serialization should succeed");

        let deserialized: BridgePacket =
            bincode::deserialize(&serialized).expect("Deserialization should succeed");

        assert_eq!(deserialized.source, source);
        assert_eq!(deserialized.destination, dest);
        assert_eq!(deserialized.data, b"test data");
        assert!(matches!(deserialized.packet_type, BridgePacketType::Block));
    }

    #[test]
    fn test_bridge_packet_types() {
        let source = create_test_node_id(1);
        let dest = create_test_node_id(2);

        let packet_types = vec![
            BridgePacketType::Block,
            BridgePacketType::Transaction,
            BridgePacketType::MeshRelay,
            BridgePacketType::Data,
        ];

        for packet_type in packet_types {
            let packet = BridgePacket {
                source,
                destination: dest,
                data: vec![],
                packet_type: packet_type.clone(),
                timestamp: 0,
            };

            let serialized = bincode::serialize(&packet).unwrap();
            let deserialized: BridgePacket = bincode::deserialize(&serialized).unwrap();

            match (packet_type, deserialized.packet_type) {
                (BridgePacketType::Block, BridgePacketType::Block) => {}
                (BridgePacketType::Transaction, BridgePacketType::Transaction) => {}
                (BridgePacketType::MeshRelay, BridgePacketType::MeshRelay) => {}
                (BridgePacketType::Data, BridgePacketType::Data) => {}
                _ => panic!("Packet type mismatch"),
            }
        }
    }
}
