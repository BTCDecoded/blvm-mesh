//! Mining pool coordination logic

use blvm_mesh::MeshClient;
use blvm_mesh::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Pool member information
#[derive(Debug, Clone)]
pub struct PoolMember {
    pub node_id: NodeId,
    pub hash_rate: u64, // Hash rate in H/s
    pub last_seen: u64, // Timestamp
}

/// Block template message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub version: u32,
    pub previous_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u32,
    pub transactions: Vec<Vec<u8>>, // Serialized transactions
}

/// Pool message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolMessage {
    /// Block template update
    BlockTemplate(BlockTemplate),
    /// Member join request
    MemberJoin { node_id: NodeId, hash_rate: u64 },
    /// Member leave notification
    MemberLeave { node_id: NodeId },
    /// Share submission
    ShareSubmit { node_id: NodeId, share_data: Vec<u8> },
}

/// Pool coordinator for managing pool members and block template distribution
pub struct PoolCoordinator {
    mesh_client: MeshClient,
    caller_module_id: String,
    members: Arc<RwLock<HashMap<NodeId, PoolMember>>>,
    current_template: Arc<RwLock<Option<BlockTemplate>>>,
}

impl PoolCoordinator {
    /// Create a new pool coordinator
    pub fn new(mesh_client: MeshClient, caller_module_id: String) -> Self {
        Self {
            mesh_client,
            caller_module_id,
            members: Arc::new(RwLock::new(HashMap::new())),
            current_template: Arc::new(RwLock::new(None)),
        }
    }

    /// Add a pool member
    pub async fn add_member(&self, member: PoolMember) {
        info!("Adding pool member: {:x?}", &member.node_id[..8]);
        let mut members = self.members.write().await;
        members.insert(member.node_id, member);
    }

    /// Remove a pool member
    pub async fn remove_member(&self, node_id: &NodeId) {
        let mut members = self.members.write().await;
        if members.remove(node_id).is_some() {
            info!("Removed pool member: {:x?}", &node_id[..8]);
        }
    }

    /// Update block template
    pub async fn update_template(&self, template: BlockTemplate) -> Result<(), String> {
        {
            let mut current = self.current_template.write().await;
            *current = Some(template.clone());
        }

        // Broadcast to all members
        let serialized = bincode::serialize(&PoolMessage::BlockTemplate(template.clone()))
            .map_err(|e| format!("Failed to serialize template: {}", e))?;

        self.broadcast_to_members(serialized).await?;
        Ok(())
    }

    /// Handle incoming pool message
    pub async fn handle_message(
        &self,
        sender: NodeId,
        message: PoolMessage,
    ) -> Result<(), String> {
        match message {
            PoolMessage::MemberJoin { node_id, hash_rate } => {
                let member = PoolMember {
                    node_id,
                    hash_rate,
                    last_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                self.add_member(member).await;
            }
            PoolMessage::MemberLeave { node_id } => {
                self.remove_member(&node_id).await;
            }
            PoolMessage::BlockTemplate(template) => {
                // Received template update (from pool operator)
                let mut current = self.current_template.write().await;
                *current = Some(template);
                info!("Block template updated");
            }
            PoolMessage::ShareSubmit { node_id, share_data } => {
                debug!("Share submitted from {:x?}: {} bytes", &node_id[..8], share_data.len());
                // TODO: Validate and process share
            }
        }
        Ok(())
    }

    /// Broadcast message to all pool members
    async fn broadcast_to_members(&self, message: Vec<u8>) -> Result<usize, String> {
        let members = self.members.read().await;
        info!("Broadcasting to {} pool members", members.len());

        let mut success_count = 0;
        for (node_id, _member) in members.iter() {
            // Send via mesh (no payment proof for internal pool coordination)
            match self
                .mesh_client
                .send_packet(
                    &self.caller_module_id,
                    *node_id,
                    message.clone(),
                    None, // No payment for pool coordination
                    Some("mining-pool-v1".to_string()),
                )
                .await
            {
                Ok(response) => {
                    if response.success {
                        success_count += 1;
                        tracing::debug!(
                            "Block template sent to {:x?}: {} hops",
                            &node_id[..8],
                            response.route_length
                        );
                    } else {
                        tracing::debug!(
                            "Failed to send to {:x?}: {}",
                            &node_id[..8],
                            response.error.unwrap_or_else(|| "Unknown".to_string())
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!("Error sending to {:x?}: {}", &node_id[..8], e);
                }
            }
        }

        info!(
            "Broadcast complete: {}/{} successful",
            success_count,
            members.len()
        );

        Ok(success_count)
    }

    /// Broadcast block template to all pool members
    ///
    /// This uses mesh routing for guaranteed delivery to all members.
    /// Since this is internal pool coordination, payment is not required.
    pub async fn broadcast_block_template(
        &self,
        template: BlockTemplate,
    ) -> Result<usize, String> {
        let serialized = bincode::serialize(&PoolMessage::BlockTemplate(template))
            .map_err(|e| format!("Failed to serialize template: {}", e))?;
        self.broadcast_to_members(serialized).await
    }

    /// Get current block template
    pub async fn get_template(&self) -> Option<BlockTemplate> {
        let current = self.current_template.read().await;
        current.clone()
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> PoolStats {
        let members = self.members.read().await;
        let total_hash_rate: u64 = members.values().map(|m| m.hash_rate).sum();
        
        PoolStats {
            member_count: members.len(),
            total_hash_rate,
        }
    }

    /// List pool members (miners)
    pub async fn list_members(&self) -> Vec<PoolMember> {
        let members = self.members.read().await;
        members.values().cloned().collect()
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub member_count: usize,
    pub total_hash_rate: u64,
}

