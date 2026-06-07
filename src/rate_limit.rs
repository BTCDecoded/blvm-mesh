//! Per-peer ingress rate limiting.

use dashmap::DashMap;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// Sliding-window rate limiter keyed by peer identifier (address or node id hex).
pub struct RateLimiter {
    windows: DashMap<String, VecDeque<u64>>,
    max_per_window: u32,
    window_secs: u64,
}

impl RateLimiter {
    /// Create a limiter. `max_per_window == 0` disables limiting.
    pub fn new(max_per_window: u32, window_secs: u64) -> Self {
        Self {
            windows: DashMap::new(),
            max_per_window,
            window_secs,
        }
    }

    pub fn disabled() -> Self {
        Self::new(0, 60)
    }

    pub fn is_enabled(&self) -> bool {
        self.max_per_window > 0
    }

    /// Record one event; returns Err when over limit.
    pub fn check_and_record(&self, key: &str) -> Result<(), String> {
        if !self.is_enabled() {
            return Ok(());
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let cutoff = now.saturating_sub(self.window_secs);

        let mut entry = self.windows.entry(key.to_string()).or_default();
        while entry.front().is_some_and(|&t| t < cutoff) {
            entry.pop_front();
        }

        if entry.len() >= self.max_per_window as usize {
            return Err(format!(
                "rate limit exceeded: {} events per {}s",
                self.max_per_window, self.window_secs
            ));
        }

        entry.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_limiter_always_allows() {
        let limiter = RateLimiter::disabled();
        for _ in 0..1000 {
            limiter.check_and_record("peer").unwrap();
        }
    }

    #[test]
    fn enforces_window_capacity() {
        let limiter = RateLimiter::new(2, 60);
        limiter.check_and_record("peer").unwrap();
        limiter.check_and_record("peer").unwrap();
        assert!(limiter.check_and_record("peer").is_err());
    }
}
