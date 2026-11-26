//! The output stream's playback clock: how many bytes it has played,
//! advancing at the fixed format's byte rate while it runs.
//!
//! A tx buffer completes once the clock has played past its last byte, so the
//! guest's audio clock follows the host's (`sys_get_time`), whether or not an
//! audio service consumes the bytes. The clock never runs past the end of what
//! the guest queued: a stream that ran dry plays its next buffer from when the
//! clock is next advanced, instead of completing it at once to make up for
//! the gap.

/// Bytes per second of S16LE stereo at 48 kHz.
pub const BYTES_PER_SEC: u64 = wire::audio::RATE as u64 * wire::audio::FRAME_BYTES as u64;
const NS_PER_SEC: u128 = 1_000_000_000;

#[derive(Default)]
pub struct PlayClock {
    /// Bytes played up to `since_ns`.
    base: u64,
    /// When the clock last started or re-anchored; `None` while stopped.
    since_ns: Option<u64>,
}

impl PlayClock {
    pub const fn new() -> Self {
        Self {
            base: 0,
            since_ns: None,
        }
    }

    /// Run from `now_ns`; a running clock keeps its pace.
    pub fn start(&mut self, now_ns: u64) {
        self.since_ns.get_or_insert(now_ns);
    }

    /// Freeze at what was played by `now_ns` (at most `limit`).
    pub fn stop(&mut self, now_ns: u64, limit: u64) {
        self.base = self.advance(now_ns, limit);
        self.since_ns = None;
    }

    /// Bytes played by `now_ns`, at most `limit`, the end of what the guest
    /// queued. Reaching it re-anchors the clock there.
    pub fn advance(&mut self, now_ns: u64, limit: u64) -> u64 {
        let Some(since_ns) = self.since_ns else {
            return self.base.min(limit);
        };
        let elapsed = u128::from(now_ns.saturating_sub(since_ns));
        let played = elapsed * u128::from(BYTES_PER_SEC) / NS_PER_SEC;
        let position = u128::from(self.base) + played;
        if position >= u128::from(limit) {
            self.base = limit;
            self.since_ns = Some(now_ns);
            return limit;
        }
        position as u64
    }

    /// Back to zero, stopped (stream release, device reset).
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;
    /// 100 ms of audio.
    const PERIOD: u64 = 19_200;

    #[test]
    fn a_buffer_plays_for_its_duration() {
        let mut clock = PlayClock::new();
        assert_eq!(clock.advance(5 * MS, PERIOD), 0);
        clock.start(1_000 * MS);
        assert_eq!(clock.advance(1_050 * MS, 2 * PERIOD), PERIOD / 2);
        assert!(clock.advance(1_099 * MS, 2 * PERIOD) < PERIOD);
        assert_eq!(clock.advance(1_100 * MS, 2 * PERIOD), PERIOD);
        assert_eq!(clock.advance(1_200 * MS, 2 * PERIOD), 2 * PERIOD);
    }

    #[test]
    fn stop_freezes_and_restart_resumes() {
        let mut clock = PlayClock::new();
        clock.start(0);
        clock.stop(50 * MS, 4 * PERIOD);
        assert_eq!(clock.advance(500 * MS, 4 * PERIOD), PERIOD / 2);
        clock.start(1_000 * MS);
        assert_eq!(clock.advance(1_050 * MS, 4 * PERIOD), PERIOD);
        // Starting a running clock does not move it.
        clock.start(1_060 * MS);
        assert_eq!(clock.advance(1_100 * MS, 4 * PERIOD), 3 * PERIOD / 2);
    }

    #[test]
    fn a_dry_stream_does_not_bank_time() {
        let mut clock = PlayClock::new();
        clock.start(0);
        assert_eq!(clock.advance(300 * MS, PERIOD), PERIOD);
        // The next buffer arrives late: it plays from now, for its duration.
        assert_eq!(clock.advance(400 * MS, PERIOD), PERIOD);
        assert_eq!(clock.advance(450 * MS, 2 * PERIOD), 3 * PERIOD / 2);
        assert_eq!(clock.advance(500 * MS, 2 * PERIOD), 2 * PERIOD);
    }

    #[test]
    fn time_going_backwards_plays_nothing() {
        let mut clock = PlayClock::new();
        clock.start(100 * MS);
        assert_eq!(clock.advance(50 * MS, PERIOD), 0);
        clock.reset();
        assert_eq!(clock.advance(200 * MS, PERIOD), 0);
    }
}
