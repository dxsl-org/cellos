// SPDX-License-Identifier: MPL-2.0
//! `clock` for the 32-bit targets: no counter or wall clock is wired up.

/// No hardware timer available; use a fixed diversifier.
/// This means KASLR is purely seed-based (hhdm_offset) on these arches.
pub(crate) fn counter() -> u64 {
    0xDEAD_BEEF_CAFE_1234u64
}

pub(crate) fn uptime_ticks() -> usize {
    0usize
}

pub(crate) fn uptime_ms() -> usize {
    0usize
}

pub(crate) fn tick_hz() -> usize {
    10_000_000usize
}

pub(crate) fn epoch_ns() -> u64 {
    0
}

pub(crate) fn monotonic_ns() -> Option<u64> {
    None
}

/// Enable the S-mode timer interrupt and arm this hart's first 10 ms tick.
#[cfg(target_arch = "riscv32")]
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
