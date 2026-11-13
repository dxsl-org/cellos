//! Intel e1000 (8254x) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! This cell owns one 82540EM or 82545EM PCIe endpoint exclusively. It:
//!   1. Calls `sys_find_pcie_device_by_vendor(8086, id)` (opcode 424) for each ID
//!      in `identity::QUERY_DEVICE_IDS`. The class triple `02:00:00` is shared
//!      with `/bin/igb`, `/bin/e1000e` and `/bin/virtio-net`, so the cell names
//!      its controllers exactly instead of using the class query (H-S3-6).
//!   2. Confirms the identity (`identity::classify`) before it touches MMIO.
//!   3. Claims exclusive MMIO via `sys_request_mmio`.
//!   4. Initialises the controller (TX/RX ring setup, EEPROM MAC read).
//!   5. Calls `sys_register_nic_driver()` to announce itself to the kernel.
//!   6. Serves Tx/Rx/GetMac IPC from the net cell. RX is request-driven (the net
//!      service asks for frames), so the cell stays polled.
//!
//! Law 4 exception: this cell uses `unsafe` for MMIO register access.
//! Every `unsafe` block carries a `// SAFETY:` comment.
//!
//! This cell is granted `PcieDriverCap` by `init` via direct TCB write at spawn
//! time — NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
extern crate alloc;

mod controller;
mod dispatch;
mod dma_layout;
mod identity;

use controller::E1000Controller;
use dispatch::{handle, NicReply, REPLY_BUF};
use identity::{classify, sku_name, E1000Sku, QUERY_DEVICE_IDS, VENDOR_INTEL};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_register_nic_driver, sys_try_send, PcieDeviceInfo,
};
use wire::proto::nic::NicStatus;

/// BAR0 window size — e1000 register space is 128 KiB.
const E1000_BAR0_LEN: usize = 0x2_0000; // 128 KiB

/// Bounded wait for the Platform Cell / kernel ECAM scan to publish the device
/// table (the same window as `/bin/igb` and `/bin/nvme`).
const FIND_ATTEMPTS: usize = 200;

struct NicState {
    ctrl: E1000Controller,
}

static STATE: Mutex<Option<NicState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // 1. Name the controller exactly, retrying for a bounded window so a
            // Platform registration race fails closed as "not present" instead
            // of a false permanent absence.
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
                // No 8254x on this platform: exit silently. The `[e1000]` tag
                // stays reserved for this cell's own device, so sibling NIC
                // lanes can assert that it never touched theirs.
                ostd::syscall::sys_exit(0);
            }

            // Defense in depth: the kernel gate should already have refused any
            // ID this cell cannot drive.
            let sku = classify(info.vendor_id, info.device_id);
            if sku == E1000Sku::Unsupported {
                let _ = print_fmt(format_args!(
                    "[e1000] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
                    info.vendor_id, info.device_id
                ));
                ostd::syscall::sys_exit(0);
            }

            let bar0_base = info.bar0_base as usize;
            let bdf = info.bdf;

            // 2. Claim exclusive MMIO access to BAR0.
            let mmio_region = match mmio::request_region(bar0_base, E1000_BAR0_LEN) {
                Ok(r) => r,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            // 3. Initialise controller (reset + MAC read + ring setup).
            let ctrl = match E1000Controller::new(mmio_region, bdf) {
                Ok(c) => c,
                Err(error) => {
                    let _ = print_fmt(format_args!(
                        "[e1000] controller initialization failed: {:?}\n",
                        error
                    ));
                    ostd::syscall::sys_exit(1)
                }
            };

            let mac = ctrl.mac;
            // Publish state before registration: the kernel may route requests
            // as soon as the syscall succeeds.
            *STATE.lock() = Some(NicState { ctrl });
            let _ = print_fmt(format_args!(
                "[e1000] controller bound {:04x}:{:04x} {} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} irq=polled reason=request-driven\n",
                info.vendor_id,
                info.device_id,
                sku_name(sku),
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
                    "[e1000] NIC driver registration failed: {:?}\n",
                    error
                ));
                ostd::syscall::sys_exit(1);
            }
        }

        // The net service speaks the raw NIC wire protocol (no 0xAC App-SDK
        // envelope), so requests arrive as RawMessage. Accept Message too for
        // envelope-wrapped senders — the dispatch payload layout is identical.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let operation = ostd::ipc::current();
            let reply = |bytes: &[u8]| {
                if let Some(operation) = operation {
                    let _ = ostd::ipc::reply(operation, bytes);
                } else {
                    let _ = sys_try_send(sender_tid, bytes);
                }
            };
            // Only the live net service may drive the NIC; any other cell could
            // inject or read raw frames. `sender_tid` is kernel-stamped and the
            // registry is asked per request, so a restarted net service — or one
            // init has not registered yet — is followed without a cached tid.
            // The refusal is the ordinary failure byte the net service retries on.
            if !sender_is_live_provider(service::NET, sender_tid) {
                reply(&[NicStatus::Failed as u8]);
                return;
            }
            let mut out_buf = [0u8; REPLY_BUF];
            if let Some(state) = STATE.lock().as_mut() {
                match handle(&mut state.ctrl, data.as_ref(), &mut out_buf) {
                    NicReply::Status(code) => {
                        reply(&[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        reply(&buf[..2 + len]);
                    }
                    NicReply::Raw(mac) => {
                        reply(&mac);
                    }
                }
            } else {
                reply(&[NicStatus::Failed as u8]);
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
// PcieDriverCap is granted by init via direct TCB write (not a manifest flag).
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// ── Syscall allowlist ─────────────────────────────────────────────────────────
// `run_app!` emits a manifest but NOT a `VICELL_SYSCALLS` section, so without
// this the kernel leaves the cell at its TCB's `u64::MAX` allowlist. The list is
// exactly `/bin/igb`'s: the AppContext base set, the exact-identity PCIe query,
// NIC registration, the try_send reply path, the exit path, and the DMA calls
// behind `DmaBuf::alloc`/`authorize`. `Exit`/`Yield` carry no allowlist bit and
// are always permitted; naming them documents the intent.
abi::declare_syscalls![
    // AppContext event-loop base set.
    Send,
    TrySend,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupServiceBound,
    GetTime,
    RecvTimeout,
    // PCIe device path: the exact-identity query (424, shares allowlist bit 50
    // with the legacy class query) and the BAR0 claim.
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
