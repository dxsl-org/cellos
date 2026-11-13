// SPDX-License-Identifier: MPL-2.0
//! `clock`: counters, uptime and wall-clock time.

/// Free-running counter for KASLR entropy.
pub(crate) fn counter() -> u64 {
    // SAFETY: `csrr time` reads the mtime-backed TIME CSR (unprivileged read
    // permitted in S-mode when sstatus.TVM=0, which is always the case here).
    let t: u64;
    unsafe {
        core::arch::asm!("csrr {}, time", out(reg) t, options(nomem, nostack));
    }
    t
}

/// `GetTime(0)`: raw monotonic ticks (mtime).
pub(crate) fn uptime_ticks() -> usize {
    hal::common::timer::read_mtime() as usize
}

/// `GetTime(1)`: milliseconds since boot. 10 MHz mtime on QEMU RV64 → 10_000 ticks/ms.
pub(crate) fn uptime_ms() -> usize {
    (hal::common::timer::read_mtime() / 10_000) as usize
}

/// `GetTime(5)`: frequency of [`uptime_ticks`] in Hz.
pub(crate) fn tick_hz() -> usize {
    10_000_000usize
}

/// Nanoseconds since the Unix epoch (wall clock).
pub(crate) fn epoch_ns() -> u64 {
    hal::common::rtc::now_epoch_ns()
}

/// Monotonic nanoseconds on the same clock `GetTime(0)` exposes to cells.
pub(crate) fn monotonic_ns() -> Option<u64> {
    hal::common::timer::read_mtime().checked_mul(100)
}

/// Enable the S-mode timer interrupt and arm this hart's first 10 ms tick.
pub(crate) fn start_tick() {
    // SAFETY: sets STIE (bit 5 = mask 0x20) in sie from S-mode. Must use the
    // register form of csrs — csrsi's immediate is only 5 bits (0..=31), so a
    // 0x20 mask cannot be encoded as an immediate.
    unsafe {
        core::arch::asm!("csrs sie, {stie}", stie = in(reg) 0x20usize);
    }
    let next = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
    hal::common::sbi::set_timer(next);
}
