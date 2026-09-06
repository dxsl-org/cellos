//! High-resolution timer for benchmark measurements.
//!
//! Reads the kernel's monotonic tick counter via `sys_get_time`.  On RV64 QEMU
//! this corresponds to `mtime` at ~10 MHz.  Convert ticks → ns by dividing by
//! `ticks_per_ns()` (queries the Config Cell; falls back to 10 MHz assumed).

use ostd::syscall::{sys_get_time, sys_get_timer_freq};

/// Timer frequency assumed when dynamic query is unavailable (10 MHz).
const FALLBACK_FREQ_HZ: u64 = 10_000_000;

/// Return the timer frequency in Hz.
pub fn timer_freq_hz() -> u64 {
    sys_get_timer_freq().unwrap_or(FALLBACK_FREQ_HZ)
}

/// Nanoseconds per tick — fallback constant for compatibility.
pub const NS_PER_TICK: u64 = 1_000_000_000 / FALLBACK_FREQ_HZ;

/// Read the current tick counter value.
///
/// Calling this twice and subtracting gives a tick delta.  Use `ticks_to_ns`
/// to convert to nanoseconds.
#[inline(always)]
pub fn read_ticks() -> u64 {
    sys_get_time()
}

/// Convert a tick delta to nanoseconds using the fallback frequency.
///
/// For accurate results on real hardware, replace `NS_PER_TICK` with a value
/// queried from the Config Cell (`system.timer_freq_hz`).
#[inline(always)]
pub fn ticks_to_ns(ticks: u64) -> u64 {
    let freq = timer_freq_hz();
    if freq == 0 {
        return ticks.saturating_mul(NS_PER_TICK);
    }
    ((ticks as u128).saturating_mul(1_000_000_000) / (freq as u128)) as u64
}
