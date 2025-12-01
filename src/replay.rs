//! Replay prevention for mesh payment proofs
//!
//! Prevents reuse of payment proofs using hash tracking, sequence numbers, and expiry.

use crate::payment_proof::PaymentProof;
use dashmap::DashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

/// Replay prevention entry (combined structure)
#[derive(Debug, Clone)]
struct ReplayEntry {
    /// Timestamp when hash was first seen
    timestamp: u64,
    /// Peer ID that used this hash
    peer_id: [u8; 32],
    /// Sequence number from that peer
    sequence: u64,
}

/// Replay prevention for payment proofs
///
/// Uses DashMap for lock-free concurrent access and combines multiple HashMaps
/// into a single structure to reduce memory overhead.
pub struct ReplayPrevention {
    /// Combined replay data: hash -> (timestamp, peer_id, sequence)
    /// Lock-free concurrent access using DashMap
    replay_data: DashMap<[u8; 32], ReplayEntry>,
    /// Per-peer sequence numbers (to detect out-of-order proofs)
    /// Lock-free concurrent access using DashMap
    used_sequences: DashMap<[u8; 32], u64>, // peer_id -> last_sequence
    /// Expiry time for hashes (default: 24 hours)
    expiry_seconds: u64,
}

impl ReplayPrevention {
    /// Create a new replay prevention system
    pub fn new(expiry_seconds: u64) -> Self {
        Self {
            replay_data: DashMap::new(),
            used_sequences: DashMap::new(),
            expiry_seconds,
        }
    }

    /// Check if payment proof is a replay
    ///
    /// Returns Ok(true) if proof is valid (not a replay), Err if replay detected.
    /// Lock-free operation using DashMap - no mut needed.
    pub fn check_replay(
        &self,
        proof: &PaymentProof,
        peer_id: &[u8; 32],
        sequence: u64,
    ) -> Result<bool, String> {
        // Clean up expired hashes first
        self.cleanup_expired();

        // Check payment hash not reused (lock-free)
        let proof_hash = proof.hash();
        if self.replay_data.contains_key(&proof_hash) {
            return Err("Payment proof already used (replay detected)".to_string());
        }

        // Check sequence number (FIBRE-inspired) - lock-free
        if let Some(entry) = self.used_sequences.get(peer_id) {
            if sequence <= *entry.value() {
                return Err(format!(
                    "Sequence number out of order: got {}, expected > {}",
                    sequence, entry.value()
                ));
            }
        }

        // Check expiry (proof itself checks this, but double-check)
        if proof.is_expired() {
            return Err("Payment proof expired".to_string());
        }

        // Mark as used (lock-free inserts)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.replay_data.insert(
            proof_hash,
            ReplayEntry {
                timestamp: now,
                peer_id: *peer_id,
                sequence,
            },
        );
        self.used_sequences.insert(*peer_id, sequence);

        debug!(
            "Payment proof accepted: peer_id={}, sequence={}, hash={:x?}",
            hex::encode(&peer_id[..8]),
            sequence,
            &proof_hash[..8]
        );

        Ok(true)
    }

    /// Clean up expired hashes
    ///
    /// Removes hashes that are older than expiry_seconds.
    /// Lock-free operation using DashMap - no mut needed.
    pub fn cleanup_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut expired_hashes = Vec::new();
        // Lock-free iteration
        for entry in self.replay_data.iter() {
            if now > entry.value().timestamp + self.expiry_seconds {
                expired_hashes.push(*entry.key());
            }
        }

        if !expired_hashes.is_empty() {
            debug!("Cleaning up {} expired payment proof hashes", expired_hashes.len());
            // Lock-free removal
            for hash in &expired_hashes {
                self.replay_data.remove(hash);
            }
        }
    }

    /// Get statistics about replay prevention
    ///
    /// Lock-free reads using DashMap - no mut needed.
    pub fn stats(&self) -> ReplayStats {
        ReplayStats {
            active_hashes: self.replay_data.len(),
            tracked_peers: self.used_sequences.len(),
            expiry_seconds: self.expiry_seconds,
        }
    }
}

/// Statistics about replay prevention
#[derive(Debug, Clone)]
pub struct ReplayStats {
    /// Number of active (non-expired) payment proof hashes
    pub active_hashes: usize,
    /// Number of tracked peers
    pub tracked_peers: usize,
    /// Expiry time in seconds
    pub expiry_seconds: u64,
}

