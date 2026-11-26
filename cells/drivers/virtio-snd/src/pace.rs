//! When the stream starts, what the next tx buffer carries, and when the
//! stream stops. Pure, host-tested.
//!
//! The device gets whole periods ([`PERIOD_BYTES`]); a partial period is sent
//! (padded with silence) only when nothing else can follow it soon: the
//! device has run dry, or the holder closed and this is the tail.

use crate::proto::PERIOD_BYTES;

/// Periods queued before START, so playback begins with a cushion.
pub const START_PERIODS: usize = 2;
/// A running stream with nothing to play for this long is stopped, even
/// with a holder (paused, or exited without CLOSE).
pub const IDLE_STOP_MS: u64 = 500;

/// Whether a stopped stream starts now, with `queued` bytes waiting.
/// `draining`: no client holds the stream.
pub fn should_start(queued: usize, draining: bool) -> bool {
    queued >= START_PERIODS * PERIOD_BYTES || (draining && queued > 0)
}

/// Client bytes the next tx buffer carries, or `None` to wait for more.
pub fn next_period(queued: usize, in_flight: usize, draining: bool) -> Option<usize> {
    if queued >= PERIOD_BYTES {
        Some(PERIOD_BYTES)
    } else if queued > 0 && (in_flight == 0 || draining) {
        Some(queued)
    } else {
        None
    }
}

/// Whether a running stream with nothing queued or in flight stops, after
/// `idle_ms` in that state.
pub fn should_stop(draining: bool, idle_ms: u64) -> bool {
    draining || idle_ms >= IDLE_STOP_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_waits_for_a_cushion_unless_draining() {
        assert!(!should_start(0, false));
        assert!(!should_start(2 * PERIOD_BYTES - 4, false));
        assert!(should_start(2 * PERIOD_BYTES, false));
        assert!(should_start(4, true));
        assert!(!should_start(0, true));
    }

    #[test]
    fn whole_periods_first_partial_only_when_dry_or_draining() {
        assert_eq!(next_period(5000, 3, false), Some(PERIOD_BYTES));
        assert_eq!(next_period(100, 3, false), None);
        assert_eq!(next_period(100, 0, false), Some(100));
        assert_eq!(next_period(100, 3, true), Some(100));
        assert_eq!(next_period(0, 0, true), None);
    }

    #[test]
    fn idle_stream_stops_at_close_or_after_the_idle_limit() {
        assert!(should_stop(true, 0));
        assert!(!should_stop(false, IDLE_STOP_MS - 1));
        assert!(should_stop(false, IDLE_STOP_MS));
    }
}
