//! Receiver-initiated bounded challenge rounds and telemetry freshness.

pub struct ChallengeRound {
    pub nonce: u64,
    pub start_time_ms: u64,
    pub max_lifetime_ms: u64,
    pub completed: bool,
}

impl ChallengeRound {
    pub fn new(nonce: u64, start_time_ms: u64, max_lifetime_ms: u64) -> Self {
        Self {
            nonce,
            start_time_ms,
            max_lifetime_ms,
            completed: false,
        }
    }

    /// Check if the challenge is still valid at `current_time_ms`.
    /// TTL runs strictly from start_time_ms, never arrival time.
    pub fn is_valid(&self, current_time_ms: u64) -> bool {
        !self.completed && current_time_ms < self.start_time_ms.saturating_add(self.max_lifetime_ms)
    }

    pub fn complete(&mut self) -> Result<(), ()> {
        if self.completed {
            return Err(());
        }
        self.completed = true;
        Ok(())
    }
}
