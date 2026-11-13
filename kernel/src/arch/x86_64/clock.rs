// SPDX-License-Identifier: MPL-2.0
//! `clock`: counters, uptime and wall-clock time.

/// Free-running counter for KASLR entropy.
pub(crate) fn counter() -> u64 {
    // RDTSC returns the current timestamp counter. On QEMU TCG this is a
    // monotonically increasing instruction count — sufficient for ASLR.
    let lo: u32;
    let hi: u32;
    // SAFETY: RDTSC is an unprivileged instruction on x86_64; it reads the
    // CPU time-stamp counter and has no memory or control-flow side-effects.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

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
