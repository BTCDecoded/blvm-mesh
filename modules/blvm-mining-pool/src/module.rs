//! Mining-pool module: unified CLI via #[module] macro.

use blvm_node::module::ipc::protocol::EventMessage;
use blvm_node::module::traits::EventType;
use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;
use std::sync::Arc;
use tracing::info;

use crate::pool::PoolCoordinator;

#[derive(Clone)]
pub struct MiningPoolModule {
    pub pool_coordinator: Arc<PoolCoordinator>,
}

#[module]
impl MiningPoolModule {
    #[on_event(PeerConnected, PeerDisconnected, NewBlock, MempoolTransactionAdded)]
    async fn on_pool_event(
        &self,
        event: &EventMessage,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let _ = ctx;
        match event.event_type {
            EventType::PeerConnected => info!("Peer connected (mining-pool)"),
            EventType::PeerDisconnected => info!("Peer disconnected (mining-pool)"),
            EventType::NewBlock => info!("New block — pool may refresh block templates"),
            EventType::MempoolTransactionAdded => {
                info!("Mempool transaction added — templates may need update");
            }
            _ => {}
        }
        Ok(())
    }

    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        Ok("blvm-mining-pool module\nProtocol: mining-pool-v1\nRunning: true".into())
    }

    #[command]
    fn pool_status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let coordinator = Arc::clone(&self.pool_coordinator);
        run_async(async move {
            let stats = coordinator.get_stats().await;
            let has_template = coordinator.get_template().await.is_some();
            Ok::<_, String>(format!(
                "Pool status:\n  Members: {}\n  Total hash rate: {} H/s\n  Template: {}",
                stats.member_count,
                stats.total_hash_rate,
                if has_template { "yes" } else { "no" }
            ))
        })
    }

    #[command]
    fn list_miners(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let coordinator = Arc::clone(&self.pool_coordinator);
        run_async(async move {
            let members = coordinator.list_members().await;
            let mut out = format!("Pool miners ({}):\n", members.len());
            for (i, m) in members.iter().enumerate() {
                out.push_str(&format!(
                    "  {}. node_id={:x?} hash_rate={} H/s last_seen={}\n",
                    i + 1,
                    &m.node_id[..8.min(m.node_id.len())],
                    m.hash_rate,
                    m.last_seen
                ));
            }
            Ok::<_, String>(out)
        })
    }
}
