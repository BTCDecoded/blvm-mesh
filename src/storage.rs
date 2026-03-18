//! Storage migrations for blvm-mesh.
//!
//! v1: Migrate mesh_config:node_id from legacy "items" tree to "mesh_state".

use blvm_sdk::module::{MigrationContext, MigrationUp};

const MESH_STATE_TREE: &str = "mesh_state";
const STORAGE_KEY: &[u8] = b"mesh_config:node_id";

pub fn up_v1(ctx: &MigrationContext) -> anyhow::Result<()> {
    let items_tree = ctx.open_tree("items")?;
    if let Some(data) = items_tree.get(STORAGE_KEY)? {
        if data.len() == 32 {
            let mesh_state = ctx.open_tree(MESH_STATE_TREE)?;
            mesh_state.insert(STORAGE_KEY, &data)?;
        }
    }
    Ok(())
}

pub const MIGRATIONS: &[(u32, MigrationUp)] = &[(1, up_v1)];
