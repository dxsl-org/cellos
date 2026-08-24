#![no_std]
#![no_main]

extern crate alloc;

use api::declare_manifest;
use driver_bcm_display::mailbox::BcmMailbox;
use driver_bcm_display::BcmFramebuffer;
use ostd::app::{AppContext, AppEvent};
use ostd::sync::Mutex;
use ostd::syscall::sys_register_gpu_driver;

declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false,
    i2c = false,
    spi = false
);

const OP_FLUSH: u8 = 0x10;

static FB: Mutex<Option<BcmFramebuffer>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            ostd::io::println("[bcm-display] BCM VideoCore IV display driver starting...");
            let mailbox = match BcmMailbox::open() {
                Ok(mb) => mb,
                Err(_) => {
                    ostd::io::println("[bcm-display] failed to open BCM mailbox MMIO");
                    ostd::syscall::sys_exit(0);
                }
            };

            let fb = match BcmFramebuffer::allocate(&mailbox, 1280, 720) {
                Ok(fb) => fb,
                Err(_) => {
                    ostd::io::println("[bcm-display] failed to allocate VideoCore framebuffer");
                    ostd::syscall::sys_exit(0);
                }
            };

            ostd::io::println("[bcm-display] VideoCore HDMI framebuffer allocated (1280x720)");
            if sys_register_gpu_driver().is_err() {
                ostd::io::println("[bcm-display] failed to register display driver");
                ostd::syscall::sys_exit(1);
            }

            ostd::io::println("[bcm-display] BCM Display Driver registered with kernel");
            *FB.lock() = Some(fb);
        }

        AppEvent::Message {
            sender_tid: _,
            data,
        } => {
            let bytes: &[u8] = data.as_ref();
            if bytes.is_empty() {
                return;
            }
            if bytes[0] == OP_FLUSH && bytes.len() >= 21 {
                let xy = u32::from_le_bytes(bytes[1..5].try_into().unwrap());
                let wh = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
                let data_ptr = u64::from_le_bytes(bytes[9..17].try_into().unwrap()) as usize;
                let data_len = u32::from_le_bytes(bytes[17..21].try_into().unwrap()) as usize;

                if let Some(fb) = &*FB.lock() {
                    // TRUST MODEL (not a soundness guarantee): `data_ptr` comes
                    // from the `sys_gpu_flush` caller; the kernel only rejects
                    // null/oversized/overflowing ranges and does NOT verify
                    // mapping or lifetime. Reads assume the caller passed a
                    // live compositor buffer (same SAS exposure as the
                    // virtio-gpu driver). Kernel-side copied/pinned ownership
                    // is the tracked fix for this systemic GpuFlush gap.
                    let src =
                        unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len) };
                    fb.flush_rect(src, xy >> 16, xy & 0xFFFF, wh >> 16, wh & 0xFFFF);
                }
            }
        }

        _ => {}
    }
}

ostd::run_app!(handler);
