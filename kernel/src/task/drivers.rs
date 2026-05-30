//! Driver interfaces and registry.
//!
//! This module manages the lifecycle and registration of kernel drivers.
//! It serves as the central point for:
//! 1. Hardware Abstraction (HAL) implementations (e.g., VirtIO)
//! 2. Driver discovery and initialization
//! 3. Driver naming and ID resolution

// Export the registry for driver management
pub mod registry;

// HAL implementations
pub mod virtio_hal;

// Serial Driver
pub mod uart;

// Drivers
pub mod console_drv;
pub mod fb_console;
pub mod font;
pub mod input_map;
pub mod ramdisk; // RAM Disk workaround for VirtIO hang
pub mod virtio_blk;
pub mod virtio_gpu;
pub mod virtio_input;
pub mod virtio_net;

/// Initialize drivers subsystem
///
/// Use: Sets up the driver registry and initializes statically linked drivers.
pub fn init() {
    registry::init();

    // Init specific drivers
    virtio_input::init_driver();
    console_drv::init();
    ramdisk::init_driver(); // RAM disk for embedded FAT32 (kernel self-hosted FS)
    // Disable global interrupts during VirtIO init to prevent IRQ deadlocks.
    // VirtIO block raises an IRQ on init; if the PLIC is enabled and the trap
    // handler tries to re-acquire a Spinlock held by this thread, it will spin
    // forever.  We re-enable SIE after all drivers are initialised.
    virtio_blk::init_driver(); // VirtIO block — GPU probe hang fixed via mem::forget
    virtio_net::init_driver(); // VirtIO NIC — backs the net service cell
    virtio_gpu::init_driver();
}
