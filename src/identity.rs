//! Mesh identity — Ed25519 node id (D-1) and hello handshake.

use crate::error::MeshError;
use crate::routing::NodeId;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::path::Path;

const MESH_STATE_TREE: &str = "mesh_state";
const IDENTITY_SECRET_KEY: &[u8] = b"mesh_config:identity_secret";

/// Control-plane protocol id for hello frames.
pub const PROTOCOL_HELLO: &str = "mesh-hello-v1";

/// Control-plane protocol id for route discovery.
pub const PROTOCOL_DISCOVERY: &str = "mesh-discovery-v1";

/// Ed25519 mesh identity; `NodeId` is the 32-byte verifying key.
pub struct MeshIdentity {
    signing_key: SigningKey,
}

impl MeshIdentity {
    /// Load from config seed, module DB, or create a new Ed25519 keypair.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, MeshError> {
        if let Ok(seed) = load_identity_seed_from_config(data_dir) {
            persist_identity_seed(data_dir, &seed);
            return Ok(Self {
                signing_key: SigningKey::from_bytes(&seed),
            });
        }

        if let Ok(db) = blvm_sdk::module::ModuleDb::open(data_dir) {
            if let Ok(tree) = db.tree(MESH_STATE_TREE) {
                if let Ok(Some(stored)) = tree.get(IDENTITY_SECRET_KEY) {
                    if stored.len() == 32 {
                        let mut seed = [0u8; 32];
                        seed.copy_from_slice(&stored);
                        let signing_key = SigningKey::from_bytes(&seed);
                        return Ok(Self { signing_key });
                    }
                }
            }
        }

        let signing_key = SigningKey::generate(&mut OsRng);
        persist_identity_seed(data_dir, &signing_key.to_bytes());
        Ok(Self { signing_key })
    }

    /// In-memory identity for tests.
    pub fn from_seed(seed: u8) -> Self {
        let bytes = [seed; 32];
        Self {
            signing_key: SigningKey::from_bytes(&bytes),
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Offer: our verifying key and a challenge the peer must sign.
    ///
    /// An offer does not prove the key. Only [`Self::verify_hello_proof`] binds a peer.
    pub fn hello_offer(&self, challenge: &[u8; 32]) -> Vec<u8> {
        let mut payload = self.node_id().to_vec();
        payload.extend_from_slice(challenge);
        payload
    }

    /// Proof: verifying key and a signature over the challenge this node issued.
    pub fn hello_proof(&self, challenge: &[u8; 32]) -> Vec<u8> {
        let mut payload = self.node_id().to_vec();
        payload.extend_from_slice(&self.sign_hello(challenge));
        payload
    }

    fn sign_hello(&self, challenge: &[u8; 32]) -> [u8; 64] {
        let signature = self
            .signing_key
            .sign(&hello_message(challenge, &self.node_id()));
        signature.to_bytes()
    }

    /// Accept a proof only for the challenge we stored for this socket.
    pub fn verify_hello_proof(payload: &[u8], challenge: &[u8; 32]) -> Result<NodeId, MeshError> {
        if payload.len() != 96 {
            return Err(MeshError::InvalidPacket(format!(
                "mesh hello proof must be 96 bytes, got {}",
                payload.len()
            )));
        }
        let mut id = [0u8; 32];
        id.copy_from_slice(&payload[..32]);
        let mut signature_bytes = [0u8; 64];
        signature_bytes.copy_from_slice(&payload[32..]);
        let verifying_key = VerifyingKey::from_bytes(&id).map_err(|e| {
            MeshError::InvalidPacket(format!("mesh hello key is not an Ed25519 key: {e}"))
        })?;
        let signature = Signature::from_bytes(&signature_bytes);
        verifying_key
            .verify(&hello_message(challenge, &id), &signature)
            .map_err(|_| MeshError::InvalidPacket("mesh hello signature rejected".to_string()))?;
        Ok(id)
    }
}

fn hello_message(challenge: &[u8; 32], node_id: &NodeId) -> Vec<u8> {
    let mut message = b"mesh-hello-v1".to_vec();
    message.extend_from_slice(challenge);
    message.extend_from_slice(node_id);
    message
}

fn load_identity_seed_from_config(data_dir: &Path) -> Result<[u8; 32], MeshError> {
    let config_path = data_dir.join("config.toml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| MeshError::InvalidPacket(format!("read {}: {e}", config_path.display())))?;
    let doc: toml::Value = toml::from_str(&content)
        .map_err(|e| MeshError::InvalidPacket(format!("parse {}: {e}", config_path.display())))?;
    let hex = doc
        .get("mesh")
        .and_then(|m| m.get("identity_seed_hex"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            MeshError::InvalidPacket("mesh.identity_seed_hex not set in config.toml".into())
        })?;
    let bytes = hex::decode(hex.trim())
        .map_err(|e| MeshError::InvalidPacket(format!("identity_seed_hex decode: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| MeshError::InvalidPacket("identity_seed_hex must be 32 bytes".into()))
}

fn persist_identity_seed(data_dir: &Path, seed: &[u8; 32]) {
    if let Ok(db) = blvm_sdk::module::ModuleDb::open(data_dir) {
        if let Ok(tree) = db.tree(MESH_STATE_TREE) {
            let _ = tree.insert(IDENTITY_SECRET_KEY, seed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_config_identity_seed_hex() {
        let dir = std::env::temp_dir().join(format!("blvm-mesh-id-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = [7u8; 32];
        std::fs::write(
            dir.join("config.toml"),
            format!("[mesh]\nidentity_seed_hex = \"{}\"\n", hex::encode(seed)),
        )
        .unwrap();
        let id = MeshIdentity::load_or_create(&dir).unwrap();
        assert_eq!(
            id.node_id(),
            SigningKey::from_bytes(&seed).verifying_key().to_bytes()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hello_proof_verifies_only_for_its_challenge() {
        let identity = MeshIdentity::from_seed(42);
        let challenge = [9u8; 32];
        let proof = identity.hello_proof(&challenge);
        assert_eq!(
            MeshIdentity::verify_hello_proof(&proof, &challenge).unwrap(),
            identity.node_id()
        );
        let wrong = [8u8; 32];
        assert!(MeshIdentity::verify_hello_proof(&proof, &wrong).is_err());
        let offer = identity.hello_offer(&challenge);
        assert_eq!(&offer[..32], identity.node_id().as_slice());
        assert!(MeshIdentity::verify_hello_proof(&offer, &challenge).is_err());
    }
}
