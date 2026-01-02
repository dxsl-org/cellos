#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "std"), no_main)]
#![allow(static_mut_refs)]

#[cfg(not(feature = "std"))]
use core::panic::PanicInfo;

use kernel::init;

/// THE NUCLEUS (ViOS Microkernel)
/// Feature="std": Runs as a process on Host OS (for Simulation/Testing)
/// Feature="no_std": Runs as Bare Metal Kernel (on Robot/Server hardware)

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::error!("KERNEL PANIC: {}", info);
    loop {}
}

#[cfg(not(feature = "std"))]
#[no_mangle]
pub extern "C" fn kmain() -> ! {
    // DEBUG: Write 'V' to UART0 (0x10000000) directly
    unsafe {
        core::ptr::write_volatile(0x1000_0000 as *mut u8, b'V');
    }

    // Called from hal-riscv boot.s
    init();
    
    // Initialize trap handler for interrupt handling
    unsafe {
        kernel::arch::trap::init();
        log::info!("Trap handler initialized");
    }
    
    // Initialize timer for preemptive multitasking
    // Timer will fire every 10ms
    unsafe {
        kernel::timer::init(10);
        log::info!("Timer initialized (10ms interval)");
    }
    
    // Enable interrupts globally
    unsafe {
        kernel::arch::trap::enable_interrupts();
        log::info!("Interrupts enabled");
    }
    
    log::info!("Kernel initialized, entering scheduler loop...");
    
    // Main Scheduler Loop
    // This is the kernel's idle task - runs when no other tasks are ready
    let mut cycle_count = 0u64;
    let mut last_stats_cycle = 0u64;
    
    loop {
        // Check if we have any tasks to run
        if kernel::process::has_ready_tasks() {
            // Schedule next task
            kernel::process::yield_cpu();
            cycle_count += 1;
        } else {
            // No tasks ready - we're truly idle
            // Log stats periodically
            if cycle_count > 0 && cycle_count != last_stats_cycle {
                let (total, ready) = kernel::process::scheduler_stats();
                log::info!("Scheduler: {} cycles, {} total tasks, {} ready", 
                    cycle_count, total, ready);
                last_stats_cycle = cycle_count;
            }
            
            // Wait for interrupt (timer, IPC, etc)
            unsafe {
                core::arch::asm!("wfi");
            }
            continue;
        }
        
        // Every 10000 cycles, log status
        if cycle_count % 10000 == 0 {
            let (total, ready) = kernel::process::scheduler_stats();
            log::debug!("Scheduler: {} cycles, {} tasks ({} ready)", 
                cycle_count, total, ready);
        }
    }
}

#[cfg(feature = "std")]
fn main() {
    println!(">>> ViOS Bootloader v0.1 (Simulated) <<<");
    init();
    
    println!(">>> Starting Scheduler Loop (5 cycles) <<<");
    for i in 0..5 {
        println!("--- Cycle {} ---", i);
        kernel::process::yield_cpu();
        // Simulate some work or time passing
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    
    println!(">>> System Halting <<<");
}
