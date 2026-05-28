use crate::sync::Spinlock;
use crate::task::drivers::virtio_hal::VirtioHal as VirtIOHal;
use core::ptr::NonNull;
// use log::info;
use virtio_drivers::device::gpu::VirtIOGpu;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};

pub struct GpuContext {
    pub gpu: VirtIOGpu<VirtIOHal, MmioTransport>,
    fb_ptr: *mut u8,
    fb_len: usize,
    pub width: u32,
    pub height: u32,
}

unsafe impl Send for GpuContext {}

pub static GPU_CONTEXT: Spinlock<Option<GpuContext>> = Spinlock::new(None);

impl GpuContext {
    pub fn framebuffer(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.fb_ptr, self.fb_len) }
    }
}

pub fn init_driver() {
    log::info!("VirtIO GPU: Probing...");

    // We scan standard VirtIO MMIO slots (0x10001000 region)
    let transport_interval = 0x1000;

    for i in 0..8 {
        let addr = 0x1000_1000 + i * transport_interval;
        let header = unsafe { NonNull::new_unchecked((addr) as *mut VirtIOHeader) };

        match unsafe { MmioTransport::new(header) } {
            Ok(transport) => {
                if transport.device_type() == DeviceType::GPU {
                    log::info!("VirtIO GPU: Found at 0x{:X}", addr);
                    match VirtIOGpu::<VirtIOHal, MmioTransport>::new(transport) {
                        Ok(mut gpu) => {
                            // Probe resolution
                            let (width, height) = match gpu.resolution() {
                                Ok(res) => res,
                                Err(_) => (1280, 800), // Fallback
                            };
                            log::info!("VirtIO GPU: Probed Resolution: {}x{}", width, height);

                            // Setup 2D Resource
                            match gpu.setup_framebuffer() {
                                Ok(fb_slice) => {
                                    log::info!(
                                        "VirtIO GPU: Framebuffer setup success. Len: {}",
                                        fb_slice.len()
                                    );

                                    let fb_ptr = fb_slice.as_mut_ptr();
                                    let fb_len = fb_slice.len();

                                    *GPU_CONTEXT.lock() = Some(GpuContext {
                                        gpu,
                                        fb_ptr,
                                        fb_len,
                                        width,
                                        height,
                                    });

                                    // Flush
                                    if let Some(ctx) = GPU_CONTEXT.lock().as_mut() {
                                        let _ = ctx.gpu.flush();
                                    }

                                    // Init Framebuffer Console here
                                    crate::task::drivers::fb_console::FramebufferConsole::init();
                                }
                                Err(e) => {
                                    log::error!("VirtIO GPU: Setup Framebuffer failed: {:?}", e)
                                }
                            }
                            return;
                        }
                        Err(e) => {
                            log::error!("VirtIO GPU: Init failed: {:?}", e);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
    log::warn!("VirtIO GPU: No device found.");
}
