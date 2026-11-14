//! Per-socket packet sequence enforcement on ingress.

use dashmap::DashMap;

/// Highest accepted sequence for a (sending peer, claimed source) pair.
///
/// The first key is the node id bound to the socket. A peer can put another
/// id in the source field, but that writes this pair, not the other node's
/// own counter.
pub struct PacketSequenceGuard {
    last_sequence: DashMap<([u8; 32], [u8; 32]), u64>,
}

impl Default for PacketSequenceGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketSequenceGuard {
    pub fn new() -> Self {
        Self {
            last_sequence: DashMap::new(),
        }
    }

    /// Accept a strictly increasing sequence for this socket and claimed source.
    pub fn check_and_record(
        &self,
        peer: &[u8; 32],
        source: &[u8; 32],
        sequence: u64,
    ) -> Result<(), String> {
        let key = (*peer, *source);
        if let Some(entry) = self.last_sequence.get(&key) {
            if sequence <= *entry.value() {
                return Err(format!(
                    "packet sequence out of order: got {}, last {}",
                    sequence,
                    entry.value()
                ));
            }
        }
        self.last_sequence.insert(key, sequence);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_sequence() {
        let guard = PacketSequenceGuard::new();
        let src = [1u8; 32];
        guard.check_and_record(&src, &src, 1).unwrap();
        assert!(guard.check_and_record(&src, &src, 1).is_err());
    }

    #[test]
    fn accepts_increasing_sequence() {
        let guard = PacketSequenceGuard::new();
        let src = [2u8; 32];
        guard.check_and_record(&src, &src, 10).unwrap();
        guard.check_and_record(&src, &src, 11).unwrap();
    }

    #[test]
    fn another_peers_claim_does_not_move_this_peers_counter() {
        let guard = PacketSequenceGuard::new();
        let victim = [2u8; 32];
        let attacker = [9u8; 32];
        guard
            .check_and_record(&attacker, &victim, u64::MAX)
            .unwrap();
        guard.check_and_record(&victim, &victim, 1).unwrap();
    }
}
