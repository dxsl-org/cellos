//! Goldfish RTC driver for AArch64 (QEMU virt machine).
//!
//! The SoC integration layer supplies the selected MMIO base during boot.

use core::sync::atomic::{AtomicUsize, Ordering};

static BASE: AtomicUsize = AtomicUsize::new(0);

/// Initialize with a specific MMIO base address (from DTB).
///
/// # Precondition
/// `base` must point to a valid 4 KB MMIO window.
pub fn init(base: usize) {
    BASE.store(base, Ordering::Release);
}

/// Nanoseconds since Unix epoch; `0` if RTC not initialized.
///
/// QEMU ARM virt uses the PL031 (ARM PrimeCell RTC) at this address.
/// PL031 RTCDR (offset 0x0) returns a 32-bit seconds count since the Unix
/// epoch (sourced from QEMU_CLOCK_REALTIME on the host).  Multiply by
/// 1_000_000_000 to convert to nanoseconds for the common hal::rtc contract.
pub fn now_epoch_ns() -> u64 {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    // SAFETY: base is a valid MMIO window; volatile read is non-aliasing.
    let secs = unsafe { core::ptr::read_volatile(base as *const u32) };
    secs as u64 * 1_000_000_000
}
