#![cfg_attr(not(feature = "std"), no_std)]
#![allow(static_mut_refs)]

extern crate alloc;

// Kernel prelude module - see docs/KERNEL_PRELUDE_POLICY.md
pub mod prelude;

#[cfg(target_arch = "riscv64")]
extern crate hal_riscv;

pub mod arch;
pub mod memory;
pub mod loader;
pub mod fs;
pub mod process;
pub mod sync;
pub mod timer;

pub use prelude::*;
use log::{info, warn, error, debug};

// Versioning Constants
pub const ERA_NAME: &str = "Mycelium";
pub const SYSTEM_STATE: &str = "Alpha Test";
pub const SYSTEM_UPDATE: &str = "2026.01";

/// Main Entry Point for the Kernel Initialization
pub fn init() {
    // 1. Init Logger
    init_logger();
    
    info!("==================================================");
    info!("   ViOS {} ({} - Update {})", ERA_NAME, SYSTEM_STATE, SYSTEM_UPDATE);
    info!("   Architecture: RISC-V 64 / Target: Bare Metal");
    info!("==================================================");
    info!("ViOS System Initializing...");

    // Ensure HAL is linked and initialized
    #[cfg(target_arch = "riscv64")]
    if let Err(e) = hal_riscv::init() {
        log::error!("Failed to init HAL: {:?}", e);
    }

    // 2. Init Memory (Heap)
    memory::init();

    // 2.5 Init Filesystem
    info!("VFS: Initializing...");
    fs::init();

    // 3. Init Process Manager (Scheduler)
    process::init();

    // 3.5 Mount Filesystems (Drivers are ready now)
    fs::mount_all();

    // 4. Init Loader (Prepare for Cells)
    loader::init();

    info!("ViOS Core Services Ready.");
}

#[cfg(feature = "std")]
fn init_logger() {
    // In simulation, we use a simple print logger or env_logger
    // For now, simple print is enough strictly for demo, but let's adhere to "log" crate
    struct SimpleLogger;
    impl log::Log for SimpleLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool { true }
        fn log(&self, record: &log::Record) {
            println!("[{}] {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
    static LOGGER: SimpleLogger = SimpleLogger;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug));
}

#[cfg(not(feature = "std"))]
mod bare_metal_logger {
    use log::{Record, Metadata};
    use crate::sync::Spinlock;
    use hal_uart::Ns16550a;
    use hal_core::uart::SerialPort;
    use core::fmt::Write;
    use crate::prelude::*;

    // QEMU Virt UART0 is at 0x10000000
    // We wrap it in a Spinlock for thread safety
    static SERIAL: Spinlock<Option<Ns16550a>> = Spinlock::new(None);

    struct SerialWrapper<'a>(&'a mut Ns16550a);

    impl<'a> core::fmt::Write for SerialWrapper<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.bytes() {
                // Ignore errors (what can we do if logging fails?)
                let _ = self.0.send(byte);
            }
            Ok(())
        }
    }

    pub struct SimpleLogger;

    impl log::Log for SimpleLogger {
        fn enabled(&self, _metadata: &Metadata) -> bool { true }

        fn log(&self, record: &Record) {
            if let Some(guard) = SERIAL.lock().as_mut() {
                 let mut wrapper = SerialWrapper(&mut *guard);
                 // "\r\n" for compatibility with raw terminals
                 let _ = write!(wrapper, "[{}] {}\r\n", record.level(), record.args());
            }
        }

        fn flush(&self) {}
    }

    pub fn init() {
        unsafe {
            // SAFETY: 0x1000_0000 is the hardcoded UART base for QEMU RISC-V Virt.
            let mut serial = Ns16550a::new(0x1000_0000); 
            if serial.init().is_ok() {
                *SERIAL.lock() = Some(serial);
            }
        }
        
        static LOGGER: SimpleLogger = SimpleLogger;
        // Set max level to Trace for debugging
        let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Trace));
    }
}

#[cfg(not(feature = "std"))]
fn init_logger() {
    bare_metal_logger::init();
}
