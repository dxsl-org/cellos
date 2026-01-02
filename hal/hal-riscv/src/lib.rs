#![no_std]

use hal_core::HalResult;

/// RISC-V Platform Initialization
pub mod boot;
pub mod timer;

/// Initialize RISC-V platform
pub fn init() -> HalResult<()> {
    // Platform-specific initialization
    log::info!("HAL-RISC-V: Initializing...");
    
    // TODO: Initialize PLIC, CLINT, etc.
    
    Ok(())
}

/// Get current time in milliseconds (from CLINT)
pub fn time_ms() -> u64 {
    // TODO: Read mtime register
    0
}

/// Halt the CPU
pub fn halt() -> ! {
    loop {
        unsafe {
            // WFI: Wait For Interrupt
            core::arch::asm!("wfi");
        }
    }
}
