//! Intel e1000 NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! This cell owns the e1000 PCIe endpoint exclusively.  It:
//!   1. Calls `sys_find_pcie_device(0x02, 0x00, 0x00)` to locate the NIC BAR0.
//!   2. Claims exclusive MMIO via `sys_request_mmio`.
//!   3. Initialises the e1000 controller (TX/RX ring setup, EEPROM MAC read).
//!   4. Calls `sys_register_nic_driver()` to announce itself to the kernel.
//!   5. Serves Tx/Rx/GetMac IPC from the net cell.
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

use ostd::app::{AppContext, AppEvent};
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{sys_find_pcie_device, sys_register_nic_driver, sys_send, PcieDeviceInfo};
use controller::E1000Controller;
use dispatch::{handle, NicReply, REPLY_BUF};

/// Ethernet controller: class 0x02, subclass 0x00, prog-if 0x00.
const ETH_CLASS:  u8 = 0x02;
const ETH_SUB:    u8 = 0x00;
const ETH_PROGIF: u8 = 0x00;

/// BAR0 window size — e1000 register space is 128 KiB.
const E1000_BAR0_LEN: usize = 0x2_0000; // 128 KiB

struct NicState {
    ctrl: E1000Controller,
}

static STATE: Mutex<Option<NicState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // 1. Discover the e1000 via the kernel ECAM table.
            let mut info = PcieDeviceInfo::zeroed();
            let Ok(true) = sys_find_pcie_device(ETH_CLASS, ETH_SUB, ETH_PROGIF, &mut info)
            else {
                // No e1000 on this platform — exit gracefully; kernel NIC remains.
                ostd::syscall::sys_exit(0);
            };

            let bar0_base = info.bar0_base as usize;
            let bdf       = info.bdf;

            // 2. Claim exclusive MMIO access to BAR0.
            let mmio_region = match mmio::request_region(bar0_base, E1000_BAR0_LEN) {
                Ok(r)  => r,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            // 3. Initialise controller (reset + MAC read + ring setup).
            let ctrl = match E1000Controller::new(mmio_region, bdf) {
                Ok(c)  => c,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            // 4. Register as the active NIC driver.
            let _ = sys_register_nic_driver();

            *STATE.lock() = Some(NicState { ctrl });
        }

        AppEvent::Message { sender_tid, data } => {
            let mut out_buf = [0u8; REPLY_BUF];
            if let Some(state) = STATE.lock().as_mut() {
                match handle(&mut state.ctrl, data.as_ref(), &mut out_buf) {
                    NicReply::Status(code) => {
                        let _ = sys_send(sender_tid, &[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        let _ = sys_send(sender_tid, &buf[..2 + len]);
                    }
                    NicReply::Raw(mac) => {
                        let _ = sys_send(sender_tid, &mac);
                    }
                }
            } else {
                let _ = sys_send(sender_tid, &[1u8]);
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
api::declare_manifest!(
    block_io = false, network = false, spawn = false,
    gpio = false, uart = false, hypervisor = false
);
