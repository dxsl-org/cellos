/// CLINT (Core-Local Interruptor) Timer Interface
/// 
/// Provides access to the RISC-V timer (mtime/mtimecmp)

const CLINT_BASE: usize = 0x0200_0000;
const MTIME_OFFSET: usize = 0xbff8;
const MTIMECMP_OFFSET: usize = 0x4000;

/// Read the current machine time
pub fn read_mtime() -> u64 {
    unsafe {
        let mtime_addr = (CLINT_BASE + MTIME_OFFSET) as *const u64;
        core::ptr::read_volatile(mtime_addr)
    }
}

/// Set the timer compare value
pub fn write_mtimecmp(value: u64) {
    unsafe {
        let mtimecmp_addr = (CLINT_BASE + MTIMECMP_OFFSET) as *mut u64;
        core::ptr::write_volatile(mtimecmp_addr, value);
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
    write_mtimecmp(target);
}
