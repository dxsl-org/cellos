#![no_std]
#![no_main]

use api::{declare_manifest, declare_syscalls};
use core::sync::atomic::{AtomicBool, Ordering};
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
declare_syscalls![
    Recv,
    Log,
    RequestMmio,
    GrantAlloc,
    GrantCacheSyncBegin,
    GrantCacheSyncComplete,
    RegisterDisplayFramebuffer,
];

const OP_FLUSH: u8 = 0x10;

struct DisplayState {
    mailbox: BcmMailbox,
    framebuffer: BcmFramebuffer,
}
static DISPLAY: Mutex<Option<DisplayState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => initialize(),
        AppEvent::Message {
            sender_tid: 0,
            data,
        } => dispatch_flush(data.as_ref()),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => ostd::syscall::sys_exit(0),
        _ => {}
    }
}

fn initialize() {
    ostd::io::println("[bcm-display] BCM VideoCore IV display driver starting...");
    let mut mailbox = match BcmMailbox::open() {
        Ok(mailbox) => mailbox,
        Err(_) => {
            ostd::io::println("[bcm-display] failed to open BCM mailbox MMIO");
            ostd::syscall::sys_exit(0);
        }
    };
    let framebuffer = match BcmFramebuffer::allocate(&mut mailbox, 1280, 720) {
        Ok(framebuffer) => framebuffer,
        Err(_) => {
            ostd::io::println("[bcm-display] failed to allocate VideoCore framebuffer");
            ostd::syscall::sys_exit(0);
        }
    };
    if sys_register_gpu_driver().is_err() {
        ostd::io::println("[bcm-display] failed to register display driver");
        ostd::syscall::sys_exit(1);
    }
    ostd::io::println("[bcm-display] BCM Display Driver registered with kernel");
    *DISPLAY.lock() = Some(DisplayState {
        mailbox,
        framebuffer,
    });
}

fn dispatch_flush(bytes: &[u8]) {
    if bytes.len() < 21 || bytes[0] != OP_FLUSH {
        return;
    }
    let Some(xy) = read_u32(bytes, 1) else { return };
    let Some(wh) = read_u32(bytes, 5) else { return };
    let Some(data_ptr) = read_u64(bytes, 9) else {
        return;
    };
    let Some(data_len) = read_u32(bytes, 17) else {
        return;
    };
    let data_ptr = data_ptr as usize;
    let data_len = data_len as usize;
    if data_ptr == 0 || data_ptr.checked_add(data_len).is_none() {
        return;
    }
    let state = DISPLAY.lock();
    let Some(state) = state.as_ref() else { return };
    static FIRST_FLUSH: AtomicBool = AtomicBool::new(false);
    if !FIRST_FLUSH.swap(true, Ordering::Relaxed) {
        ostd::io::println("[bcm-display] first scanout flush received");
    }
    let _mailbox_lifetime = &state.mailbox;
    // SAFETY: this trusted Tier-1 SAS contract lets the kernel send its own
    // compositor pointer only as sender TID 0. The kernel validates IPC shape,
    // while this handler validates non-nullness and arithmetic bounds; it does
    // not establish independent mapping or lifetime safety for the raw pointer.
    let src = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len) };
    state
        .framebuffer
        .flush_rect(src, xy >> 16, xy & 0xffff, wh >> 16, wh & 0xffff);
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let raw: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

ostd::run_app!(handler);
