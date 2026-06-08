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
    use std::io::Write;

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
}
