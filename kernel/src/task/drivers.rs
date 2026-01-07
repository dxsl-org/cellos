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
pub mod virtio_blk;
pub mod ramdisk;  // RAM Disk workaround for VirtIO hang
pub mod virtio_gpu;
pub mod console_drv;
pub mod font;
pub mod fb_console;
pub mod virtio_input;
pub mod input_map;

/// Initialize drivers subsystem
///
/// Use: Sets up the driver registry and initializes statically linked drivers.
pub fn init() {
    registry::init();
    
    // Init specific drivers
    virtio_input::init_driver();
    console_drv::init();
    ramdisk::init_driver();  // Use RAM disk instead of VirtIO (workaround)
    // virtio_blk::init_driver();  // Disabled due to hang issue
    virtio_gpu::init_driver();
}
