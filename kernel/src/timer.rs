/// Timer subsystem for ViOS
/// 
/// Provides timer initialization and preemptive scheduling support.
/// Uses RISC-V CLINT (Core-Local Interruptor) for timer interrupts.

use log::info;

/// Initialize the timer subsystem
/// 
/// Sets up periodic timer interrupts for preemptive multitasking.
/// On RISC-V, this configures the CLINT timer.
/// 
/// # Arguments
/// * `interval_ms` - Timer interrupt interval in milliseconds
/// 
/// # Safety
/// Must be called after trap handler initialization.
/// Should only be called once during kernel boot.
pub unsafe fn init(interval_ms: u64) {
    #[cfg(target_arch = "riscv64")]
    {
        extern crate hal_riscv;
        
        // Set the first timer interrupt
        hal_riscv::timer::set_timer_ms(interval_ms);
        
        info!("Timer initialized: {}ms interval", interval_ms);
    }
    
    #[cfg(not(target_arch = "riscv64"))]
    {
        info!("Timer initialized (simulation mode): {}ms interval", interval_ms);
    }
}

/// Get current system time in milliseconds
/// 
/// Returns the number of milliseconds since boot.
pub fn current_time_ms() -> u64 {
    #[cfg(target_arch = "riscv64")]
    {
        extern crate hal_riscv;
        hal_riscv::timer::time_ms()
    }
    
    #[cfg(not(target_arch = "riscv64"))]
    {
        0 // Simulation mode
    }
}

/// Get current raw timer value
/// 
/// Returns the raw mtime register value (cycles).
pub fn current_time_raw() -> u64 {
    #[cfg(target_arch = "riscv64")]
    {
        extern crate hal_riscv;
        hal_riscv::timer::read_mtime()
    }
    
    #[cfg(not(target_arch = "riscv64"))]
    {
        0 // Simulation mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_timer_functions() {
        // Just ensure they compile and don't panic
        let _time = current_time_ms();
        let _raw = current_time_raw();
    }
}
