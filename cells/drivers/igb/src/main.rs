//! Intel `igb` (i210/i211) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! First NIC family after the 82540EM. It:
//!   1. Calls `sys_find_pcie_device_by_vendor(8086, id)` for each ID in
//!      `identity::QUERY_DEVICE_IDS` to locate **its own** Ethernet endpoint.
//!      The class triple `02:00:00` is shared with `/bin/e1000`, so naming the
//!      controller exactly is what removes the sibling decline-and-retry race.
//!   2. **Confirms the identity is this family** (`identity::classify`) before it
//!      touches MMIO: a 82540EM (or any other family) must not be programmed with
//!      the igb register model. An unknown ID fails closed with a line naming
//!      vendor:device.
//!   3. Claims BAR0 via `sys_request_mmio`, resets the controller, reads the MAC
//!      from the NVM, programs the TX/RX descriptor rings and the unicast/
//!      multicast filters, and enables TX/RX — every DMA buffer through
//!      `DmaBuf::authorize`.
//!   4. Calls `sys_register_nic_driver()` — **only after a device was found and
//!      brought up** — and serves the same Tx/Rx/GetMac IPC the net service
//!      already speaks to `/bin/e1000`.
//!
//! Register model and the QEMU-vs-datasheet ID split live in `controller.rs` and
//! `identity.rs`.
//!
//! Law 4 exception: this cell uses `unsafe` for DMA memory access; MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/igb` launch edge —
//! NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
extern crate alloc;

mod controller;
mod dispatch;
mod dma_layout;
mod identity;

use controller::IgbController;
use dispatch::{handle, NicReply, REPLY_BUF};
use identity::{classify, sku_name, IgbSku, QUERY_DEVICE_IDS, VENDOR_INTEL};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_register_nic_driver, sys_try_send, PcieDeviceInfo,
};

/// BAR0 window — every igb part exposes a 128 KiB register aperture (QEMU `igb`:
/// `E1000E_MMIO_SIZE = 128 KiB`).
const IGB_BAR0_LEN: usize = 0x2_0000;
/// Highest register this cell touches (`TDWBAH0`) plus one word.
const IGB_MMIO_MIN: usize = 0x0_E040;

/// Bounded wait for the Platform Cell / kernel ECAM scan to publish the device
/// table, matching `/bin/e1000` and `/bin/nvme`. This is **not** a sibling
/// workaround: the cell names its controller exactly (`sys_find_pcie_device_by_vendor`),
/// so no other NIC cell can hold it and there is no retry race to win.
const FIND_ATTEMPTS: usize = 200;

struct NicState {
    ctrl: IgbController,
}

static STATE: Mutex<Option<NicState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // Name the controller exactly. The class triple is shared with
            // `/bin/e1000`, so a class query would hand this cell the sibling's
            // device (or vice versa) and one of them would have to decline and
            // release it; the vendor:device query removes that dependency.
            let mut info = PcieDeviceInfo::zeroed();
            let mut found = false;
            'attempts: for _ in 0..FIND_ATTEMPTS {
                for &device_id in QUERY_DEVICE_IDS.iter() {
                    if let Ok(true) =
                        sys_find_pcie_device_by_vendor(VENDOR_INTEL, device_id, &mut info)
                    {
                        found = true;
                        break 'attempts;
                    }
                }
                ostd::task::yield_now();
            }
            if !found {
                let _ = print_fmt(format_args!(
                    "[igb] no igb controller present; driver cell idle\n"
                ));
                ostd::syscall::sys_exit(0);
            }

            // Defense in depth: the kernel's family gate should already have
            // refused any ID this cell cannot drive, but the register model must
            // never be programmed on an unclassified device.
            let sku = classify(info.vendor_id, info.device_id);
            if sku == IgbSku::Unsupported {
                let _ = print_fmt(format_args!(
                    "[igb] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
                    info.vendor_id, info.device_id
                ));
                ostd::syscall::sys_exit(0);
            }

            let bar0_base = info.bar0_base as usize;
            let bar0_len = if info.bar0_len == 0 {
                IGB_BAR0_LEN
            } else {
                (info.bar0_len as usize).min(IGB_BAR0_LEN)
            };
            if bar0_base == 0 || bar0_len < IGB_MMIO_MIN {
                let _ = print_fmt(format_args!(
                    "[igb] controller has no usable MMIO BAR (base={:#x} len={:#x})\n",
                    bar0_base, bar0_len
                ));
                ostd::syscall::sys_exit(1);
            }

            let mmio_region = match mmio::request_region(bar0_base, bar0_len) {
                Ok(r) => r,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            let ctrl = match IgbController::new(mmio_region, info.bdf) {
                Ok(c) => c,
                Err(error) => {
                    let _ = print_fmt(format_args!(
                        "[igb] controller initialization failed: {:?}\n",
                        error
                    ));
                    ostd::syscall::sys_exit(1)
                }
            };

            let mac = ctrl.mac;
            let link_up = ctrl.link_up;
            // Publish state before registration: the kernel may route requests as
            // soon as the syscall succeeds.
            *STATE.lock() = Some(NicState { ctrl });
            let _ = print_fmt(format_args!(
                "[igb] controller bound {:04x}:{:04x} {} link_up={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                info.vendor_id,
                info.device_id,
                sku_name(sku),
                link_up,
                mac[0],
                mac[1],
                mac[2],
                mac[3],
                mac[4],
                mac[5]
            ));
            if let Err(error) = sys_register_nic_driver() {
                *STATE.lock() = None;
                let _ = print_fmt(format_args!(
                    "[igb] NIC driver registration failed: {:?}\n",
                    error
                ));
                ostd::syscall::sys_exit(1);
            }
        }

        // The net service speaks the raw NIC wire protocol (no 0xAC App-SDK
        // envelope), so requests arrive as RawMessage. Accept Message too for
        // envelope-wrapped senders — the dispatch payload layout is identical.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            // Replies use NON-blocking try_send — see virtio-net/src/main.rs: a
            // blocking reply to a net service that already timed out parks this
            // cell in Sending{net} and desyncs every later request/reply.
            let mut out_buf = [0u8; REPLY_BUF];
            if let Some(state) = STATE.lock().as_mut() {
                match handle(&mut state.ctrl, data.as_ref(), &mut out_buf) {
                    NicReply::Status(code) => {
                        let _ = sys_try_send(sender_tid, &[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        let _ = sys_try_send(sender_tid, &buf[..2 + len]);
                    }
                    NicReply::Raw(mac) => {
                        let _ = sys_try_send(sender_tid, &mac);
                    }
                }
            } else {
                let _ = sys_try_send(sender_tid, &[1u8]);
            }
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }
        _ => {}
    }
}

ostd::run_app!(handler);

// ── Capability manifest ───────────────────────────────────────────────────────
// PcieDriverCap is granted by init via the reviewed `/bin/igb` launch edge (not
// a manifest flag). This manifest declares NO privileged flags.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// ── Syscall allowlist ─────────────────────────────────────────────────────────
// `run_app!` emits a manifest but NOT a `VICELL_SYSCALLS` section, so without
// this the kernel leaves the cell at its TCB's `u64::MAX` allowlist. List
// exactly what the cell calls: the AppContext base set, the PCIe device path,
// NIC registration, the try_send reply path, the exit path, and the DMA calls
// behind `DmaBuf::alloc`/`authorize`. `Exit`/`Yield` carry no allowlist bit and
// are always permitted; naming them documents the intent.
api::declare_syscalls![
    // AppContext event-loop base set.
    Send,
    TrySend,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    RecvTimeout,
    // PCIe device path: the exact-identity query (shares allowlist bit 50 with
    // the legacy class query, so the signed mask is unchanged).
    FindPcieDeviceByVendor,
    RequestMmio,
    // System NIC-driver role.
    RegisterNicDriver,
    // Clean shutdown returns through the exit path.
    Exit,
    // DMA: `DmaBuf::alloc` -> GrantAlloc, `DmaBuf::authorize` -> GrantDma.
    GrantAlloc,
    GrantDma
];
