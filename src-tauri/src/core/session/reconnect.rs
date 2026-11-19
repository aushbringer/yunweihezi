//! Reconnect state machine with exponential backoff (P1 wiring).
//!
//! The monitor side of a session reports disconnects; this helper decides the
//! next retry delay. Kept standalone so SSH / serial / telnet sessions share
//! the same policy.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl ReconnectPolicy {
    /// Delay before attempt `attempt` (1-based). `None` when attempts exhausted.
    pub fn delay_for(&self, attempt: u32) -> Option<Duration> {
        if attempt == 0 || attempt > self.max_attempts {
            return None;
        }
        let shift = attempt.saturating_sub(1).min(10);
        let d = self.base_delay.saturating_mul(1u32 << shift);
        Some(d.min(self.max_delay))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let p = ReconnectPolicy::default();
        assert_eq!(p.delay_for(0), None);
        assert_eq!(p.delay_for(1), Some(Duration::from_secs(1)));
        assert_eq!(p.delay_for(2), Some(Duration::from_secs(2)));
        assert_eq!(p.delay_for(6), None); // max_attempts = 5
    }
}
