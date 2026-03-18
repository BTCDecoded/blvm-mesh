//! Mesh module configuration.
//!
//! Loaded from config.toml in module data dir. Node overrides via [modules.mesh] and
//! MODULE_CONFIG_* env vars.

use blvm_sdk_macros::config;
use serde::{Deserialize, Serialize};

fn default_mode() -> String {
    "payment_gated".to_string()
}

/// Mesh module configuration.
///
/// Config file: `config.toml` in module data dir.
/// Node override: `[modules.mesh]` or `[modules.blvm-mesh]` in node config.
/// Env override: `MODULE_CONFIG_ENABLED`, `MODULE_CONFIG_MODE`.
#[config(name = "mesh")]
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Enable mesh networking (default: false)
    #[serde(default)]
    #[config_env]
    pub enabled: bool,

    /// Operating mode: "bitcoin_only", "payment_gated", or "open"
    #[serde(default = "default_mode")]
    #[config_env]
    pub mode: String,

    /// Static peer list (comma-separated addresses).
    #[serde(default)]
    pub peer_list: Vec<String>,
    /// Bootstrap nodes for discovery.
    #[serde(default)]
    pub bootstrap_nodes: Vec<String>,
    /// Max direct peers.
    #[serde(default = "default_max_peers")]
    pub max_peers: u32,
}

fn default_max_peers() -> u32 {
    50
}

impl MeshConfig {
    /// Convert to ModuleContext config map for manager compatibility.
    pub fn to_context_map(&self) -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "mesh.enabled".to_string(),
            self.enabled.to_string(),
        );
        m.insert("mesh.mode".to_string(), self.mode.clone());
        m
    }
}

blvm_sdk::impl_module_config!(MeshConfig);
