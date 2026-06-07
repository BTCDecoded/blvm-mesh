//! Shared test doubles for unit and integration tests.

use async_trait::async_trait;
use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::ipc::protocol::{EventPayload, FileMetadata};
use blvm_node::module::metrics::manager::Metric;
use blvm_node::module::process::monitor::ModuleHealth;
use blvm_node::module::timers::manager::{TaskCallback, TaskId, TimerCallback, TimerId};
use blvm_node::module::traits::{
    BlockServeDenylistSnapshot, ChainInfo, EventType, LightningInfo, MempoolSize, ModuleError,
    ModuleInfo, NetworkStats, NodeAPI, PaymentState, PeerInfo, SubmitBlockResult, SyncStatus,
    TxServeDenylistSnapshot,
};
use blvm_protocol::{Block, BlockHeader, Hash, OutPoint, Transaction, UTXO};
use std::collections::HashMap;
use std::sync::Arc;

type MeshDeliverFn = Arc<dyn Fn(String, Vec<u8>) + Send + Sync>;

/// In-memory mesh wire for multi-node integration tests.
pub struct WireHub {
    routes: std::sync::Mutex<HashMap<String, MeshDeliverFn>>,
}

impl Default for WireHub {
    fn default() -> Self {
        Self::new()
    }
}

impl WireHub {
    pub fn new() -> Self {
        Self {
            routes: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, addr: &str, handler: MeshDeliverFn) {
        self.routes
            .lock()
            .unwrap()
            .insert(addr.to_string(), handler);
    }

    pub fn deliver(&self, dest_addr: &str, from_addr: String, data: Vec<u8>) {
        if let Some(handler) = self.routes.lock().unwrap().get(dest_addr) {
            handler(from_addr, data);
        }
    }
}

type OutboxType = Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// Minimal `NodeAPI` stub for mesh unit tests.
pub struct TestNodeAPI {
    wire: Option<(Arc<WireHub>, String)>,
    /// Captured outbound mesh sends (peer_addr, bytes) when `wire` is unset.
    pub outbox: OutboxType,
}

impl Default for TestNodeAPI {
    fn default() -> Self {
        Self {
            wire: None,
            outbox: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl TestNodeAPI {
    pub fn with_wire(hub: Arc<WireHub>, local_addr: impl Into<String>) -> Self {
        Self {
            wire: Some((hub, local_addr.into())),
            outbox: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn take_outbox(&self) -> Vec<(String, Vec<u8>)> {
        self.outbox.lock().unwrap().drain(..).collect()
    }
}

#[async_trait]
impl NodeAPI for TestNodeAPI {
    async fn get_block(&self, _: &Hash) -> Result<Option<Block>, ModuleError> {
        Ok(None)
    }
    async fn get_block_header(&self, _: &Hash) -> Result<Option<BlockHeader>, ModuleError> {
        Ok(None)
    }
    async fn get_transaction(&self, _: &Hash) -> Result<Option<Transaction>, ModuleError> {
        Ok(None)
    }
    async fn has_transaction(&self, _: &Hash) -> Result<bool, ModuleError> {
        Ok(false)
    }
    async fn get_chain_tip(&self) -> Result<Hash, ModuleError> {
        Ok([0u8; 32])
    }
    async fn get_block_height(&self) -> Result<u64, ModuleError> {
        Ok(0)
    }
    async fn get_utxo(&self, _: &OutPoint) -> Result<Option<UTXO>, ModuleError> {
        Ok(None)
    }
    async fn subscribe_events(
        &self,
        _: Vec<EventType>,
    ) -> Result<tokio::sync::mpsc::Receiver<blvm_node::module::ipc::protocol::ModuleMessage>, ModuleError>
    {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }
    async fn get_mempool_transactions(&self) -> Result<Vec<Hash>, ModuleError> {
        Ok(Vec::new())
    }
    async fn get_mempool_transaction(&self, _: &Hash) -> Result<Option<Transaction>, ModuleError> {
        Ok(None)
    }
    async fn get_mempool_size(&self) -> Result<MempoolSize, ModuleError> {
        Ok(MempoolSize {
            transaction_count: 0,
            size_bytes: 0,
            total_fee_sats: 0,
        })
    }
    async fn get_network_stats(&self) -> Result<NetworkStats, ModuleError> {
        Ok(NetworkStats {
            peer_count: 0,
            hash_rate: 0.0,
            bytes_sent: 0,
            bytes_received: 0,
        })
    }
    async fn get_network_peers(&self) -> Result<Vec<PeerInfo>, ModuleError> {
        Ok(Vec::new())
    }
    async fn get_chain_info(&self) -> Result<ChainInfo, ModuleError> {
        Ok(ChainInfo {
            tip_hash: [0u8; 32],
            height: 0,
            difficulty: 1,
            chain_work: 0,
            is_synced: true,
        })
    }
    async fn get_block_by_height(&self, _: u64) -> Result<Option<Block>, ModuleError> {
        Ok(None)
    }
    async fn get_lightning_node_url(&self) -> Result<Option<String>, ModuleError> {
        Ok(None)
    }
    async fn get_lightning_info(&self) -> Result<Option<LightningInfo>, ModuleError> {
        Ok(None)
    }
    async fn get_payment_state(&self, _: &str) -> Result<Option<PaymentState>, ModuleError> {
        Ok(None)
    }
    async fn check_transaction_in_mempool(&self, _: &Hash) -> Result<bool, ModuleError> {
        Ok(false)
    }
    async fn get_fee_estimate(&self, _: u32) -> Result<u64, ModuleError> {
        Ok(1)
    }
    async fn read_file(&self, _: String) -> Result<Vec<u8>, ModuleError> {
        Ok(Vec::new())
    }
    async fn write_file(&self, _: String, _: Vec<u8>) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn delete_file(&self, _: String) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn list_directory(&self, _: String) -> Result<Vec<String>, ModuleError> {
        Ok(Vec::new())
    }
    async fn create_directory(&self, _: String) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_file_metadata(&self, _: String) -> Result<FileMetadata, ModuleError> {
        Ok(FileMetadata {
            path: String::new(),
            size: 0,
            is_file: false,
            is_directory: false,
            modified: None,
            created: None,
        })
    }
    async fn get_all_metrics(
        &self,
    ) -> Result<HashMap<String, Vec<Metric>>, ModuleError> {
        Ok(HashMap::new())
    }
    async fn register_rpc_endpoint(&self, _: String, _: String) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn unregister_rpc_endpoint(&self, _: &str) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn register_core_rpc_override(&self, _: String, _: String) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn unregister_core_rpc_override(&self, _: &str) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn register_timer(
        &self,
        _: u64,
        _: Arc<dyn TimerCallback>,
    ) -> Result<TimerId, ModuleError> {
        Ok(0)
    }
    async fn cancel_timer(&self, _: TimerId) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn schedule_task(
        &self,
        _: u64,
        _: Arc<dyn TaskCallback>,
    ) -> Result<TaskId, ModuleError> {
        Ok(0)
    }
    async fn report_metric(&self, _: Metric) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_module_metrics(&self, _: &str) -> Result<Vec<Metric>, ModuleError> {
        Ok(Vec::new())
    }
    async fn initialize_module(
        &self,
        _: String,
        _: std::path::PathBuf,
        _: std::path::PathBuf,
    ) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn discover_modules(&self) -> Result<Vec<ModuleInfo>, ModuleError> {
        Ok(Vec::new())
    }
    async fn get_module_info(&self, _: &str) -> Result<Option<ModuleInfo>, ModuleError> {
        Ok(None)
    }
    async fn is_module_available(&self, _: &str) -> Result<bool, ModuleError> {
        Ok(false)
    }
    async fn publish_event(
        &self,
        _: EventType,
        _: EventPayload,
    ) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn call_module(
        &self,
        _: Option<&str>,
        _: &str,
        _: Vec<u8>,
    ) -> Result<Vec<u8>, ModuleError> {
        Ok(Vec::new())
    }
    async fn register_module_api(&self, _: Arc<dyn ModuleAPI>) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn unregister_module_api(&self) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_module_health(&self, _: &str) -> Result<Option<ModuleHealth>, ModuleError> {
        Ok(None)
    }
    async fn get_all_module_health(
        &self,
    ) -> Result<Vec<(String, ModuleHealth)>, ModuleError> {
        Ok(Vec::new())
    }
    async fn report_module_health(&self, _: ModuleHealth) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn send_mesh_packet_to_module(
        &self,
        _: &str,
        _: Vec<u8>,
        _: String,
    ) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn send_mesh_packet_to_peer(&self, peer_addr: String, packet_data: Vec<u8>) -> Result<(), ModuleError> {
        if let Some((hub, from)) = &self.wire {
            hub.deliver(&peer_addr, from.clone(), packet_data);
        } else {
            self.outbox
                .lock()
                .unwrap()
                .push((peer_addr, packet_data));
        }
        Ok(())
    }
    async fn send_peer_transport_payload(&self, _: String, _: Vec<u8>) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_block_template(
        &self,
        _: Vec<String>,
        _: Option<Vec<u8>>,
        _: Option<String>,
    ) -> Result<blvm_protocol::mining::BlockTemplate, ModuleError> {
        Err(ModuleError::Other("not implemented".into()))
    }
    async fn submit_block(&self, _: Block) -> Result<SubmitBlockResult, ModuleError> {
        Err(ModuleError::Other("not implemented".into()))
    }
    async fn merge_block_serve_denylist(&self, _: &[Hash]) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_block_serve_denylist_snapshot(
        &self,
    ) -> Result<BlockServeDenylistSnapshot, ModuleError> {
        Ok(BlockServeDenylistSnapshot {
            total_count: 0,
            truncated: false,
            hashes: vec![],
        })
    }
    async fn clear_block_serve_denylist(&self) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn replace_block_serve_denylist(&self, _: &[Hash]) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn merge_tx_serve_denylist(&self, _: &[Hash]) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_tx_serve_denylist_snapshot(
        &self,
    ) -> Result<TxServeDenylistSnapshot, ModuleError> {
        Ok(TxServeDenylistSnapshot {
            total_count: 0,
            truncated: false,
            hashes: vec![],
        })
    }
    async fn clear_tx_serve_denylist(&self) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn replace_tx_serve_denylist(&self, _: &[Hash]) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn get_sync_status(&self) -> Result<SyncStatus, ModuleError> {
        Ok(SyncStatus {
            phase: "Synced".to_string(),
            progress: 1.0,
            is_synced: true,
            error_message: None,
        })
    }
    async fn ban_peer(&self, _: &str, _: Option<u64>) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn set_block_serve_maintenance_mode(&self, _: bool) -> Result<(), ModuleError> {
        Ok(())
    }
}
