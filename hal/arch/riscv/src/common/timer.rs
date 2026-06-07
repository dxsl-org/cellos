//! Timer Interface (S-mode)
//!
//! Provides access to the RISC-V timer via SBI calls and the `time` CSR.
//! Direct CLINT access is NOT available in S-mode — use SBI for `mtimecmp`.

/// Ticks per 10 ms at the assumed 10 MHz `mtime` clock on QEMU virt.
///
/// Used to set the preemptive timeslice duration.  If the actual clock
/// differs (detectable via DTB), callers should adjust accordingly.
pub const TICKS_PER_10MS: u64 = 100_000;

/// Read the current machine time (via 'time' CSR).
///
/// On RV32 the `time` CSR is only 32 bits wide; the upper 32 bits are in
/// `timeh`.  We use a carry-safe retry loop to avoid a torn 64-bit read
/// across a `timeh` overflow boundary.
pub fn read_mtime() -> u64 {
    #[cfg(target_arch = "riscv64")]
    {
        let time: u64;
        // SAFETY: reads the read-only `time` CSR (0xC01), no side effects.
        unsafe { core::arch::asm!("csrr {0}, time", out(reg) time, options(nomem, nostack)); }
        time
    }
    #[cfg(target_arch = "riscv32")]
    {
        // Retry if timeh changes between the two reads (i.e., the lower half
        // rolled over between the timeh read and the time read).
        loop {
            let hi1: u32;
            let lo: u32;
            let hi2: u32;
            // SAFETY: `timeh`/`time` are read-only user-mode CSRs; safe from S-mode.
            unsafe {
                core::arch::asm!("csrr {}, timeh", out(reg) hi1, options(nomem, nostack));
                core::arch::asm!("csrr {}, time",  out(reg) lo,  options(nomem, nostack));
                core::arch::asm!("csrr {}, timeh", out(reg) hi2, options(nomem, nostack));
            }
            if hi1 == hi2 {
                return ((hi1 as u64) << 32) | (lo as u64);
            }
        }
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    {
        0
    }
}

/// Get time in milliseconds (assuming 10MHz clock)
pub fn time_ms() -> u64 {
    read_mtime() / 10_000
}

/// Set a timer interrupt to fire after `ms` milliseconds
pub fn set_timer_ms(ms: u64) {
    let current = read_mtime();
    let target = current + (ms * 10_000);

    // Use SBI call to set timer in M-mode
    crate::common::sbi::set_timer(target);
}
