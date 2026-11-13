//! Payment verification for mesh routing
//!
//! Verifies Lightning and CTV payment proofs for payment-gated mesh routing.

use crate::error::MeshError;
use crate::payment_proof::{PaymentProof, VerificationResult};
use blvm_node::module::traits::NodeAPI;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, warn};

/// Payment verifier for mesh routing
pub struct PaymentVerifier {
    /// Node API for querying payment state
    node_api: Arc<dyn NodeAPI>,
    /// Whether Lightning verification is enabled
    lightning_enabled: bool,
    /// Whether CTV verification is enabled
    #[cfg(feature = "ctv")]
    ctv_enabled: bool,
}

impl PaymentVerifier {
    /// Create a new payment verifier
    pub fn new(node_api: Arc<dyn NodeAPI>) -> Self {
        Self {
            node_api,
            lightning_enabled: true, // Lightning is primary payment method
            #[cfg(feature = "ctv")]
            ctv_enabled: true, // CTV enabled if feature flag is set
        }
    }

    /// Verify a payment proof
    ///
    /// Verifies Lightning or CTV payment proofs for mesh routing.
    /// Returns verification result with amount and validity.
    pub async fn verify(&self, proof: &PaymentProof) -> Result<VerificationResult, MeshError> {
        // Check if proof is expired
        if proof.is_expired() {
            return Ok(VerificationResult::failure(
                "Payment proof expired".to_string(),
            ));
        }

        match proof {
            PaymentProof::Lightning {
                invoice,
                preimage,
                amount_msats,
                timestamp,
                expires_at,
            } => {
                self.verify_lightning(invoice, preimage, *amount_msats, *timestamp, *expires_at)
                    .await
            }
            PaymentProof::OnChainSettlement {
                payment_request_id,
                tx_hash,
                amount_sats,
                timestamp,
            } => {
                self.verify_on_chain(payment_request_id, tx_hash, *amount_sats, *timestamp)
                    .await
            }
            #[cfg(feature = "ctv")]
            PaymentProof::InstantSettlement {
                covenant_proof,
                output_index,
                amount_sats,
                timestamp,
                ..
            } => {
                self.verify_ctv(covenant_proof, *output_index, *amount_sats, *timestamp)
                    .await
            }
        }
    }

    /// Verify Lightning payment proof
    async fn verify_lightning(
        &self,
        invoice: &str,
        preimage: &[u8; 32],
        amount_msats: u64,
        timestamp: u64,
        expires_at: u64,
    ) -> Result<VerificationResult, MeshError> {
        if !self.lightning_enabled {
            return Ok(VerificationResult::failure(
                "Lightning verification not enabled".to_string(),
            ));
        }

        debug!(
            "Verifying Lightning payment: invoice={}, amount={} msats",
            invoice, amount_msats
        );

        // Parse BOLT11 invoice
        use lightning_invoice::Bolt11Invoice;
        let parsed_invoice = match Bolt11Invoice::from_str(invoice) {
            Ok(inv) => inv,
            Err(e) => {
                warn!("Failed to parse Lightning invoice: {:?}", e);
                return Ok(VerificationResult::failure(format!(
                    "Invalid Lightning invoice format: {e:?}"
                )));
            }
        };

        // Verify payment hash matches preimage
        // Calculate SHA256 of preimage to get payment hash
        use sha2::{Digest, Sha256};
        let preimage_hash = Sha256::digest(preimage);

        // Get payment hash from invoice
        use bitcoin::hashes::Hash;
        if preimage_hash.as_slice() != parsed_invoice.payment_hash().as_byte_array() {
            warn!("Payment hash mismatch: preimage hash does not match invoice");
            return Ok(VerificationResult::failure(
                "Payment preimage hash does not match invoice payment hash".to_string(),
            ));
        }

        // Verify amount matches when the invoice encodes an amount.
        if let Some(invoice_msats) = parsed_invoice.amount_milli_satoshis() {
            if invoice_msats != amount_msats {
                warn!(
                    "Invoice amount mismatch: proof={} msats, invoice={} msats",
                    amount_msats, invoice_msats
                );
                return Ok(VerificationResult::failure(format!(
                    "Lightning amount mismatch: proof has {amount_msats} msats, invoice encodes {invoice_msats} msats"
                )));
            }
        }

        // Verify expiry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if expires_at < now {
            warn!("Invoice expired: expires_at={}, now={}", expires_at, now);
            return Ok(VerificationResult::failure(
                "Lightning invoice has expired".to_string(),
            ));
        }

        // A matching preimage shows the sender knows the invoice. It does not
        // show this node was paid. The Lightning module reports the amount it
        // stored for an inbound payment.
        let payment_hash = hex::encode(preimage_hash);
        let params = serde_json::json!({
            "invoice": invoice,
            "payment_hash": payment_hash,
            "payment_id": payment_hash,
        });
        let body = serde_json::to_vec(&params).unwrap_or_default();
        let response = match self
            .node_api
            .call_module(Some("blvm-lightning"), "verify_payment", body)
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                debug!("Lightning module did not verify the payment: {}", e);
                return Ok(VerificationResult::failure(
                    "lightning payment was not verified by this node".to_string(),
                ));
            }
        };
        let parsed: serde_json::Value = match serde_json::from_slice(&response) {
            Ok(value) => value,
            Err(_) => {
                return Ok(VerificationResult::failure(
                    "lightning module returned an unreadable verification".to_string(),
                ));
            }
        };
        if parsed.get("verified").and_then(|v| v.as_bool()) != Some(true) {
            return Ok(VerificationResult::failure(
                "lightning payment was not received by this node".to_string(),
            ));
        }
        let Some(received_msats) = parsed.get("amount_msats").and_then(|v| v.as_u64()) else {
            return Ok(VerificationResult::failure(
                "lightning module did not report a received amount".to_string(),
            ));
        };
        if received_msats == 0 || amount_msats > received_msats {
            return Ok(VerificationResult::failure(
                "claimed lightning amount exceeds the amount this node received".to_string(),
            ));
        }

        Ok(VerificationResult::success(
            received_msats / 1000,
            timestamp,
            Some(expires_at),
        ))
    }

    /// Verify on-chain settlement against a payment this node recorded.
    ///
    /// Mempool membership is not a payment. The stock node has no payment
    /// state, and that error fails closed.
    async fn verify_on_chain(
        &self,
        payment_request_id: &str,
        tx_hash: &[u8; 32],
        amount_sats: u64,
        timestamp: u64,
    ) -> Result<VerificationResult, MeshError> {
        if payment_request_id.is_empty() {
            return Ok(VerificationResult::failure(
                "empty payment_request_id".to_string(),
            ));
        }
        if tx_hash.iter().all(|&b| b == 0) {
            return Ok(VerificationResult::failure("invalid tx_hash".to_string()));
        }
        if amount_sats == 0 {
            return Ok(VerificationResult::failure("zero amount".to_string()));
        }

        let state = match self.node_api.get_payment_state(payment_request_id).await {
            Ok(Some(state)) => state,
            Ok(None) => {
                return Ok(VerificationResult::failure(
                    "no payment recorded for this request".to_string(),
                ));
            }
            Err(e) => {
                debug!("Payment state unavailable for {payment_request_id}: {e}");
                return Ok(VerificationResult::failure(
                    "payment state unavailable".to_string(),
                ));
            }
        };
        if !Self::received_on_chain_status(&state.status) {
            return Ok(VerificationResult::failure(format!(
                "payment is not received (status {})",
                state.status
            )));
        }
        if state.amount_sats < amount_sats {
            return Ok(VerificationResult::failure(format!(
                "claimed amount {amount_sats} sats exceeds recorded payment {} sats",
                state.amount_sats
            )));
        }
        match state.tx_hash {
            Some(hash) if hash == *tx_hash => {}
            Some(_) => {
                return Ok(VerificationResult::failure(
                    "payment state tx_hash mismatch".to_string(),
                ));
            }
            None => {
                return Ok(VerificationResult::failure(
                    "payment state has no transaction".to_string(),
                ));
            }
        }

        Ok(VerificationResult::success(
            state.amount_sats,
            timestamp,
            None,
        ))
    }

    /// Verify CTV instant settlement proof
    #[cfg(feature = "ctv")]
    async fn verify_ctv(
        &self,
        covenant_proof: &[u8],
        output_index: u32,
        amount_sats: u64,
        timestamp: u64,
    ) -> Result<VerificationResult, MeshError> {
        if !self.ctv_enabled {
            return Ok(VerificationResult::failure(
                "CTV verification not enabled".to_string(),
            ));
        }

        debug!(
            "Verifying CTV payment: output_index={}, amount={} sats",
            output_index, amount_sats
        );

        // CTV verification via NodeAPI
        // CTV (CheckTemplateVerify) payments are verified by checking on-chain transactions
        // This verifier checks CTV payment proofs that reference on-chain transactions
        // This should:
        // 1. Deserialize CovenantProof from bytes
        // 2. Verify template hash matches expected structure
        // 3. Verify output amount matches
        // 4. Check if transaction is in mempool or confirmed
        // 5. Verify merkle proof (if provided)

        // Deserialize covenant proof
        if covenant_proof.is_empty() {
            return Ok(VerificationResult::failure(
                "Empty CTV covenant proof".to_string(),
            ));
        }
        if covenant_proof.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Ok(VerificationResult::failure(format!(
                "Covenant proof too large: {} bytes (max: {} bytes)",
                covenant_proof.len(),
                crate::packet::MAX_BINCODE_PAYLOAD_SIZE
            )));
        }

        // Deserialize CovenantProof from bytes
        use blvm_node::payment::covenant::CovenantProof;
        let proof: CovenantProof = match bincode::deserialize(covenant_proof) {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to deserialize CTV covenant proof: {}", e);
                return Ok(VerificationResult::failure(format!(
                    "Invalid CTV covenant proof format: {}",
                    e
                )));
            }
        };

        // Verify output amount matches
        // Check if the proof's transaction template has an output matching the expected amount
        let expected_amount = amount_sats;
        let mut found_matching_output = false;

        for output in &proof.transaction_template.outputs {
            if output.value == expected_amount {
                found_matching_output = true;
                break;
            }
        }

        if !found_matching_output {
            warn!(
                "CTV proof output amount mismatch: expected {} sats",
                expected_amount
            );
            return Ok(VerificationResult::failure(
                "CTV covenant proof output amount does not match expected amount".to_string(),
            ));
        }

        // Verify template hash and output structure via the public covenant API.
        use blvm_node::payment::covenant::CovenantEngine;
        use blvm_protocol::payment::PaymentOutput;
        let covenant_engine = CovenantEngine::new();
        let expected_outputs: Vec<PaymentOutput> = proof
            .transaction_template
            .outputs
            .iter()
            .map(|o| PaymentOutput {
                amount: Some(o.value),
                script: o.script_pubkey.clone(),
            })
            .collect();
        match covenant_engine.verify_covenant_proof(&proof, &expected_outputs) {
            Ok(true) => {}
            Ok(false) => {
                warn!("CTV covenant proof verification failed");
                return Ok(VerificationResult::failure(
                    "CTV covenant proof template hash verification failed".to_string(),
                ));
            }
            Err(e) => {
                warn!("Failed to verify CTV covenant proof: {}", e);
                return Ok(VerificationResult::failure(format!(
                    "CTV template hash calculation error: {}",
                    e
                )));
            }
        }

        debug!("CTV covenant proof verified successfully");
        Ok(VerificationResult::success(amount_sats, timestamp, None))
    }

    /// A request that is only pending, or that failed, is not a receipt.
    fn received_on_chain_status(status: &str) -> bool {
        matches!(status, "confirmed" | "settled" | "in_mempool")
    }

    /// Check if minimum payment amount is met
    pub fn check_minimum_payment(&self, amount_sats: u64, minimum_sats: u64) -> bool {
        amount_sats >= minimum_sats
    }

    /// Calculate required payment amount for data size
    pub fn calculate_payment_for_data(
        &self,
        data_size_bytes: usize,
        rate_sats_per_byte: u64,
    ) -> u64 {
        (data_size_bytes as u64) * rate_sats_per_byte
    }

    /// Verify multiple payment proofs in parallel (batch operation)
    ///
    /// Processes multiple payment verifications concurrently for better performance.
    /// Returns a vector of verification results in the same order as inputs.
    pub async fn verify_batch(
        &self,
        proofs: &[&PaymentProof],
    ) -> Result<Vec<VerificationResult>, MeshError> {
        if proofs.is_empty() {
            return Ok(Vec::new());
        }

        // Verify all proofs in parallel
        let futures: Vec<_> = proofs.iter().map(|proof| self.verify(proof)).collect();

        // Wait for all verifications to complete
        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
    }
}
