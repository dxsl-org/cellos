// SPDX-License-Identifier: MPL-2.0
//! `clock`: counters, uptime and wall-clock time.

/// `CNTPCT_EL0`, the physical counter.
pub(crate) fn physical_counter() -> u64 {
    let ticks: u64;
    // SAFETY: CNTPCT_EL0 is a read-only architectural counter available at EL1/EL2.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) ticks, options(nomem, nostack));
    }
    ticks
}

/// `CNTFRQ_EL0`, the counter frequency in Hz.
pub(crate) fn counter_frequency() -> u64 {
    let frequency: u64;
    // SAFETY: CNTFRQ_EL0 is a read-only architectural frequency register.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack));
    }
    frequency
}

/// `GetTime(0)`: raw monotonic ticks (generic timer).
pub(crate) fn uptime_ticks() -> usize {
    hal::timer::read_ticks() as usize
}

/// `GetTime(1)`: milliseconds since boot. Dynamic timer frequency on ARM64
/// (19.2 MHz on RPi3, 62.5 MHz on QEMU virt).
pub(crate) fn uptime_ms() -> usize {
    let freq = hal::timer::read_freq();
    let ticks_per_ms = if freq > 0 { freq / 1000 } else { 62_500 };
    (hal::timer::read_ticks() / ticks_per_ms) as usize
}

/// `GetTime(5)`: frequency of [`uptime_ticks`] in Hz.
pub(crate) fn tick_hz() -> usize {
    hal::timer::read_freq() as usize
}

/// Nanoseconds since the Unix epoch (wall clock).
pub(crate) fn epoch_ns() -> u64 {
    hal::rtc::now_epoch_ns()
}

/// Monotonic nanoseconds on the same clock `GetTime(0)` exposes to cells.
pub(crate) fn monotonic_ns() -> Option<u64> {
    let freq = hal::timer::read_freq();
    if freq == 0 {
        return None;
    }
    u64::try_from(u128::from(hal::timer::read_ticks()) * 1_000_000_000u128 / u128::from(freq)).ok()
}

/// `CNTP_CTL_EL0`, the EL1 physical timer control (Pi monitor diagnostics).
pub(crate) fn cntp_ctl() -> u64 {
    // SAFETY: reading CNTP_CTL_EL0 has no side effect.
    unsafe {
        let v: u64;
        core::arch::asm!("mrs {v}, cntp_ctl_el0",
                         v = out(reg) v, options(nomem, nostack));
        v
    }
}
