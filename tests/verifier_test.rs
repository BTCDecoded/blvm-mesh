//! Unit tests for payment verifier

use blvm_mesh::payment_proof::PaymentProof;
use blvm_mesh::verifier::PaymentVerifier;
use blvm_node::module::traits::NodeAPI;
use std::collections::HashMap;
use std::sync::Arc;

// Mock NodeAPI for testing
struct MockNodeAPI;

#[async_trait::async_trait]
impl NodeAPI for MockNodeAPI {
    async fn get_block(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<Option<blvm_protocol::Block>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn get_block_header(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<Option<blvm_protocol::BlockHeader>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn get_transaction(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<Option<blvm_protocol::Transaction>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn has_transaction(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<bool, blvm_node::module::traits::ModuleError> {
        Ok(false)
    }
    async fn get_chain_tip(
        &self,
    ) -> Result<blvm_protocol::Hash, blvm_node::module::traits::ModuleError> {
        Ok([0u8; 32])
    }
    async fn get_block_height(&self) -> Result<u64, blvm_node::module::traits::ModuleError> {
        Ok(100)
    }
    async fn get_utxo(
        &self,
        _: &blvm_protocol::OutPoint,
    ) -> Result<Option<blvm_protocol::UTXO>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn subscribe_events(
        &self,
        _: Vec<blvm_node::module::traits::EventType>,
    ) -> Result<
        tokio::sync::mpsc::Receiver<blvm_node::module::ipc::protocol::ModuleMessage>,
        blvm_node::module::traits::ModuleError,
    > {
        let (_tx, rx) = tokio::sync::mpsc::channel(100);
        Ok(rx)
    }
    async fn get_mempool_transactions(
        &self,
    ) -> Result<Vec<blvm_protocol::Hash>, blvm_node::module::traits::ModuleError> {
        Ok(Vec::new())
    }
    async fn get_mempool_transaction(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<Option<blvm_protocol::Transaction>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn get_mempool_size(
        &self,
    ) -> Result<blvm_node::module::traits::MempoolSize, blvm_node::module::traits::ModuleError>
    {
        Ok(blvm_node::module::traits::MempoolSize {
            transaction_count: 0,
            size_bytes: 0,
            total_fee_sats: 0,
        })
    }
    async fn get_network_stats(
        &self,
    ) -> Result<blvm_node::module::traits::NetworkStats, blvm_node::module::traits::ModuleError>
    {
        Ok(blvm_node::module::traits::NetworkStats {
            peer_count: 0,
            hash_rate: 0.0,
            bytes_sent: 0,
            bytes_received: 0,
        })
    }
    async fn get_network_peers(
        &self,
    ) -> Result<Vec<blvm_node::module::traits::PeerInfo>, blvm_node::module::traits::ModuleError>
    {
        Ok(Vec::new())
    }
    async fn get_chain_info(
        &self,
    ) -> Result<blvm_node::module::traits::ChainInfo, blvm_node::module::traits::ModuleError> {
        Ok(blvm_node::module::traits::ChainInfo {
            tip_hash: [0u8; 32],
            height: 100,
            difficulty: 1,
            chain_work: 0,
            is_synced: true,
        })
    }
    async fn get_block_by_height(
        &self,
        _: u64,
    ) -> Result<Option<blvm_protocol::Block>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn get_lightning_node_url(
        &self,
    ) -> Result<Option<String>, blvm_node::module::traits::ModuleError> {
        Ok(None)
    }
    async fn get_lightning_info(
        &self,
    ) -> Result<
        Option<blvm_node::module::traits::LightningInfo>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(None)
    }
    async fn get_payment_state(
        &self,
        _: &str,
    ) -> Result<
        Option<blvm_node::module::traits::PaymentState>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(None)
    }
    async fn check_transaction_in_mempool(
        &self,
        _: &blvm_protocol::Hash,
    ) -> Result<bool, blvm_node::module::traits::ModuleError> {
        Ok(false)
    }
    async fn get_fee_estimate(
        &self,
        _: u32,
    ) -> Result<u64, blvm_node::module::traits::ModuleError> {
        Ok(1)
    }
    async fn read_file(
        &self,
        _: String,
    ) -> Result<Vec<u8>, blvm_node::module::traits::ModuleError> {
        Ok(Vec::new())
    }
    async fn write_file(
        &self,
        _: String,
        _: Vec<u8>,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn delete_file(&self, _: String) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn list_directory(
        &self,
        _: String,
    ) -> Result<Vec<String>, blvm_node::module::traits::ModuleError> {
        Ok(Vec::new())
    }
    async fn create_directory(
        &self,
        _: String,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn get_file_metadata(
        &self,
        _: String,
    ) -> Result<
        blvm_node::module::ipc::protocol::FileMetadata,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(blvm_node::module::ipc::protocol::FileMetadata {
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
    ) -> Result<
        HashMap<String, Vec<blvm_node::module::metrics::manager::Metric>>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(HashMap::new())
    }
    async fn register_rpc_endpoint(
        &self,
        _: String,
        _: String,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn unregister_rpc_endpoint(
        &self,
        _: &str,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn register_core_rpc_override(
        &self,
        _: String,
        _: String,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn unregister_core_rpc_override(
        &self,
        _: &str,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn register_timer(
        &self,
        _: u64,
        _: Arc<dyn blvm_node::module::timers::manager::TimerCallback>,
    ) -> Result<blvm_node::module::timers::manager::TimerId, blvm_node::module::traits::ModuleError>
    {
        Ok(0)
    }
    async fn cancel_timer(
        &self,
        _: blvm_node::module::timers::manager::TimerId,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn schedule_task(
        &self,
        _: u64,
        _: Arc<dyn blvm_node::module::timers::manager::TaskCallback>,
    ) -> Result<blvm_node::module::timers::manager::TaskId, blvm_node::module::traits::ModuleError>
    {
        Ok(0)
    }
    async fn report_metric(
        &self,
        _: blvm_node::module::metrics::manager::Metric,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn get_module_metrics(
        &self,
        _: &str,
    ) -> Result<
        Vec<blvm_node::module::metrics::manager::Metric>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(Vec::new())
    }
    async fn initialize_module(
        &self,
        _: String,
        _: std::path::PathBuf,
        _: std::path::PathBuf,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn discover_modules(
        &self,
    ) -> Result<Vec<blvm_node::module::traits::ModuleInfo>, blvm_node::module::traits::ModuleError>
    {
        Ok(Vec::new())
    }
    async fn get_module_info(
        &self,
        _: &str,
    ) -> Result<Option<blvm_node::module::traits::ModuleInfo>, blvm_node::module::traits::ModuleError>
    {
        Ok(None)
    }
    async fn is_module_available(
        &self,
        _: &str,
    ) -> Result<bool, blvm_node::module::traits::ModuleError> {
        Ok(false)
    }
    async fn publish_event(
        &self,
        _: blvm_node::module::traits::EventType,
        _: blvm_node::module::ipc::protocol::EventPayload,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn call_module(
        &self,
        _: Option<&str>,
        _: &str,
        _: Vec<u8>,
    ) -> Result<Vec<u8>, blvm_node::module::traits::ModuleError> {
        Ok(Vec::new())
    }
    async fn register_module_api(
        &self,
        _: Arc<dyn blvm_node::module::inter_module::api::ModuleAPI>,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn unregister_module_api(&self) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn get_module_health(
        &self,
        _: &str,
    ) -> Result<
        Option<blvm_node::module::process::monitor::ModuleHealth>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(None)
    }
    async fn get_all_module_health(
        &self,
    ) -> Result<
        Vec<(String, blvm_node::module::process::monitor::ModuleHealth)>,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(Vec::new())
    }
    async fn report_module_health(
        &self,
        _: blvm_node::module::process::monitor::ModuleHealth,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn send_mesh_packet_to_module(
        &self,
        _: &str,
        _: Vec<u8>,
        _: String,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn send_mesh_packet_to_peer(
        &self,
        _: String,
        _: Vec<u8>,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn send_stratum_v2_message_to_peer(
        &self,
        _: String,
        _: Vec<u8>,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
    async fn get_block_template(
        &self,
        _: Vec<String>,
        _: Option<Vec<u8>>,
        _: Option<String>,
    ) -> Result<blvm_protocol::mining::BlockTemplate, blvm_node::module::traits::ModuleError> {
        Err(blvm_node::module::traits::ModuleError::Other(
            "not implemented".into(),
        ))
    }
    async fn submit_block(
        &self,
        _: blvm_protocol::Block,
    ) -> Result<blvm_node::module::traits::SubmitBlockResult, blvm_node::module::traits::ModuleError>
    {
        Err(blvm_node::module::traits::ModuleError::Other(
            "not implemented".into(),
        ))
    }

    async fn merge_block_serve_denylist(
        &self,
        _: &[blvm_protocol::Hash],
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn get_block_serve_denylist_snapshot(
        &self,
    ) -> Result<
        blvm_node::module::traits::BlockServeDenylistSnapshot,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(blvm_node::module::traits::BlockServeDenylistSnapshot {
            total_count: 0,
            truncated: false,
            hashes: vec![],
        })
    }

    async fn clear_block_serve_denylist(
        &self,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn replace_block_serve_denylist(
        &self,
        _: &[blvm_protocol::Hash],
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn merge_tx_serve_denylist(
        &self,
        _: &[blvm_protocol::Hash],
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn get_tx_serve_denylist_snapshot(
        &self,
    ) -> Result<
        blvm_node::module::traits::TxServeDenylistSnapshot,
        blvm_node::module::traits::ModuleError,
    > {
        Ok(blvm_node::module::traits::TxServeDenylistSnapshot {
            total_count: 0,
            truncated: false,
            hashes: vec![],
        })
    }

    async fn clear_tx_serve_denylist(&self) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn replace_tx_serve_denylist(
        &self,
        _: &[blvm_protocol::Hash],
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn get_sync_status(
        &self,
    ) -> Result<blvm_node::module::traits::SyncStatus, blvm_node::module::traits::ModuleError> {
        Ok(blvm_node::module::traits::SyncStatus {
            phase: "Synced".to_string(),
            progress: 1.0,
            is_synced: true,
            error_message: None,
        })
    }

    async fn ban_peer(
        &self,
        _: &str,
        _: Option<u64>,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }

    async fn set_block_serve_maintenance_mode(
        &self,
        _: bool,
    ) -> Result<(), blvm_node::module::traits::ModuleError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_payment_verifier_creation() {
    let node_api = Arc::new(MockNodeAPI);
    let _verifier = PaymentVerifier::new(node_api);
    // Verifier should be created successfully
    assert!(true); // Basic creation test
}

#[tokio::test]
async fn test_expired_payment_proof() {
    let node_api = Arc::new(MockNodeAPI);
    let verifier = PaymentVerifier::new(node_api);

    // Create an expired payment proof
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired_timestamp = now - 3600; // 1 hour ago

    let proof = PaymentProof::Lightning {
        invoice: "lnbc1pstub_invoice".to_string(),
        preimage: [0u8; 32],
        amount_msats: 1000,
        timestamp: expired_timestamp,
        expires_at: expired_timestamp - 100, // Already expired
    };

    let result = verifier.verify(&proof).await;
    assert!(result.is_ok());
    let verification = result.unwrap();
    assert!(!verification.verified);
    assert!(verification.error.is_some());
}
