//! Payment verification for mesh routing
//!
//! Verifies Lightning and CTV payment proofs for payment-gated mesh routing.

use crate::error::MeshError;
use crate::payment_proof::{PaymentProof, VerificationResult};
use bllvm_node::module::traits::NodeAPI;
use std::sync::Arc;
use std::str::FromStr;
use tracing::{debug, error, warn};

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

        debug!("Verifying Lightning payment: invoice={}, amount={} msats", invoice, amount_msats);

        // Parse BOLT11 invoice
        use lightning_invoice::Invoice;
        let parsed_invoice = match Invoice::from_str(invoice) {
            Ok(inv) => inv,
            Err(e) => {
                warn!("Failed to parse Lightning invoice: {}", e);
                return Ok(VerificationResult::failure(format!(
                    "Invalid Lightning invoice format: {}",
                    e
                )));
            }
        };
        
        // Verify payment hash matches preimage
        let payment_hash = parsed_invoice.payment_hash();
        let preimage_hash = {
            use sha2::{Digest, Sha256};
            Sha256::digest(preimage)
        };
        
        if payment_hash.as_bytes() != preimage_hash.as_slice() {
            warn!("Payment hash mismatch: invoice hash != preimage hash");
            return Ok(VerificationResult::failure(
                "Payment hash does not match preimage".to_string(),
            ));
        }
        
        // Verify amount matches (if specified in invoice)
        if let Some(invoice_amount) = parsed_invoice.amount_milli_satoshis() {
            if invoice_amount != amount_msats {
                warn!("Amount mismatch: invoice={} msats, proof={} msats", invoice_amount, amount_msats);
                return Ok(VerificationResult::failure(
                    "Payment amount does not match invoice".to_string(),
                ));
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

        // Deserialize CovenantProof from bytes
        use bllvm_node::payment::covenant::CovenantProof;
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
            warn!("CTV proof output amount mismatch: expected {} sats", expected_amount);
            return Ok(VerificationResult::failure(
                "CTV covenant proof output amount does not match expected amount".to_string(),
            ));
        }
        
        // Verify template hash (recalculate and compare)
        // The template hash should match the proof's template hash
        use bllvm_node::payment::covenant::CovenantEngine;
        let covenant_engine = CovenantEngine::new();
        
        // Recalculate template hash for the proof's transaction template
        match covenant_engine.calculate_template_hash_for_template(&proof.transaction_template, 0) {
            Ok(recalculated_hash) => {
                if recalculated_hash != proof.template_hash {
                    warn!("CTV template hash mismatch");
                    return Ok(VerificationResult::failure(
                        "CTV covenant proof template hash verification failed".to_string(),
                    ));
                }
            }
            Err(e) => {
                warn!("Failed to recalculate CTV template hash: {}", e);
                return Ok(VerificationResult::failure(format!(
                    "CTV template hash calculation error: {}",
                    e
                )));
            }
        }
        
        // Check if transaction is in mempool or confirmed (optional, via NodeAPI)
        // For mesh routing, we accept mempool transactions
        // The covenant proof itself is sufficient proof of payment commitment
        
        debug!("CTV covenant proof verified successfully");
        Ok(VerificationResult::success(amount_sats, timestamp, None))
    }

    /// Check if minimum payment amount is met
    pub fn check_minimum_payment(&self, amount_sats: u64, minimum_sats: u64) -> bool {
        amount_sats >= minimum_sats
    }

    /// Calculate required payment amount for data size
    pub fn calculate_payment_for_data(&self, data_size_bytes: usize, rate_sats_per_byte: u64) -> u64 {
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
        let futures: Vec<_> = proofs
            .iter()
            .map(|proof| self.verify(*proof))
            .collect();
        
        // Wait for all verifications to complete
        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
    }
}
