//! Payment proof structures for mesh routing
//!
//! Defines payment proof types (Lightning and CTV) for payment-gated mesh routing.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Payment proof for mesh routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentProof {
    /// Lightning payment proof (BOLT11 invoice + preimage)
    Lightning {
        /// Lightning invoice (BOLT11 format)
        invoice: String,
        /// Payment preimage (32 bytes)
        preimage: [u8; 32],
        /// Amount in millisatoshis
        amount_msats: u64,
        /// Payment timestamp
        timestamp: u64,
        /// Invoice expiry timestamp
        expires_at: u64,
    },
    /// CTV instant settlement proof (future, when CTV is activated)
    #[cfg(feature = "ctv")]
    InstantSettlement {
        /// CTV covenant proof (template hash + transaction structure)
        covenant_proof: Vec<u8>, // Serialized CovenantProof from bllvm-node
        /// Output index in the covenant transaction
        output_index: u32,
        /// Merkle proof (if needed for verification)
        merkle_proof: Vec<[u8; 32]>,
        /// Amount in satoshis
        amount_sats: u64,
        /// Proof timestamp
        timestamp: u64,
    },
}

impl PaymentProof {
    /// Get payment amount in satoshis
    pub fn amount_sats(&self) -> u64 {
        match self {
            PaymentProof::Lightning { amount_msats, .. } => amount_msats / 1000,
            #[cfg(feature = "ctv")]
            PaymentProof::InstantSettlement { amount_sats, .. } => *amount_sats,
        }
    }

    /// Get payment timestamp
    pub fn timestamp(&self) -> u64 {
        match self {
            PaymentProof::Lightning { timestamp, .. } => *timestamp,
            #[cfg(feature = "ctv")]
            PaymentProof::InstantSettlement { timestamp, .. } => *timestamp,
        }
    }

    /// Check if payment proof is expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        match self {
            PaymentProof::Lightning { expires_at, .. } => now > *expires_at,
            #[cfg(feature = "ctv")]
            PaymentProof::InstantSettlement { timestamp, .. } => {
                // CTV proofs don't expire (they're on-chain commitments)
                // But we can check if they're too old (e.g., > 24 hours)
                const MAX_AGE_SECONDS: u64 = 24 * 60 * 60; // 24 hours
                now > timestamp + MAX_AGE_SECONDS
            }
        }
    }

    /// Calculate hash of payment proof (for replay prevention)
    pub fn hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        
        let serialized = bincode::serialize(self)
            .expect("Payment proof should be serializable");
        let hash = Sha256::digest(&serialized);
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }
}

/// Payment verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether payment is verified
    pub verified: bool,
    /// Amount in satoshis
    pub amount: u64,
    /// Payment timestamp
    pub timestamp: u64,
    /// Expiry timestamp (if applicable)
    pub expires_at: Option<u64>,
    /// Verification error message (if verification failed)
    pub error: Option<String>,
}

impl VerificationResult {
    /// Create a successful verification result
    pub fn success(amount: u64, timestamp: u64, expires_at: Option<u64>) -> Self {
        Self {
            verified: true,
            amount,
            timestamp,
            expires_at,
            error: None,
        }
    }

    /// Create a failed verification result
    pub fn failure(error: String) -> Self {
        Self {
            verified: false,
            amount: 0,
            timestamp: 0,
            expires_at: None,
            error: Some(error),
        }
    }
}

