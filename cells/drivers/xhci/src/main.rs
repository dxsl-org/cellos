//! xHCI Driver Cell — Tier-1 Privileged Driver Cell (x86_64 PC lane, phase 03).
//!
//! Owns the xHCI USB host controller and performs the whole USB host family:
//!   1. `sys_find_pcie_device(0x0C/0x03/0x30)` locates the controller (a
//!      non-xHCI USB controller fails closed with a named log line).
//!   2. Claims the controller's memory BAR via `sys_request_mmio`.
//!   3. Resets the controller, programs the command ring and event ring, resets
//!      the port carrying the device, and enumerates it: Enable Slot, Address
//!      Device, GET_DESCRIPTOR (device + configuration), SET_CONFIGURATION,
//!      SET_PROTOCOL(boot), Configure Endpoint.
//!   4. Polls one interrupt-IN transfer for the HID boot-protocol keyboard,
//!      decodes it with the shared `driver-hid` crate, and delivers the key to
//!      the input service through the same producer path `/bin/dwc2-usb` uses.
//!
//! Every DMA structure goes through `DmaBuf` + `authorize` so phase-05's IOMMU
//! path applies without rework. A machine with no xHCI device leaves the cell
//! idle; an unsupported revision, missing interrupters or non-HID device fails
//! closed with a named reason.
//!
//! Law 4 exception: this cell uses `unsafe` for DMA memory access; MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.

#![no_std]
#![no_main]
extern crate alloc;

mod controller;
mod dma;
mod input;
mod regs;

use alloc::vec::Vec;
use api::syscall::service;
use controller::XhciController;
use driver_hid::{decode_boot_report, BootState, EvdevEvent, HidDeviceId, HidKind};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device, sys_lookup_service, sys_register_usb_hid_producer, PcieDeviceInfo,
};
use types::ViError;

/// USB controller class triple. xHCI is prog-if 0x30.
const XHCI_CLASS: u8 = 0x0C;
const XHCI_SUB: u8 = 0x03;
const XHCI_PROGIF: u8 = 0x30;
/// Cap on the MMIO window this cell will map; the register file is far smaller.
const XHCI_BAR_LEN: usize = 0x10000;
/// Smallest window that still covers the capability, operational and runtime
/// registers plus the first port register set.
const XHCI_BAR_MIN: usize = 0x1000;
/// Receive timeout (scheduler ticks) between interrupt-IN polls — ~10 ms.
const POLL_TICKS: u64 = 1;
/// Stable logical identity for the single xHCI keyboard interface.
const DEVICE_ID: HidDeviceId = HidDeviceId(0x7830_0001);

struct XhciState {
    ctrl: XhciController,
    input_tid: usize,
    source_registered: bool,
    boot: BootState,
    events: Vec<EvdevEvent>,
}

static STATE: Mutex<Option<XhciState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => init(),
        AppEvent::Timeout => poll(),
        // The input service may send lock-LED frames; a boot keyboard without an
        // LED output endpoint has nothing to do with them, so they are ignored.
        AppEvent::Message { .. } | AppEvent::RawMessage { .. } => {}
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => ostd::syscall::sys_exit(0),
        _ => {}
    }
}

/// One-time bring-up: locate the controller, claim its BAR, start it, enumerate
/// the keyboard, and register as an input event producer.
fn init() {
    // The Platform Cell registers devices concurrently with this cell's spawn;
    // retry for a bounded window before concluding absent.
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;
    for _ in 0..200 {
        if let Ok(true) = sys_find_pcie_device(XHCI_CLASS, XHCI_SUB, XHCI_PROGIF, &mut info) {
            found = true;
            break;
        }
        ostd::task::yield_now();
    }
    if !found {
        let _ = print_fmt(format_args!(
            "[xhci] no xHCI controller present; driver cell idle\n"
        ));
        ostd::syscall::sys_exit(0);
    }

    let base = info.bar_mem_base as usize;
    let len = if info.bar_mem_len == 0 {
        XHCI_BAR_LEN
    } else {
        (info.bar_mem_len as usize).min(XHCI_BAR_LEN)
    };
    if base == 0 || len < XHCI_BAR_MIN {
        let _ = print_fmt(format_args!(
            "[xhci] controller has no usable MMIO BAR (base=0x{:x} len=0x{:x})\n",
            base, len
        ));
        ostd::syscall::sys_exit(1);
    }
    let region = match mmio::request_region(base, len) {
        Ok(region) => region,
        Err(error) => {
            let _ = print_fmt(format_args!("[xhci] BAR request failed: {:?}\n", error));
            ostd::syscall::sys_exit(1);
        }
    };
    let _ = print_fmt(format_args!(
        "[xhci] BAR claim ok base=0x{:x} len=0x{:x}\n",
        base, len
    ));

    let mut ctrl = match XhciController::new(region, info.bdf) {
        Ok(ctrl) => ctrl,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[xhci] controller bring-up failed: {:?}\n",
                error
            ));
            ostd::syscall::sys_exit(1);
        }
    };

    match ctrl.enumerate() {
        Ok(dev) => {
            let _ = print_fmt(format_args!(
                "[xhci] USB HID keyboard ready vid=0x{:04x} pid=0x{:04x}\n",
                dev.vid, dev.pid
            ));
        }
        Err(ViError::NotFound) => {
            let _ = print_fmt(format_args!(
                "[xhci] no USB device attached; driver cell idle\n"
            ));
            ostd::syscall::sys_exit(0);
        }
        Err(error) => {
            let _ = print_fmt(format_args!("[xhci] enumeration failed: {:?}\n", error));
            ostd::syscall::sys_exit(1);
        }
    }

    // The input service's producer gate verifies `service::USB_HID_PRODUCER`.
    // This cell must NOT call `sys_register_nic_driver()`: that is the singleton
    // NIC owner, and claiming it on a PC that already has e1000/virtio-net would
    // redirect network IPC here and drop it. The USB HID producer role is a
    // separate kernel-verified identity that publishes only
    // `service::USB_HID_PRODUCER`, leaving the NIC route untouched.
    if let Err(error) = sys_register_usb_hid_producer() {
        let _ = print_fmt(format_args!(
            "[xhci] USB HID producer registration failed: {:?}\n",
            error
        ));
    }
    let input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
    let source_registered = input_tid != 0 && input::register_as_source(input_tid);
    *STATE.lock() = Some(XhciState {
        ctrl,
        input_tid,
        source_registered,
        boot: BootState::default(),
        events: Vec::new(),
    });
}

/// Poll the interrupt-IN endpoint and forward any decoded key events.
fn poll() {
    let mut guard = STATE.lock();
    let Some(state) = guard.as_mut() else {
        return;
    };

    // The input service is supervised and comes back under a new tid after a
    // restart, so re-resolve and re-register whenever the route is missing.
    if state.input_tid == 0 {
        state.input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
        state.source_registered = false;
    }
    if state.input_tid != 0 && !state.source_registered {
        state.source_registered = input::register_as_source(state.input_tid);
    }

    let Some(report) = state.ctrl.poll_report() else {
        return;
    };
    state.events.clear();
    decode_boot_report(HidKind::Keyboard, &report, &mut state.boot, &mut state.events);
    for event in state.events.drain(..) {
        if let EvdevEvent::Key { code, pressed } = event {
            let _ = print_fmt(format_args!(
                "[xhci] key {} code=0x{:x}\n",
                if pressed { "down" } else { "up" },
                code
            ));
        }
        if state.source_registered
            && !input::forward_device_event(state.input_tid, DEVICE_ID, &event)
        {
            // The supervised input service restarted under a new tid: drop the
            // stale route so the next poll re-resolves and re-registers instead
            // of sending every later key to the dead tid and losing it.
            state.input_tid = 0;
            state.source_registered = false;
            break;
        }
    }
}

#[no_mangle]
pub fn main() {
    let mut ctx = AppContext::new();
    // Bring-up runs before the receive loop; a long-running Init arm would block
    // the input-service lock-LED frames, so the poll loop is the Timeout arm.
    handler(&mut ctx, AppEvent::Init);
    ctx.run_with_timeout(POLL_TICKS, handler);
}

// ── Capability manifest ───────────────────────────────────────────────────────
// PcieDriverCap and UsbDriverCap are granted by init via the reviewed
// `/bin/xhci` launch edge (not a manifest flag). This manifest declares NO
// privileged flags.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// ── Syscall allowlist ─────────────────────────────────────────────────────────
// Without this section the kernel leaves the cell at the TCB's `u64::MAX`
// (kernel/src/task/syscall.rs): every syscall is permitted by default. This cell
// hand-rolls `main()` with `AppContext::run_with_timeout` instead of `run_app!`,
// so nothing declared an allowlist for it. List exactly what the cell calls:
// the AppContext base set, the PCIe device path, the HID-producer role, IPC, the
// exit path, and the DMA grant calls behind `DmaBuf::alloc`/`authorize`.
// `Exit`/`ForceExit`/`Yield` carry no allowlist bit and are always permitted, but
// naming them keeps the intent explicit (the macro skips bit-less names).
api::declare_syscalls![
    // AppContext event-loop base set.
    Send,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    RecvTimeout,
    // PCIe device path.
    FindPcieDevice,
    RequestMmio,
    // Kernel-verified USB HID producer role.
    RegisterUsbHidProducer,
    // Producer registration uses a non-blocking send.
    TrySend,
    // Clean shutdown returns through the exit path.
    Exit,
    // DMA: `DmaBuf::alloc` -> GrantAlloc, `DmaBuf::authorize` -> GrantDma.
    GrantAlloc,
    GrantDma
];
