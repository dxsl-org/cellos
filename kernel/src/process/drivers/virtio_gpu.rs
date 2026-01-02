use log::info;
use virtio_drivers::transport::mmio::VirtIOHeader;
use virtio_drivers::device::gpu::VirtIOGpu;
use virtio_drivers::transport::mmio::MmioTransport;
use virtio_drivers::transport::DeviceType;
use virtio_drivers::transport::Transport;
use crate::process::drivers::virtio_hal::VirtioHal as VirtIOHal;
use crate::sync::Spinlock;

// QEMU VirtIO GPU address (Bus 7, but legacy MMIO might be different)
// In virt machine, MMIO devices are at 0x10001000, 0x10002000...
// We need to scan like Block driver did.

static GPU_DEVICE: Spinlock<Option<VirtIOGpu<VirtIOHal, MmioTransport>>> = Spinlock::new(None);

pub fn init_driver() {
    info!("VirtIO: Scanning for GPU Device...");

    // Scan MMIO region (8 devices max for basic virt machine)
    for i in 0..8 {
        let addr = 0x1000_1000 + i * 0x1000;
        
        // Safety: We assume this address maps to MMIO region.
        // MmioTransport::new expects a NonNull pointer to the header.
        if let Some(header) = core::ptr::NonNull::new(addr as *mut VirtIOHeader) {
            match unsafe { MmioTransport::new(header) } {
                Ok(transport) => {
                    if transport.device_type() == DeviceType::GPU {
                        info!("VirtIO: Found GPU at 0x{:X}", addr);
                        match VirtIOGpu::<VirtIOHal, MmioTransport>::new(transport) {
                           Ok(mut gpu) => {
                               // Try to get resolution.
                               match gpu.resolution() {
                                   Ok((w, h)) => info!("VirtIO GPU: Display Resolution {}x{}", w, h),
                                   Err(_) => info!("VirtIO GPU: Init successful (default resolution)."),
                               }
                               
                               *GPU_DEVICE.lock() = Some(gpu);
                               info!("VirtIO GPU: Driver Initialized successfully.");
                               return;
                           }
                           Err(e) => log::error!("VirtIO GPU: Failed to init driver: {:?}", e),
                        }
                    }
                }
                Err(_) => {
                    // Not a valid VirtIO device, continue scanning
                }
            }
        }
    }
    log::warn!("VirtIO: No GPU device found.");
}
