//! Integration tests for blvm-onion

#[cfg(test)]
mod tests {
    use blvm_mesh::NodeId;
    use blvm_onion::encryption::OnionEncryption;

    fn create_test_node_id(seed: u8) -> NodeId {
        let mut id = [0u8; 32];
        id[0] = seed;
        id
    }

    #[test]
    fn test_onion_route_encryption_decryption() {
        let encryption = OnionEncryption::new();
        
        let node_a = create_test_node_id(1);
        let node_b = create_test_node_id(2);
        let node_c = create_test_node_id(3);
        let route = vec![node_a, node_b, node_c];
        
        let message = b"Test message for onion routing".to_vec();
        
        // Encrypt
        let encrypted = encryption.encrypt_onion(message.clone(), &route)
            .expect("Encryption should succeed");
        
        // Simulate routing through nodes
        // Node A decrypts
        let (next_hop_a, payload_a) = encryption.decrypt_layer(&encrypted, node_a)
            .expect("Node A should decrypt");
        assert_eq!(next_hop_a, Some(node_b));
        
        // Node B decrypts
        let inner_b = blvm_onion::encryption::OnionMessage {
            encrypted_payload: payload_a,
            route_hint: None,
        };
        let (next_hop_b, payload_b) = encryption.decrypt_layer(&inner_b, node_b)
            .expect("Node B should decrypt");
        assert_eq!(next_hop_b, Some(node_c));
        
        // Node C decrypts (destination)
        let inner_c = blvm_onion::encryption::OnionMessage {
            encrypted_payload: payload_b,
            route_hint: None,
        };
        let (final_hop, final_message) = encryption.decrypt_layer(&inner_c, node_c)
            .expect("Node C should decrypt");
        
        assert_eq!(final_hop, None);
        assert_eq!(final_message, message);
    }
}

