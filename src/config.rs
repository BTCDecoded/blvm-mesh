//! Mesh module configuration.
//!
//! Loaded from config.toml in module data dir. Node overrides via [modules.mesh] and
//! MODULE_CONFIG_* env vars.

use std::path::Path;

use blvm_sdk_macros::config;
use serde::{Deserialize, Serialize};

fn default_mode() -> String {
    "payment_gated".to_string()
}

/// Static peer entry with optional explicit node id (D-5).
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct MeshPeerEntry {
    pub address: String,
    #[serde(default)]
    pub node_id_hex: Option<String>,
}

/// Mesh module configuration.
///
/// Config file: `config.toml` in module data dir.
/// Node override: `[modules.mesh]` or `[modules.blvm-mesh]` in node config.
/// Env override: `MODULE_CONFIG_MODE`.
/// Loading the module turns it on. Peers are `peers` entries (`address`, optional `node_id_hex`).
#[config(name = "mesh")]
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Operating mode: "bitcoin_only", "payment_gated", or "open"
    #[serde(default = "default_mode")]
    #[config_env]
    pub mode: String,

    /// Ed25519 signing seed (hex) injected by co-located apps for unified mesh identity.
    #[serde(default)]
    pub identity_seed_hex: Option<String>,

    /// Direct peers. `node_id_hex` is optional.
    #[serde(default)]
    pub peers: Vec<MeshPeerEntry>,
    /// Max direct peers.
    #[serde(default = "default_max_peers")]
    pub max_peers: u32,
    /// Ingress packets per peer per minute (0 = disabled).
    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
}

fn default_rate_limit_per_minute() -> u32 {
    120
}

fn default_max_peers() -> u32 {
    50
}

const RETIRED_KEYS: &[&str] = &["enabled", "peer_list", "bootstrap_nodes"];
const RETIRED_ENVS: &[&str] = &[
    "MODULE_CONFIG_ENABLED",
    "MODULE_CONFIG_PEER_LIST",
    "MODULE_CONFIG_BOOTSTRAP_NODES",
];

impl MeshConfig {
    /// Fail when a file or node override still uses a retired peer or enable key.
    pub fn reject_retired(path: &Path) -> Result<(), String> {
        for key in RETIRED_ENVS {
            if std::env::var(key).is_ok() {
                return Err(retired_peer_message());
            }
        }
        if !path.exists() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let value: toml::Value = toml::from_str(&text).map_err(|error| error.to_string())?;
        let Some(table) = value.as_table() else {
            return Ok(());
        };
        if RETIRED_KEYS.iter().any(|key| table.contains_key(*key)) {
            return Err(retired_peer_message());
        }
        Ok(())
    }

    /// Convert to ModuleContext config map for manager compatibility.
    pub fn to_context_map(&self) -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("mesh.mode".to_string(), self.mode.clone());
        m
    }
}

fn retired_peer_message() -> String {
    "enabled, peer_list, and bootstrap_nodes are not blvm-mesh settings. Loading the module turns it on. List peers under peers".to_string()
}

blvm_sdk::impl_module_config!(MeshConfig);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retired_peer_key_fails_and_peers_alone_does_not() {
        let dir = std::env::temp_dir().join(format!("mesh-peers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "peer_list = [\"127.0.0.1:8333\"]\n").unwrap();
        let error = MeshConfig::reject_retired(&path).unwrap_err();
        assert!(error.contains("peers"), "{error}");
        std::fs::write(&path, "[[peers]]\naddress = \"127.0.0.1:8333\"\n").unwrap();
        assert!(MeshConfig::reject_retired(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
