// SPDX-License-Identifier: MPL-2.0
//! `clock`: counters, uptime and wall-clock time.

/// `GetTime(0)`: raw monotonic ticks (the clocksource is in nanoseconds).
pub(crate) fn uptime_ticks() -> usize {
    hal::clock::now_ns() as usize
}

/// `GetTime(1)`: milliseconds since boot. The x86 clocksource already returns
/// nanoseconds; ÷ 1_000_000 → ms.
pub(crate) fn uptime_ms() -> usize {
    (hal::clock::now_ns() / 1_000_000) as usize
}

/// `GetTime(5)`: frequency of [`uptime_ticks`] in Hz.
pub(crate) fn tick_hz() -> usize {
    1_000_000_000usize // HPET is in nanoseconds (1 GHz)
}

/// Nanoseconds since the Unix epoch (wall clock).
pub(crate) fn epoch_ns() -> u64 {
    hal::rtc::now_epoch_ns()
}

/// Monotonic nanoseconds on the same clock `GetTime(0)` exposes to cells.
pub(crate) fn monotonic_ns() -> Option<u64> {
    Some(hal::clock::now_ns())
}
