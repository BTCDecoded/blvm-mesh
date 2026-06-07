//! Mesh identity — Ed25519 node id (D-1) and hello handshake.

use crate::error::MeshError;
use crate::routing::NodeId;
use ed25519_dalek::SigningKey;
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
    /// Load from module DB or create a new Ed25519 keypair.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, MeshError> {
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
        if let Ok(db) = blvm_sdk::module::ModuleDb::open(data_dir) {
            if let Ok(tree) = db.tree(MESH_STATE_TREE) {
                let _ = tree.insert(IDENTITY_SECRET_KEY, &signing_key.to_bytes());
            }
        }
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

    /// Hello wire payload: raw 32-byte Ed25519 verifying key.
    pub fn hello_payload(&self) -> Vec<u8> {
        self.node_id().to_vec()
    }

    pub fn parse_hello_payload(payload: &[u8]) -> Result<NodeId, MeshError> {
        if payload.len() != 32 {
            return Err(MeshError::InvalidPacket(format!(
                "mesh hello payload must be 32 bytes, got {}",
                payload.len()
            )));
        }
        let mut id = [0u8; 32];
        id.copy_from_slice(payload);
        Ok(id)
    }
}
