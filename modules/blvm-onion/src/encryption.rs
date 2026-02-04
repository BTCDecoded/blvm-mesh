//! Onion encryption/decryption for blvm-onion
//!
//! Implements layered encryption where each hop in the route adds/removes one layer.

use blvm_mesh::NodeId;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Onion-encrypted message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnionMessage {
    /// Encrypted payload (layers of encryption)
    pub encrypted_payload: Vec<u8>,
    /// Route information (for debugging/logging, not in production)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_hint: Option<Vec<NodeId>>,
}

/// Onion layer structure (encrypted for each hop)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnionLayer {
    /// Next hop node ID
    next_hop: Option<NodeId>,
    /// Encrypted inner payload
    payload: Vec<u8>,
}

/// Encrypted layer with nonce (what gets sent over the wire)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedLayer {
    /// Nonce used for encryption
    nonce: [u8; 12],
    /// Encrypted layer data
    ciphertext: Vec<u8>,
}

/// Onion encryption/decryption handler
pub struct OnionEncryption {
    // Ephemeral keys for each hop (in production, would use public keys from route)
    // For now, we'll derive keys from node IDs (simplified)
}

impl OnionEncryption {
    /// Create a new onion encryption handler
    pub fn new() -> Self {
        Self {}
    }

    /// Derive encryption key from node ID (simplified - in production use actual public keys)
    fn derive_key(node_id: &NodeId) -> Key {
        // In production, this would use the node's public key
        // For now, derive a deterministic key from node ID
        let mut hasher = Sha256::new();
        hasher.update(b"onion_key_derivation_v1");
        hasher.update(node_id);
        let hash = hasher.finalize();
        *Key::from_slice(&hash[..32])
    }

    /// Encrypt a message with onion layers
    ///
    /// For each hop in the route (in reverse order), encrypt the message
    /// so that each node can only decrypt one layer.
    pub fn encrypt_onion(
        &self,
        message: Vec<u8>,
        route: &[NodeId],
    ) -> Result<OnionMessage, String> {
        if route.len() < 2 {
            return Err("Route must have at least 2 nodes".to_string());
        }

        debug!(
            "Encrypting message with {} onion layers (route length: {})",
            route.len() - 1,
            route.len()
        );

        // Start with the final message
        let mut payload = message;

        // Encrypt in reverse order (destination first, then work backwards)
        for i in (0..route.len()).rev() {
            let node_id = route[i];
            let key = Self::derive_key(&node_id);

            // Determine next hop
            let next_hop = if i + 1 < route.len() {
                Some(route[i + 1])
            } else {
                None // Final destination
            };

            // Create layer
            let layer = OnionLayer {
                next_hop,
                payload: payload.clone(),
            };

            // Serialize layer
            let layer_bytes = bincode::serialize(&layer)
                .map_err(|e| format!("Failed to serialize layer: {}", e))?;

            // Generate nonce for this layer
            let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

            // Encrypt layer
            let cipher = ChaCha20Poly1305::new(&key);
            let ciphertext = cipher
                .encrypt(&nonce, layer_bytes.as_ref())
                .map_err(|e| format!("Encryption failed: {}", e))?;

            // Package nonce with ciphertext
            let mut nonce_bytes = [0u8; 12];
            nonce_bytes.copy_from_slice(nonce.as_slice());
            let encrypted_layer = EncryptedLayer {
                nonce: nonce_bytes,
                ciphertext,
            };

            // Serialize encrypted layer for next encryption
            payload = bincode::serialize(&encrypted_layer)
                .map_err(|e| format!("Failed to serialize encrypted layer: {}", e))?;
        }

        Ok(OnionMessage {
            encrypted_payload: payload,
            route_hint: None, // Don't leak route info in production
        })
    }

    /// Decrypt one layer of onion encryption
    ///
    /// Each node in the route decrypts one layer to reveal:
    /// - The next hop (if not the destination)
    /// - The inner encrypted message
    ///
    /// Returns (next_hop, inner_message) or (None, final_message) if destination
    pub fn decrypt_layer(
        &self,
        encrypted: &OnionMessage,
        my_node_id: NodeId,
    ) -> Result<(Option<NodeId>, Vec<u8>), String> {
        debug!("Decrypting onion layer for node {:x?}", &my_node_id[..8]);

        // Derive key for this node
        let key = Self::derive_key(&my_node_id);
        let cipher = ChaCha20Poly1305::new(&key);

        // Deserialize encrypted layer (contains nonce + ciphertext)
        let encrypted_layer: EncryptedLayer = bincode::deserialize(&encrypted.encrypted_payload)
            .map_err(|e| format!("Failed to deserialize encrypted layer: {}", e))?;

        // Decrypt with the nonce
        let nonce = Nonce::from_slice(&encrypted_layer.nonce);
        let decrypted_bytes = cipher
            .decrypt(nonce, encrypted_layer.ciphertext.as_ref())
            .map_err(|e| {
                warn!("Decryption failed: {} - may not be for this node", e);
                format!("Decryption failed: {}", e)
            })?;

        // Deserialize layer
        let layer: OnionLayer = bincode::deserialize(&decrypted_bytes)
            .map_err(|e| format!("Failed to deserialize layer: {}", e))?;

        Ok((layer.next_hop, layer.payload))
    }
}

impl Default for OnionEncryption {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node_id(seed: u8) -> NodeId {
        let mut id = [0u8; 32];
        id[0] = seed;
        id
    }

    #[test]
    fn test_onion_encryption_simple_route() {
        let encryption = OnionEncryption::new();
        
        let node_a = create_test_node_id(1);
        let node_b = create_test_node_id(2);
        let route = vec![node_a, node_b];
        
        let message = b"Hello, onion routing!".to_vec();
        
        // Encrypt
        let encrypted = encryption.encrypt_onion(message.clone(), &route)
            .expect("Encryption should succeed");
        
        // Node A decrypts first layer
        let (next_hop, inner_payload) = encryption.decrypt_layer(&encrypted, node_a)
            .expect("Node A should decrypt successfully");
        
        assert_eq!(next_hop, Some(node_b), "Next hop should be node B");
        
        // Node B decrypts final layer
        // Create a new OnionMessage with the inner payload
        let inner_message = OnionMessage {
            encrypted_payload: inner_payload,
            route_hint: None,
        };
        
        let (final_hop, final_message) = encryption.decrypt_layer(&inner_message, node_b)
            .expect("Node B should decrypt successfully");
        
        assert_eq!(final_hop, None, "Node B should be destination");
        assert_eq!(final_message, message, "Final message should match original");
    }

    #[test]
    fn test_onion_encryption_three_hop_route() {
        let encryption = OnionEncryption::new();
        
        let node_a = create_test_node_id(1);
        let node_b = create_test_node_id(2);
        let node_c = create_test_node_id(3);
        let route = vec![node_a, node_b, node_c];
        
        let message = b"Three-hop onion route test".to_vec();
        
        // Encrypt
        let encrypted = encryption.encrypt_onion(message.clone(), &route)
            .expect("Encryption should succeed");
        
        // Node A decrypts first layer
        let (next_hop_a, payload_a) = encryption.decrypt_layer(&encrypted, node_a)
            .expect("Node A should decrypt");
        assert_eq!(next_hop_a, Some(node_b));
        
        // Node B decrypts second layer
        let inner_message_b = OnionMessage {
            encrypted_payload: payload_a,
            route_hint: None,
        };
        let (next_hop_b, payload_b) = encryption.decrypt_layer(&inner_message_b, node_b)
            .expect("Node B should decrypt");
        assert_eq!(next_hop_b, Some(node_c));
        
        // Node C decrypts final layer
        let inner_message_c = OnionMessage {
            encrypted_payload: payload_b,
            route_hint: None,
        };
        let (final_hop, final_message) = encryption.decrypt_layer(&inner_message_c, node_c)
            .expect("Node C should decrypt");
        
        assert_eq!(final_hop, None);
        assert_eq!(final_message, message);
    }

    #[test]
    fn test_onion_encryption_wrong_node_fails() {
        let encryption = OnionEncryption::new();
        
        let node_a = create_test_node_id(1);
        let node_b = create_test_node_id(2);
        let node_c = create_test_node_id(3); // Not in route
        let route = vec![node_a, node_b];
        
        let message = b"Test message".to_vec();
        
        let encrypted = encryption.encrypt_onion(message, &route)
            .expect("Encryption should succeed");
        
        // Node C tries to decrypt (should fail)
        let result = encryption.decrypt_layer(&encrypted, node_c);
        assert!(result.is_err(), "Node C should not be able to decrypt");
    }

    #[test]
    fn test_onion_encryption_route_too_short() {
        let encryption = OnionEncryption::new();
        
        let node_a = create_test_node_id(1);
        let route = vec![node_a]; // Only one node
        
        let message = b"Test".to_vec();
        
        let result = encryption.encrypt_onion(message, &route);
        assert!(result.is_err(), "Route with < 2 nodes should fail");
    }
}
