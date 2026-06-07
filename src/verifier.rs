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
        use lightning_invoice::Invoice;
        let parsed_invoice = match Invoice::from_str(invoice) {
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
        // lightning-invoice 0.2: payment_hash() returns &Sha256 (which wraps sha256::Hash)
        // Convert hash to bytes via hex string (sha256::Hash Display outputs hex)
        let invoice_payment_hash = parsed_invoice.payment_hash();
        let hash_str = format!("{}", invoice_payment_hash.0);
        let invoice_hash_bytes = hex::decode(hash_str).map_err(|e| {
            MeshError::PaymentError(format!("Failed to decode payment hash: {e}"))
        })?;
        let mut invoice_hash_array = [0u8; 32];
        invoice_hash_array.copy_from_slice(&invoice_hash_bytes[..32]);

        // Verify preimage hash matches invoice payment hash
        if preimage_hash.as_slice() != invoice_hash_array.as_slice() {
            warn!("Payment hash mismatch: preimage hash does not match invoice");
            return Ok(VerificationResult::failure(
                "Payment preimage hash does not match invoice payment hash".to_string(),
            ));
        }

        // Verify amount matches when the invoice encodes an amount.
        if let Some(pico) = parsed_invoice.amount_pico_btc() {
            // BOLT11 amount is in pico-BTC; 1 msat = 10 pico-BTC.
            let invoice_msats = pico / 10;
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

        // Check if payment exists in node's payment system (optional verification)
        let payment_id = format!("lightning_{}", hex::encode(&preimage[..16]));
        match self.node_api.get_payment_state(&payment_id).await {
            Ok(Some(payment_state)) => {
                // Payment exists in node's payment system - additional verification
                debug!("Payment found in node state: {:?}", payment_state);
            }
            Ok(None) => {
                // Payment not in node's system yet - that's okay for mesh routing
                // The invoice and preimage verification above is sufficient
                debug!("Payment not yet in node state, but invoice verification passed");
            }
            Err(e) => {
                // Error querying - log but don't fail (invoice verification is primary)
                debug!("Error querying payment state (non-fatal): {}", e);
            }
        }

        // All verifications passed
        Ok(VerificationResult::success(
            amount_msats / 1000, // Convert to satoshis
            timestamp,
            Some(expires_at),
        ))
    }

    /// Verify on-chain settlement proof (path 3 — mempool + payment state via NodeAPI).
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

        let mut mempool_ok = false;
        if let Ok(in_mempool) = self.node_api.check_transaction_in_mempool(tx_hash).await {
            if in_mempool {
                mempool_ok = true;
            }
        }

        if let Ok(Some(state)) = self
            .node_api
            .get_payment_state(payment_request_id)
            .await
        {
            debug!("On-chain payment state for {}: {:?}", payment_request_id, state);
            if state.amount_sats != amount_sats {
                return Ok(VerificationResult::failure(format!(
                    "payment state amount mismatch: expected {} sats, got {}",
                    amount_sats, state.amount_sats
                )));
            }
            if let Some(ref hash) = state.tx_hash {
                if hash != tx_hash {
                    return Ok(VerificationResult::failure(
                        "payment state tx_hash mismatch".to_string(),
                    ));
                }
            }
            if state.status == "failed" {
                return Ok(VerificationResult::failure(
                    "payment state is failed".to_string(),
                ));
            }
            return Ok(VerificationResult::success(amount_sats, timestamp, None));
        }

        if mempool_ok {
            return Ok(VerificationResult::success(amount_sats, timestamp, None));
        }

        Ok(VerificationResult::failure(
            "transaction not in mempool and no matching payment state".to_string(),
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
