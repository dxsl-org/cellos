//! Time budget for one polled TX completion.
//!
//! The budget is milliseconds on the kernel's monotonic clock (`GetTime`),
//! read only at the yield cadence; without a clock a poll count bounds the
//! wait instead. Same scheme as the virtio-scsi and NVMe cells.

/// Budget for the device to consume one transmitted frame.
pub const TX_BUDGET_MS: u64 = 5_000;
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
