/// Timer Interface (S-mode)
/// 
/// Provides access to the RISC-V timer via SBI calls and 'time' CSR.
/// Direct CLINT access is NOT allowed in S-mode.

/// Read the current machine time (via 'time' CSR)
pub fn read_mtime() -> u64 {
    let time: u64;
    #[cfg(target_arch = "riscv64")]
    unsafe {
        // Read "time" CSR (0xC01) which mirrors mtime
        core::arch::asm!("csrr {0}, time", out(reg) time);
    }
    #[cfg(not(target_arch = "riscv64"))]
    { time = 0; }
    
    time
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
