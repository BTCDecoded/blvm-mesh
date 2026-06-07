//! Configurable on-chain mocks for path-3 verifier tests.

use async_trait::async_trait;
use blvm_mesh::test_support::TestNodeAPI;
use blvm_node::module::traits::{ModuleError, NodeAPI, PaymentState};
use blvm_protocol::Hash;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct OnChainMockNode {
    inner: Arc<TestNodeAPI>,
    mempool: Mutex<Vec<[u8; 32]>>,
    payments: Mutex<HashMap<String, PaymentState>>,
}

impl OnChainMockNode {
    pub fn with_mempool_tx(tx_hash: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(TestNodeAPI::default()),
            mempool: Mutex::new(vec![tx_hash]),
            payments: Mutex::new(HashMap::new()),
        })
    }

    pub fn with_payment_state(state: PaymentState) -> Arc<Self> {
        let mut map = HashMap::new();
        map.insert(state.payment_id.clone(), state);
        Arc::new(Self {
            inner: Arc::new(TestNodeAPI::default()),
            mempool: Mutex::new(Vec::new()),
            payments: Mutex::new(map),
        })
    }
}

#[async_trait]
impl NodeAPI for OnChainMockNode {
    async fn check_transaction_in_mempool(&self, hash: &Hash) -> Result<bool, ModuleError> {
        Ok(self.mempool.lock().unwrap().iter().any(|h| h == hash))
    }

    async fn get_payment_state(&self, id: &str) -> Result<Option<PaymentState>, ModuleError> {
        Ok(self.payments.lock().unwrap().get(id).cloned())
    }

    async fn get_block(
        &self,
        h: &Hash,
    ) -> Result<Option<blvm_protocol::Block>, ModuleError> {
        self.inner.get_block(h).await
    }
    async fn get_block_header(
        &self,
        h: &Hash,
    ) -> Result<Option<blvm_protocol::BlockHeader>, ModuleError> {
        self.inner.get_block_header(h).await
    }
    async fn get_transaction(
        &self,
        h: &Hash,
    ) -> Result<Option<blvm_protocol::Transaction>, ModuleError> {
        self.inner.get_transaction(h).await
    }
    async fn has_transaction(&self, h: &Hash) -> Result<bool, ModuleError> {
        self.inner.has_transaction(h).await
    }
    async fn get_chain_tip(&self) -> Result<Hash, ModuleError> {
        self.inner.get_chain_tip().await
    }
    async fn get_block_height(&self) -> Result<u64, ModuleError> {
        self.inner.get_block_height().await
    }
    async fn get_utxo(
        &self,
        o: &blvm_protocol::OutPoint,
    ) -> Result<Option<blvm_protocol::UTXO>, ModuleError> {
        self.inner.get_utxo(o).await
    }
    async fn subscribe_events(
        &self,
        e: Vec<blvm_node::module::traits::EventType>,
    ) -> Result<
        tokio::sync::mpsc::Receiver<blvm_node::module::ipc::protocol::ModuleMessage>,
        ModuleError,
    > {
        self.inner.subscribe_events(e).await
    }
    async fn get_mempool_transactions(&self) -> Result<Vec<Hash>, ModuleError> {
        self.inner.get_mempool_transactions().await
    }
    async fn get_mempool_transaction(
        &self,
        h: &Hash,
    ) -> Result<Option<blvm_protocol::Transaction>, ModuleError> {
        self.inner.get_mempool_transaction(h).await
    }
    async fn get_mempool_size(
        &self,
    ) -> Result<blvm_node::module::traits::MempoolSize, ModuleError> {
        self.inner.get_mempool_size().await
    }
    async fn get_network_stats(
        &self,
    ) -> Result<blvm_node::module::traits::NetworkStats, ModuleError> {
        self.inner.get_network_stats().await
    }
    async fn get_network_peers(
        &self,
    ) -> Result<Vec<blvm_node::module::traits::PeerInfo>, ModuleError> {
        self.inner.get_network_peers().await
    }
    async fn get_chain_info(
        &self,
    ) -> Result<blvm_node::module::traits::ChainInfo, ModuleError> {
        self.inner.get_chain_info().await
    }
    async fn get_block_by_height(
        &self,
        h: u64,
    ) -> Result<Option<blvm_protocol::Block>, ModuleError> {
        self.inner.get_block_by_height(h).await
    }
    async fn get_lightning_node_url(&self) -> Result<Option<String>, ModuleError> {
        self.inner.get_lightning_node_url().await
    }
    async fn get_lightning_info(
        &self,
    ) -> Result<Option<blvm_node::module::traits::LightningInfo>, ModuleError> {
        self.inner.get_lightning_info().await
    }
    async fn get_fee_estimate(&self, c: u32) -> Result<u64, ModuleError> {
        self.inner.get_fee_estimate(c).await
    }
    async fn read_file(&self, p: String) -> Result<Vec<u8>, ModuleError> {
        self.inner.read_file(p).await
    }
    async fn write_file(&self, p: String, d: Vec<u8>) -> Result<(), ModuleError> {
        self.inner.write_file(p, d).await
    }
    async fn delete_file(&self, p: String) -> Result<(), ModuleError> {
        self.inner.delete_file(p).await
    }
    async fn list_directory(&self, p: String) -> Result<Vec<String>, ModuleError> {
        self.inner.list_directory(p).await
    }
    async fn create_directory(&self, p: String) -> Result<(), ModuleError> {
        self.inner.create_directory(p).await
    }
    async fn get_file_metadata(
        &self,
        p: String,
    ) -> Result<blvm_node::module::ipc::protocol::FileMetadata, ModuleError> {
        self.inner.get_file_metadata(p).await
    }
    async fn get_all_metrics(
        &self,
    ) -> Result<
        std::collections::HashMap<String, Vec<blvm_node::module::metrics::manager::Metric>>,
        ModuleError,
    > {
        self.inner.get_all_metrics().await
    }
    async fn register_rpc_endpoint(&self, a: String, b: String) -> Result<(), ModuleError> {
        self.inner.register_rpc_endpoint(a, b).await
    }
    async fn unregister_rpc_endpoint(&self, n: &str) -> Result<(), ModuleError> {
        self.inner.unregister_rpc_endpoint(n).await
    }
    async fn register_core_rpc_override(&self, a: String, b: String) -> Result<(), ModuleError> {
        self.inner.register_core_rpc_override(a, b).await
    }
    async fn unregister_core_rpc_override(&self, n: &str) -> Result<(), ModuleError> {
        self.inner.unregister_core_rpc_override(n).await
    }
    async fn register_timer(
        &self,
        d: u64,
        cb: Arc<dyn blvm_node::module::timers::manager::TimerCallback>,
    ) -> Result<blvm_node::module::timers::manager::TimerId, ModuleError> {
        self.inner.register_timer(d, cb).await
    }
    async fn cancel_timer(
        &self,
        id: blvm_node::module::timers::manager::TimerId,
    ) -> Result<(), ModuleError> {
        self.inner.cancel_timer(id).await
    }
    async fn schedule_task(
        &self,
        d: u64,
        cb: Arc<dyn blvm_node::module::timers::manager::TaskCallback>,
    ) -> Result<blvm_node::module::timers::manager::TaskId, ModuleError> {
        self.inner.schedule_task(d, cb).await
    }
    async fn report_metric(
        &self,
        m: blvm_node::module::metrics::manager::Metric,
    ) -> Result<(), ModuleError> {
        self.inner.report_metric(m).await
    }
    async fn get_module_metrics(
        &self,
        id: &str,
    ) -> Result<Vec<blvm_node::module::metrics::manager::Metric>, ModuleError> {
        self.inner.get_module_metrics(id).await
    }
    async fn initialize_module(
        &self,
        a: String,
        b: std::path::PathBuf,
        c: std::path::PathBuf,
    ) -> Result<(), ModuleError> {
        self.inner.initialize_module(a, b, c).await
    }
    async fn discover_modules(
        &self,
    ) -> Result<Vec<blvm_node::module::traits::ModuleInfo>, ModuleError> {
        self.inner.discover_modules().await
    }
    async fn get_module_info(
        &self,
        id: &str,
    ) -> Result<Option<blvm_node::module::traits::ModuleInfo>, ModuleError> {
        self.inner.get_module_info(id).await
    }
    async fn is_module_available(&self, id: &str) -> Result<bool, ModuleError> {
        self.inner.is_module_available(id).await
    }
    async fn publish_event(
        &self,
        t: blvm_node::module::traits::EventType,
        p: blvm_node::module::ipc::protocol::EventPayload,
    ) -> Result<(), ModuleError> {
        self.inner.publish_event(t, p).await
    }
    async fn call_module(
        &self,
        m: Option<&str>,
        method: &str,
        params: Vec<u8>,
    ) -> Result<Vec<u8>, ModuleError> {
        self.inner.call_module(m, method, params).await
    }
    async fn register_module_api(
        &self,
        api: Arc<dyn blvm_node::module::inter_module::api::ModuleAPI>,
    ) -> Result<(), ModuleError> {
        self.inner.register_module_api(api).await
    }
    async fn unregister_module_api(&self) -> Result<(), ModuleError> {
        self.inner.unregister_module_api().await
    }
    async fn get_module_health(
        &self,
        id: &str,
    ) -> Result<Option<blvm_node::module::process::monitor::ModuleHealth>, ModuleError> {
        self.inner.get_module_health(id).await
    }
    async fn get_all_module_health(
        &self,
    ) -> Result<
        Vec<(
            String,
            blvm_node::module::process::monitor::ModuleHealth,
        )>,
        ModuleError,
    > {
        self.inner.get_all_module_health().await
    }
    async fn report_module_health(
        &self,
        h: blvm_node::module::process::monitor::ModuleHealth,
    ) -> Result<(), ModuleError> {
        self.inner.report_module_health(h).await
    }
    async fn send_mesh_packet_to_module(
        &self,
        m: &str,
        p: Vec<u8>,
        a: String,
    ) -> Result<(), ModuleError> {
        self.inner.send_mesh_packet_to_module(m, p, a).await
    }
    async fn send_mesh_packet_to_peer(&self, a: String, p: Vec<u8>) -> Result<(), ModuleError> {
        self.inner.send_mesh_packet_to_peer(a, p).await
    }
    async fn send_peer_transport_payload(&self, a: String, p: Vec<u8>) -> Result<(), ModuleError> {
        self.inner.send_peer_transport_payload(a, p).await
    }
    async fn get_block_template(
        &self,
        a: Vec<String>,
        b: Option<Vec<u8>>,
        c: Option<String>,
    ) -> Result<blvm_protocol::mining::BlockTemplate, ModuleError> {
        self.inner.get_block_template(a, b, c).await
    }
    async fn submit_block(
        &self,
        b: blvm_protocol::Block,
    ) -> Result<blvm_node::module::traits::SubmitBlockResult, ModuleError> {
        self.inner.submit_block(b).await
    }
    async fn merge_block_serve_denylist(&self, h: &[Hash]) -> Result<(), ModuleError> {
        self.inner.merge_block_serve_denylist(h).await
    }
    async fn get_block_serve_denylist_snapshot(
        &self,
    ) -> Result<blvm_node::module::traits::BlockServeDenylistSnapshot, ModuleError> {
        self.inner.get_block_serve_denylist_snapshot().await
    }
    async fn clear_block_serve_denylist(&self) -> Result<(), ModuleError> {
        self.inner.clear_block_serve_denylist().await
    }
    async fn replace_block_serve_denylist(&self, h: &[Hash]) -> Result<(), ModuleError> {
        self.inner.replace_block_serve_denylist(h).await
    }
    async fn merge_tx_serve_denylist(&self, h: &[Hash]) -> Result<(), ModuleError> {
        self.inner.merge_tx_serve_denylist(h).await
    }
    async fn get_tx_serve_denylist_snapshot(
        &self,
    ) -> Result<blvm_node::module::traits::TxServeDenylistSnapshot, ModuleError> {
        self.inner.get_tx_serve_denylist_snapshot().await
    }
    async fn clear_tx_serve_denylist(&self) -> Result<(), ModuleError> {
        self.inner.clear_tx_serve_denylist().await
    }
    async fn replace_tx_serve_denylist(&self, h: &[Hash]) -> Result<(), ModuleError> {
        self.inner.replace_tx_serve_denylist(h).await
    }
    async fn get_sync_status(
        &self,
    ) -> Result<blvm_node::module::traits::SyncStatus, ModuleError> {
        self.inner.get_sync_status().await
    }
    async fn ban_peer(&self, p: &str, t: Option<u64>) -> Result<(), ModuleError> {
        self.inner.ban_peer(p, t).await
    }
    async fn set_block_serve_maintenance_mode(&self, on: bool) -> Result<(), ModuleError> {
        self.inner.set_block_serve_maintenance_mode(on).await
    }
}
