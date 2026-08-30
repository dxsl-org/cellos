// SPDX-License-Identifier: Apache-2.0
//! Bounded reconnect timing for the future authenticated relay client.

/// First exponential-backoff ceiling after a relay connection failure.
pub const RELAY_RECONNECT_INITIAL_CEILING_MS: u32 = 1_000;
/// Maximum exponential-backoff ceiling after repeated relay failures.
pub const RELAY_RECONNECT_MAX_CEILING_MS: u32 = 30_000;

/// Equal-jitter exponential backoff for relay reconnect attempts.
///
/// Call [`Self::next_delay_ms`] exactly once after each failed connection or
/// disconnect. Call [`Self::record_authenticated_session`] only after relay
/// authentication succeeds. The returned delay is always in the upper half of
/// exponential window, preventing a hot reconnect loop while independent
/// caller-provided samples desynchronize nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayReconnectBackoff {
    ceiling_ms: u32,
}

impl RelayReconnectBackoff {
    /// Start at the one-second reconnect ceiling.
    pub const fn new() -> Self {
        Self {
            ceiling_ms: RELAY_RECONNECT_INITIAL_CEILING_MS,
        }
    }

    /// Return a jittered delay and advance the exponential window.
    ///
    /// `random` must be an unbiased caller-provided `u32` sample. The result is
    /// within `ceiling / 2..ceiling`; the ceiling doubles until the 30-second
    /// cap. Multiply-high scaling distributes the full input space across that
    /// half-open range without an almost-unreachable endpoint bucket.
    pub fn next_delay_ms(&mut self, random: u32) -> u32 {
        let floor_ms = self.ceiling_ms / 2;
        let width_ms = self.ceiling_ms - floor_ms;
        let offset_ms = ((u64::from(random) * u64::from(width_ms)) >> 32) as u32;
        let delay_ms = floor_ms + offset_ms;
        self.ceiling_ms = self
            .ceiling_ms
            .saturating_mul(2)
            .min(RELAY_RECONNECT_MAX_CEILING_MS);
        delay_ms
    }

    /// Reset the sequence after relay authentication succeeds.
    pub fn record_authenticated_session(&mut self) {
        self.ceiling_ms = RELAY_RECONNECT_INITIAL_CEILING_MS;
    }
}

impl Default for RelayReconnectBackoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_jitter_grows_and_saturates_without_hot_loop() {
        let mut backoff = RelayReconnectBackoff::new();
        let delays = core::array::from_fn::<_, 7, _>(|_| backoff.next_delay_ms(0));
        assert_eq!(delays, [500, 1_000, 2_000, 4_000, 8_000, 15_000, 15_000]);
    }

    #[test]
    fn maximum_jitter_never_exceeds_cap() {
        let mut backoff = RelayReconnectBackoff::new();
        let delays = core::array::from_fn::<_, 7, _>(|_| backoff.next_delay_ms(u32::MAX));
        assert_eq!(delays, [999, 1_999, 3_999, 7_999, 15_999, 29_999, 29_999]);
    }

    #[test]
    fn midpoint_sample_maps_to_midpoint_of_half_open_window() {
        let mut backoff = RelayReconnectBackoff::new();
        assert_eq!(backoff.next_delay_ms(1 << 31), 750);
    }

    #[test]
    fn authenticated_session_resets_the_sequence() {
        let mut backoff = RelayReconnectBackoff::new();
        for _ in 0..6 {
            backoff.next_delay_ms(0);
        }
        backoff.record_authenticated_session();
        assert_eq!(backoff.next_delay_ms(0), 500);
    }
}
