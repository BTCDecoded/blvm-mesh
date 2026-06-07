//! Per-source monotonic packet sequence enforcement on ingress.

use dashmap::DashMap;

/// Tracks the highest accepted sequence per source node id.
pub struct PacketSequenceGuard {
    last_sequence: DashMap<[u8; 32], u64>,
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

    /// Accept strictly increasing sequences from `source`.
    pub fn check_and_record(&self, source: &[u8; 32], sequence: u64) -> Result<(), String> {
        if let Some(entry) = self.last_sequence.get(source) {
            if sequence <= *entry.value() {
                return Err(format!(
                    "packet sequence out of order: got {}, last {}",
                    sequence,
                    entry.value()
                ));
            }
        }
        self.last_sequence.insert(*source, sequence);
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
        guard.check_and_record(&src, 1).unwrap();
        assert!(guard.check_and_record(&src, 1).is_err());
    }

    #[test]
    fn accepts_increasing_sequence() {
        let guard = PacketSequenceGuard::new();
        let src = [2u8; 32];
        guard.check_and_record(&src, 10).unwrap();
        guard.check_and_record(&src, 11).unwrap();
    }
}
