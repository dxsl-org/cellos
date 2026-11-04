//! Time budget for one polled command completion.
//!
//! The budget is milliseconds on the kernel's monotonic clock (`GetTime`),
//! read only at the yield cadence; without a clock a poll count bounds the
//! wait instead. Same scheme as the NVMe cell's `deadline` module.

/// Per-command budget, including SYNCHRONIZE CACHE (a host `fsync`).
pub const IO_BUDGET_MS: u64 = 30_000;
/// Yield (and read the clock) every this many polls so a long wait does not
/// starve the rest of the system.
const POLL_YIELD_EVERY: u64 = 1024;
/// The bound when no monotonic clock is available.
const FALLBACK_POLLS: u64 = 50_000_000;

pub struct Deadline {
    start_ms: Option<u64>,
    budget_ms: u64,
    polls: u64,
}

impl Deadline {
    /// A wait of `budget_ms` starting at `now` (`None`: no clock).
    pub fn start(budget_ms: u64, now: Option<u64>) -> Self {
        Self {
            start_ms: now,
            budget_ms,
            polls: 0,
        }
    }

    /// Counts one poll; `true` at a yield point, where the caller yields and
    /// then calls [`Deadline::expired`].
    pub fn tick(&mut self) -> bool {
        self.polls += 1;
        self.polls.is_multiple_of(POLL_YIELD_EVERY)
    }

    /// Whether the budget is spent at `now`. A clock that disappears
    /// mid-wait falls back to the poll bound.
    pub fn expired(&self, now: Option<u64>) -> bool {
        match (self.start_ms, now) {
            (Some(start), Some(now)) => now.saturating_sub(start) >= self.budget_ms,
            _ => self.polls >= FALLBACK_POLLS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_budget_expires_at_budget() {
        let deadline = Deadline::start(100, Some(1_000));
        assert!(!deadline.expired(Some(1_099)));
        assert!(deadline.expired(Some(1_100)));
        assert!(
            !deadline.expired(Some(500)),
            "a clock going backwards never expires early"
        );
    }

    #[test]
    fn without_clock_the_poll_count_bounds_the_wait() {
        let mut deadline = Deadline::start(100, None);
        assert!(!deadline.expired(None));
        deadline.polls = FALLBACK_POLLS;
        assert!(deadline.expired(None));
    }

    #[test]
    fn yield_points_follow_the_cadence() {
        let mut deadline = Deadline::start(100, None);
        let yields = (0..3 * POLL_YIELD_EVERY)
            .filter(|_| deadline.tick())
            .count();
        assert_eq!(yields, 3);
    }
}
