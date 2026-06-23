//! NVMe Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! This cell owns the NVMe PCIe endpoint exclusively.  It:
//!   1. Calls `sys_find_pcie_device(NVMe class/sub/progif)` to locate BAR0.
//!   2. Claims exclusive MMIO via `sys_request_mmio`.
//!   3. Initialises the NVMe controller (init sequence identical to blk_nvme.rs).
//!   4. Calls `sys_register_block_driver()` to announce itself to the kernel.
//!   5. Serves `DrvRequest` IPC from VFS (sector read/write).
//!
//! Law 4 exception: this cell uses `unsafe` for MMIO register access.  Every
//! `unsafe` block carries a `// SAFETY:` comment.
//!
//! This cell is granted `PcieDriverCap` by `init` via direct TCB write at spawn
//! time — NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
extern crate alloc;

mod controller;
mod dispatch;
mod queue;

use ostd::app::{AppContext, AppEvent};
use ostd::dma::DmaBuf;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{sys_find_pcie_device, sys_register_block_driver, sys_send, PcieDeviceInfo};
use controller::NvmeController;

/// NVMe PCIe class triple.
const NVME_CLASS:  u8 = 0x01;
const NVME_SUB:    u8 = 0x08;
const NVME_PROGIF: u8 = 0x02;

/// BAR0 MMIO window size (NVMe spec: at least 16 KiB for BAR0).
const NVME_BAR0_LEN: usize = 0x4000; // 16 KiB

struct NvmeState {
    ctrl:   NvmeController,
    io_buf: DmaBuf,
}

static STATE: Mutex<Option<NvmeState>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // 1. Discover NVMe device via kernel ECAM table.
            let mut info = PcieDeviceInfo::zeroed();
            let Ok(true) = sys_find_pcie_device(NVME_CLASS, NVME_SUB, NVME_PROGIF, &mut info)
            else {
                // No NVMe found or no PcieDriverCap — exit; kernel NVMe remains active.
                ostd::syscall::sys_exit(0);
            };

            let bar0_base = info.bar0_base as usize;
            let bdf       = info.bdf;

            // 2. Claim exclusive MMIO access to BAR0.
            let mmio_region = match mmio::request_region(bar0_base, NVME_BAR0_LEN) {
                Ok(r)  => r,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            // 3. Initialise controller.
            let ctrl = match NvmeController::new(mmio_region, bdf) {
                Ok(c)  => c,
                Err(_) => ostd::syscall::sys_exit(1),
            };

            // Allocate a reusable 512-byte I/O DMA buffer.
            let io_buf = match DmaBuf::alloc(1) {
                Some(b) => b,
                None    => ostd::syscall::sys_exit(1),
            };
            let _ = io_buf.authorize(bdf);

            // 4. Register as the active block driver.
            let _ = sys_register_block_driver();

            *STATE.lock() = Some(NvmeState { ctrl, io_buf });
        }

        AppEvent::Message { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if let Some(state) = STATE.lock().as_mut() {
                dispatch::handle(&mut state.ctrl, &state.io_buf, data.as_ref(), &mut reply)
            } else {
                reply[0] = 1;
                1 // not initialised
            };
            let _ = sys_send(sender_tid, &reply[..len]);
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
// This manifest declares NO privileged flags — init elevates the cell at spawn.
api::declare_manifest!(
    block_io = false, network = false, spawn = false,
    gpio = false, uart = false, hypervisor = false
);
