//! Tests for blvm-mining-pool

#[cfg(test)]
mod tests {
    use blvm_mesh::NodeId;
    use blvm_mining_pool::pool::{BlockTemplate, PoolMessage};

    fn create_test_node_id(seed: u8) -> NodeId {
        let mut id = [0u8; 32];
        id[0] = seed;
        id
    }

    #[test]
    fn test_block_template_serialization() {
        let template = BlockTemplate {
            version: 0x20000000,
            previous_block_hash: [0u8; 32],
            merkle_root: [1u8; 32],
            timestamp: 1234567890,
            bits: 0x1d00ffff,
            nonce: 0,
            transactions: vec![vec![1, 2, 3]],
        };

        let serialized = bincode::serialize(&PoolMessage::BlockTemplate(template.clone()))
            .expect("Serialization should succeed");
        
        let deserialized: PoolMessage = bincode::deserialize(&serialized)
            .expect("Deserialization should succeed");

        match deserialized {
            PoolMessage::BlockTemplate(deser_template) => {
                assert_eq!(deser_template.version, template.version);
                assert_eq!(deser_template.timestamp, template.timestamp);
            }
            _ => panic!("Expected BlockTemplate message"),
        }
    }

    #[test]
    fn test_pool_message_types() {
        let node_id = create_test_node_id(1);

        // Test MemberJoin
        let join_msg = PoolMessage::MemberJoin {
            node_id,
            hash_rate: 1000,
        };
        let serialized = bincode::serialize(&join_msg).unwrap();
        let deserialized: PoolMessage = bincode::deserialize(&serialized).unwrap();
        match deserialized {
            PoolMessage::MemberJoin { node_id: id, hash_rate } => {
                assert_eq!(id, node_id);
                assert_eq!(hash_rate, 1000);
            }
            _ => panic!("Expected MemberJoin"),
        }

        // Test MemberLeave
        let leave_msg = PoolMessage::MemberLeave { node_id };
        let serialized = bincode::serialize(&leave_msg).unwrap();
        let deserialized: PoolMessage = bincode::deserialize(&serialized).unwrap();
        match deserialized {
            PoolMessage::MemberLeave { node_id: id } => assert_eq!(id, node_id),
            _ => panic!("Expected MemberLeave"),
        }
    }
}

