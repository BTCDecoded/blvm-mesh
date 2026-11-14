//! Messaging service for blvm-messaging

use blvm_mesh::{MeshClient, NodeId, PaymentProof};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub from: NodeId,
    pub to: NodeId,
    pub timestamp: u64,
    pub content: Vec<u8>,     // Encrypted message content
    pub message_id: [u8; 32], // Unique message ID
}

/// Messaging service
pub struct MessagingService {
    mesh_client: MeshClient,
    caller_module_id: String,
    node_id: NodeId, // Cache node ID
    inbox: Arc<RwLock<HashMap<[u8; 32], Message>>>,
    outbox: Arc<RwLock<HashMap<[u8; 32], Message>>>, // message_id -> Message (sent)
}

impl MessagingService {
    /// Create a new messaging service
    pub async fn new(mesh_client: MeshClient, caller_module_id: String) -> Result<Self, String> {
        // Get node ID from mesh
        let node_id = mesh_client
            .get_node_id()
            .await
            .map_err(|e| format!("Failed to get node ID: {e}"))?;

        Ok(Self {
            mesh_client,
            caller_module_id,
            node_id,
            inbox: Arc::new(RwLock::new(HashMap::new())),
            outbox: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn local_node_id(&self) -> NodeId {
        self.node_id
    }

    /// Send a direct message
    ///
    /// This encrypts the message end-to-end and sends via mesh with payment.
    pub async fn send_message(
        &self,
        recipient: NodeId,
        content: Vec<u8>,
        payment_proof: Option<PaymentProof>,
    ) -> Result<[u8; 32], String> {
        // Generate message ID
        let message_id = self.generate_message_id(&content);

        // Use cached node ID
        let sender = self.node_id;

        // Create message
        let message = Message {
            from: sender,
            to: recipient,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            content: self.encrypt_message(&content, recipient).await?,
            message_id,
        };

        // Serialize message
        let serialized = bincode::serialize(&message)
            .map_err(|e| format!("Failed to serialize message: {e}"))?;

        // Send via mesh
        let serialized_len = serialized.len();
        let response = self
            .mesh_client
            .send_packet(
                &self.caller_module_id,
                recipient,
                serialized,
                payment_proof,
                Some("messaging-v1".to_string()),
            )
            .await
            .map_err(|e| format!("Failed to send message via mesh: {e}"))?;

        if !response.success {
            return Err(response
                .error
                .unwrap_or_else(|| "Unknown error".to_string()));
        }

        // Store in outbox
        {
            let mut outbox = self.outbox.write().await;
            outbox.insert(message_id, message);
        }

        info!(
            "Message sent: {} bytes, cost: {} sats, route: {} hops",
            serialized_len, response.estimated_cost_sats, response.route_length
        );

        Ok(message_id)
    }

    /// Handle incoming message
    pub async fn handle_incoming_message(
        &self,
        packet_payload: Vec<u8>,
        my_node_id: NodeId,
    ) -> Result<Option<Vec<u8>>, String> {
        debug!("Handling incoming message: {} bytes", packet_payload.len());

        if packet_payload.len() > blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(format!(
                "Message too large: {} bytes (max: {} bytes)",
                packet_payload.len(),
                blvm_mesh::packet::MAX_BINCODE_PAYLOAD_SIZE
            ));
        }
        // Deserialize message
        let message: Message = bincode::deserialize(&packet_payload)
            .map_err(|e| format!("Failed to deserialize message: {e}"))?;

        // Check if message is for us
        if message.to != my_node_id {
            // Not for us, but we might be forwarding
            return Ok(None);
        }

        // Decrypt message
        let decrypted = self.decrypt_message(&message.content, message.from).await?;

        // Store in inbox
        {
            let mut inbox = self.inbox.write().await;
            inbox.insert(message.message_id, message.clone());
        }

        info!(
            "Message received from {:x?}: {} bytes",
            &message.from[..8],
            decrypted.len()
        );

        Ok(Some(decrypted))
    }

    /// Get inbox messages
    pub async fn get_inbox(&self) -> Vec<Message> {
        let inbox = self.inbox.read().await;
        inbox.values().cloned().collect()
    }

    /// Get outbox messages
    pub async fn get_outbox(&self) -> Vec<Message> {
        let outbox = self.outbox.read().await;
        outbox.values().cloned().collect()
    }

    /// List conversations (unique peers from inbox + outbox with last activity)
    pub async fn list_conversations(&self) -> Vec<(NodeId, u64, String)> {
        let mut peers: std::collections::HashMap<[u8; 32], (u64, bool, bool)> =
            std::collections::HashMap::new();
        {
            let inbox = self.inbox.read().await;
            for m in inbox.values() {
                let entry = peers.entry(m.from).or_insert((0, false, false));
                entry.0 = entry.0.max(m.timestamp);
                entry.1 = true; // has_in
            }
        }
        {
            let outbox = self.outbox.read().await;
            for m in outbox.values() {
                let entry = peers.entry(m.to).or_insert((0, false, false));
                entry.0 = entry.0.max(m.timestamp);
                entry.2 = true; // has_out
            }
        }
        peers
            .into_iter()
            .map(|(id, (ts, has_in, has_out))| {
                let dir = match (has_in, has_out) {
                    (true, true) => "both",
                    (true, false) => "in",
                    (false, true) => "out",
                    _ => "-",
                };
                (id, ts, dir.to_string())
            })
            .collect()
    }

    /// Encrypt message for `recipient`. The key comes from the mesh identity.
    async fn encrypt_message(&self, content: &[u8], recipient: NodeId) -> Result<Vec<u8>, String> {
        let key = self
            .mesh_client
            .messaging_key(recipient)
            .await
            .map_err(|e| format!("Failed to derive messaging key: {e}"))?;
        seal(&key, content)
    }

    /// Decrypt a message that claims to be from `sender`.
    async fn decrypt_message(&self, encrypted: &[u8], sender: NodeId) -> Result<Vec<u8>, String> {
        let key = self
            .mesh_client
            .messaging_key(sender)
            .await
            .map_err(|e| format!("Failed to derive messaging key: {e}"))?;
        open(&key, encrypted)
    }

    /// Generate unique message ID
    fn generate_message_id(&self, content: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content);
        hasher.update(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_le_bytes(),
        );
        let hash = hasher.finalize();
        let mut message_id = [0u8; 32];
        message_id.copy_from_slice(&hash);
        message_id
    }
}

fn seal(key: &[u8; 32], content: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, content)
        .map_err(|e| format!("Encryption failed: {e}"))?;
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(nonce.as_slice());
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

fn open(key: &[u8; 32], encrypted: &[u8]) -> Result<Vec<u8>, String> {
    if encrypted.len() < 12 {
        return Err("Encrypted message too short".to_string());
    }
    let nonce = Nonce::from_slice(&encrypted[..12]);
    let ciphertext = &encrypted[12..];
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use blvm_mesh::MeshIdentity;
    use sha2::{Digest, Sha256};

    #[test]
    fn a_third_party_cannot_open_a_direct_message() {
        let alice = MeshIdentity::from_seed(1);
        let bob = MeshIdentity::from_seed(2);
        let carol = MeshIdentity::from_seed(3);
        let alice_bob = alice.messaging_key(&bob.node_id()).unwrap();
        let bob_alice = bob.messaging_key(&alice.node_id()).unwrap();
        assert_eq!(alice_bob, bob_alice);
        let carol_bob = carol.messaging_key(&bob.node_id()).unwrap();
        assert_ne!(alice_bob, carol_bob);

        let message = b"Hello, encrypted world!";
        let sealed = seal(&alice_bob, message).unwrap();
        assert_eq!(open(&bob_alice, &sealed).unwrap(), message);
        assert!(open(&carol_bob, &sealed).is_err());

        let mut hasher = Sha256::new();
        hasher.update(b"e2e_messaging_key_v1");
        let (left, right) = if alice.node_id() < bob.node_id() {
            (alice.node_id(), bob.node_id())
        } else {
            (bob.node_id(), alice.node_id())
        };
        hasher.update(left);
        hasher.update(right);
        assert_ne!(alice_bob.as_slice(), &hasher.finalize()[..]);
    }
}
