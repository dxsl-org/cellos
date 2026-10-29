//! Time budgets for the driver's waits (Phase 10 S10-3c, D3c-1 A).
//!
//! Every wait used to end after a fixed number of polls. A poll is one volatile read and the
//! loop yields every [`POLL_YIELD_EVERY`] polls, but a yield returns at once when nothing else
//! is runnable, so the same count lasted a few seconds in an idle guest and much longer during
//! a busy boot: a Flush (a host `fsync`) could outlast it. The budgets are now milliseconds on
//! the kernel's monotonic clock (`GetTime`), read only at the yield cadence. Without a clock the
//! old poll count stays the bound.

use core::fmt;

/// Yield every this many polls so a long wait does not starve the rest of the system (the
/// driver Cell shares the machine with the VFS and the guest). The clock is read here too.
pub(crate) const POLL_YIELD_EVERY: u64 = 8192;
/// The bound when no monotonic clock is available (the former budget).
pub(crate) const FALLBACK_POLLS: u64 = 50_000_000;
/// Admin commands (Identify, Create CQ/SQ): Linux's `nvme_core.admin_timeout` default.
pub(crate) const ADMIN_BUDGET_MS: u64 = 60_000;
/// I/O commands, including Flush: Linux's `nvme_core.io_timeout` default.
pub(crate) const IO_BUDGET_MS: u64 = 30_000;
/// Floor of the `CSTS.RDY` budget when the controller reports `CAP.TO = 0`.
pub(crate) const RDY_FLOOR_MS: u64 = 1_000;
/// A completion this slow is logged, so evidence shows how close a run came to a budget.
pub(crate) const SLOW_COMPLETION_MS: u64 = 1_000;

/// The `CSTS.RDY` transition budget: `CAP.TO` (bits 31:24) in 500 ms units, the
/// specification's worst case, with a floor of [`RDY_FLOOR_MS`].
pub(crate) fn rdy_budget_ms(cap: u64) -> u64 {
    (((cap >> 24) & 0xFF) * 500).max(RDY_FLOOR_MS)
}

/// How much of a budget a wait used when it gave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spent {
    Millis(u64),
    /// No monotonic clock: the poll-count bound applied.
    Polls(u64),
}

impl fmt::Display for Spent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Spent::Millis(ms) => write!(f, "{} ms", ms),
            Spent::Polls(polls) => write!(f, "{} polls (no clock)", polls),
        }
    }
}

/// One wait: its start on the monotonic clock (if any), its budget and its poll count.
pub(crate) struct Deadline {
    start_ms: Option<u64>,
    budget_ms: u64,
    polls: u64,
}

impl Deadline {
    /// A wait of `budget_ms` starting at `now` (`None`: no clock).
    pub(crate) fn start(budget_ms: u64, now: Option<u64>) -> Self {
        Self { start_ms: now, budget_ms, polls: 0 }
    }

    /// Count one poll; `true` when this poll is a yield point, where the caller yields and
    /// then calls [`check`](Self::check).
    pub(crate) fn tick(&mut self) -> bool {
        self.polls += 1;
        self.polls % POLL_YIELD_EVERY == 0
    }

    /// `Some` once the budget is spent at `now`: elapsed milliseconds with a clock, the poll
    /// count without one.
    pub(crate) fn check(&self, now: Option<u64>) -> Option<Spent> {
        match (self.start_ms, now) {
            (Some(start), Some(now)) => {
                let elapsed = now.saturating_sub(start);
                (elapsed >= self.budget_ms).then_some(Spent::Millis(elapsed))
            }
            _ => (self.polls >= FALLBACK_POLLS).then_some(Spent::Polls(self.polls)),
        }
    }

    /// Elapsed milliseconds at `now` when at least [`SLOW_COMPLETION_MS`]; `None` otherwise
    /// or without a clock.
    pub(crate) fn slow(&self, now: Option<u64>) -> Option<u64> {
        let elapsed = now?.saturating_sub(self.start_ms?);
        (elapsed >= SLOW_COMPLETION_MS).then_some(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_expires_at_its_budget_and_not_before() {
        let deadline = Deadline::start(IO_BUDGET_MS, Some(1_000));
        assert_eq!(deadline.check(Some(1_000)), None);
        assert_eq!(deadline.check(Some(1_000 + IO_BUDGET_MS - 1)), None);
        assert_eq!(deadline.check(Some(1_000 + IO_BUDGET_MS)), Some(Spent::Millis(IO_BUDGET_MS)));
        // A clock that went backwards is no time at all, never an early expiry.
        assert_eq!(deadline.check(Some(10)), None);
    }

    #[test]
    fn many_fast_polls_do_not_expire_a_clocked_wait() {
        let mut deadline = Deadline::start(ADMIN_BUDGET_MS, Some(0));
        deadline.polls = FALLBACK_POLLS;
        deadline.tick();
        assert_eq!(deadline.check(Some(ADMIN_BUDGET_MS - 1)), None);
    }

    #[test]
    fn without_a_clock_the_poll_count_is_the_bound() {
        for (start, now) in [(None, None), (Some(5), None), (None, Some(5))] {
            let mut deadline = Deadline::start(IO_BUDGET_MS, start);
            deadline.polls = FALLBACK_POLLS - 2;
            deadline.tick();
            assert_eq!(deadline.check(now), None);
            deadline.tick();
            assert_eq!(deadline.check(now), Some(Spent::Polls(FALLBACK_POLLS)));
        }
    }

    #[test]
    fn a_yield_point_every_8192_polls() {
        let mut deadline = Deadline::start(IO_BUDGET_MS, Some(0));
        let yields = (0..3 * POLL_YIELD_EVERY).filter(|_| deadline.tick()).count();
        assert_eq!(yields, 3);
        assert_eq!(deadline.polls, 3 * POLL_YIELD_EVERY);
    }

    #[test]
    fn rdy_budget_follows_cap_to_with_a_floor() {
        assert_eq!(rdy_budget_ms(0), RDY_FLOOR_MS);
        assert_eq!(rdy_budget_ms(1 << 24), RDY_FLOOR_MS);
        assert_eq!(rdy_budget_ms(0xF << 24), 7_500);
        assert_eq!(rdy_budget_ms((0xFF << 24) | 0xFFFF), 127_500);
        // Bits outside CAP.TO do not count.
        assert_eq!(rdy_budget_ms(0xFFFF_FFFF_00FF_FFFF), RDY_FLOOR_MS);
    }

    #[test]
    fn slow_completions_are_reported_from_one_second() {
        let deadline = Deadline::start(IO_BUDGET_MS, Some(100));
        assert_eq!(deadline.slow(Some(100 + SLOW_COMPLETION_MS - 1)), None);
        assert_eq!(deadline.slow(Some(100 + SLOW_COMPLETION_MS)), Some(SLOW_COMPLETION_MS));
        assert_eq!(deadline.slow(None), None);
        assert_eq!(Deadline::start(IO_BUDGET_MS, None).slow(Some(5_000)), None);
        assert_eq!(alloc::format!("{}", Spent::Millis(30_000)), "30000 ms");
        assert_eq!(alloc::format!("{}", Spent::Polls(7)), "7 polls (no clock)");
    }
}
