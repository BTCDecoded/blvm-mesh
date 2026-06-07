//! Unit tests for payment verifier

mod ln_helpers;
mod on_chain_mock;

use blvm_mesh::payment_proof::PaymentProof;
use blvm_mesh::verifier::PaymentVerifier;
use blvm_node::module::traits::NodeAPI;
use ln_helpers::test_lightning_invoice;
use on_chain_mock::OnChainMockNode;
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
    async fn send_peer_transport_payload(
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

#[tokio::test]
async fn test_on_chain_settlement_stub_accepts_valid_proof() {
    let tx_hash = [7u8; 32];
    let node_api = OnChainMockNode::with_mempool_tx(tx_hash);
    let verifier = PaymentVerifier::new(node_api);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let proof = PaymentProof::OnChainSettlement {
        payment_request_id: "req-abc".to_string(),
        tx_hash,
        amount_sats: 500,
        timestamp: now,
    };

    let verification = verifier.verify(&proof).await.unwrap();
    assert!(verification.verified);
    assert_eq!(verification.amount, 500);
}

#[tokio::test]
async fn test_on_chain_settlement_stub_rejects_zero_amount() {
    let node_api = Arc::new(MockNodeAPI);
    let verifier = PaymentVerifier::new(node_api);

    let proof = PaymentProof::OnChainSettlement {
        payment_request_id: "req-abc".to_string(),
        tx_hash: [7u8; 32],
        amount_sats: 0,
        timestamp: 1,
    };

    let verification = verifier.verify(&proof).await.unwrap();
    assert!(!verification.verified);
}

#[tokio::test]
async fn test_on_chain_rejects_without_mempool_or_state() {
    let node_api = Arc::new(MockNodeAPI);
    let verifier = PaymentVerifier::new(node_api);
    let proof = PaymentProof::OnChainSettlement {
        payment_request_id: "req-x".to_string(),
        tx_hash: [8u8; 32],
        amount_sats: 100,
        timestamp: 1,
    };
    let v = verifier.verify(&proof).await.unwrap();
    assert!(!v.verified);
}

#[tokio::test]
async fn test_on_chain_accepts_matching_payment_state() {
    use blvm_node::module::traits::PaymentState;
    let tx_hash = [9u8; 32];
    let state = PaymentState {
        payment_id: "req-state".to_string(),
        status: "pending".to_string(),
        amount_sats: 250,
        tx_hash: Some(tx_hash),
        confirmations: None,
    };
    let node_api = OnChainMockNode::with_payment_state(state);
    let verifier = PaymentVerifier::new(node_api);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let proof = PaymentProof::OnChainSettlement {
        payment_request_id: "req-state".to_string(),
        tx_hash,
        amount_sats: 250,
        timestamp: now,
    };
    assert!(verifier.verify(&proof).await.unwrap().verified);
}

#[tokio::test]
async fn test_lightning_amount_mismatch_rejected() {
    let node_api = Arc::new(MockNodeAPI);
    let verifier = PaymentVerifier::new(node_api);
    let preimage = [3u8; 32];
    let invoice = test_lightning_invoice(1000, preimage);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let proof = PaymentProof::Lightning {
        invoice,
        preimage,
        amount_msats: 2000,
        timestamp: now,
        expires_at: now + 3600,
    };
    let v = verifier.verify(&proof).await.unwrap();
    assert!(!v.verified);
    assert!(v.error.unwrap().contains("amount mismatch"));
}

#[tokio::test]
async fn test_lightning_valid_preimage_and_amount() {
    let node_api = Arc::new(MockNodeAPI);
    let verifier = PaymentVerifier::new(node_api);
    let preimage = [4u8; 32];
    let amount_msats = 5000;
    let invoice = test_lightning_invoice(amount_msats, preimage);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let proof = PaymentProof::Lightning {
        invoice,
        preimage,
        amount_msats,
        timestamp: now,
        expires_at: now + 3600,
    };
    assert!(verifier.verify(&proof).await.unwrap().verified);
}
