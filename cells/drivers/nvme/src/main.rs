//! NVMe Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! This cell owns the NVMe PCIe endpoint exclusively.  It:
//!   1. Calls `sys_find_pcie_device(NVMe class/sub/progif)` to locate BAR0.
//!   2. Claims exclusive MMIO via `sys_request_mmio`.
//!   3. Initialises the NVMe controller (init sequence identical to blk_nvme.rs).
//!   4. Publishes the block route of its BDF via
//!      `sys_register_block_device(BlockDevice::Pci { .. })`.
//!   5. Serves `DrvRequest` IPC (sector read/write) from the registered VFS only.
//!
//! Law 4 exception: this cell uses `unsafe` for MMIO register access.  Every
//! `unsafe` block carries a `// SAFETY:` comment.
//!
//! This cell is granted `PcieDriverCap` by `init` via direct TCB write at spawn
//! time — NOT via a manifest flag (all 8 flag bits in v1 are occupied).

// Test builds only: a hosted `cargo test` gets std and libtest's entry, so the wait
// budgets (`deadline`) run as host unit tests (S10-3c, D3c-3 A). Every Cell target
// (`*-unknown-none`, `*-unknown-cellos`) keeps `no_std`/`no_main`.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod bounce;
mod controller;
mod deadline;
mod dispatch;
mod dma;
mod irq;
mod layout;
mod queue;

use bounce::BounceBuffer;
use controller::NvmeController;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{sys_find_pcie_device, sys_register_block_device, BlockDevice, PcieDeviceInfo};

/// NVMe PCIe class triple.
const NVME_CLASS: u8 = 0x01;
const NVME_SUB: u8 = 0x08;
const NVME_PROGIF: u8 = 0x02;

/// BAR0 MMIO window size (NVMe spec: at least 16 KiB for BAR0).
const NVME_BAR0_LEN: usize = 0x4000; // 16 KiB

struct NvmeState {
    ctrl: NvmeController,
    bounce: BounceBuffer,
}

static STATE: Mutex<Option<NvmeState>> = Mutex::new(None);

/// Tid of the registered VFS once a request from it was accepted. Tids are never
/// reissued, so a cached match stays valid for the life of that VFS.
static VFS_TID: AtomicUsize = AtomicUsize::new(0);

/// Whether `sender_tid` is the registered VFS, the only accepted block client.
fn sender_is_vfs(sender_tid: usize) -> bool {
    let cached = VFS_TID.load(Ordering::Relaxed);
    if cached != 0 && cached == sender_tid {
        return true;
    }
    if ostd::service::sender_is_live_provider(abi::syscall::service::VFS, sender_tid) {
        VFS_TID.store(sender_tid, Ordering::Relaxed);
        return true;
    }
    false
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // 1. Discover NVMe device via kernel ECAM table. This cell spawns
            // pre-VFS, concurrently with the Platform Cell's ECAM scan — the
            // device entry may not be registered yet (the scan is slower when
            // each RegisterPciDevice also runs the deferred VT-d init), so
            // retry with yields for a bounded window before concluding absent.
            let mut info = PcieDeviceInfo::zeroed();
            let mut found = false;
            for _ in 0..200 {
                if let Ok(true) = sys_find_pcie_device(NVME_CLASS, NVME_SUB, NVME_PROGIF, &mut info)
                {
                    found = true;
                    break;
                }
                ostd::task::yield_now();
            }
            if !found {
                // Exit 0: nothing to claim (no NVMe controller visible to this
                // cell, e.g. none on this machine or no PcieDriverCap).
                ostd::syscall::sys_exit(0);
            }

            let bar0_base = info.bar0_base as usize;
            let bdf = info.bdf;

            // 2. Claim exclusive MMIO access to BAR0.
            let mmio_region = match mmio::request_region(bar0_base, NVME_BAR0_LEN) {
                Ok(r) => r,
                Err(error) => {
                    let _ = print_fmt(format_args!("[nvme] BAR0 request failed: {:?}\n", error));
                    ostd::syscall::sys_exit(1)
                }
            };

            // 3. Initialise controller.
            let ctrl = match NvmeController::new(mmio_region, bdf) {
                Ok(c) => c,
                Err(error) => {
                    let _ = print_fmt(format_args!("[nvme] controller init failed: {:?}\n", error));
                    ostd::syscall::sys_exit(1)
                }
            };

            // Allocate the one-page I/O bounce buffer and retain the
            // device-visible address returned by authorization.
            let bounce = match BounceBuffer::new(bdf) {
                Ok(bounce) => bounce,
                Err(why) => {
                    let _ = print_fmt(format_args!("[nvme] io buffer {}\n", why));
                    ostd::syscall::sys_exit(1)
                }
            };
            if let Some(page) = bounce.page() {
                let _ = print_fmt(format_args!("[nvme] io buffer iova=0x{:x}\n", page.iova()));
            }
            let _ = print_fmt(format_args!(
                "[nvme] DMA authorized for bus {} device {} function {}\n",
                (bdf >> 8) & 0xFF,
                (bdf >> 3) & 0x1F,
                bdf & 0x07
            ));

            // Publish state before registration: VFS may route requests as soon
            // as the syscall succeeds.
            *STATE.lock() = Some(NvmeState { ctrl, bounce });
            if let Err(error) = sys_register_block_device(BlockDevice::Pci { bdf: bdf as u16 }) {
                *STATE.lock() = None;
                let _ = print_fmt(format_args!(
                    "[nvme] block driver registration failed: {:?}\n",
                    error
                ));
                ostd::syscall::sys_exit(1);
            }
        }

        // VFS speaks the raw DrvRequest protocol (no 0xAC App-SDK envelope), so
        // requests arrive as RawMessage; accept Message too — layout is identical.
        // Without the RawMessage arm the request falls into `_ => {}` and VFS
        // blocks forever in its reply recv (x86 FAT-on-NVMe boot hang).
        // Only the registered VFS may issue block requests; any other sender gets
        // `NotIssued` without touching the device.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if !sender_is_vfs(sender_tid) {
                let _ = print_fmt(format_args!(
                    "[nvme] refused block request from tid={}: sender is not the registered VFS\n",
                    sender_tid
                ));
                reply[0] = types::block_wire::BlockOutcome::NotIssued.to_byte();
                1
            } else if let Some(state) = STATE.lock().as_mut() {
                dispatch::handle(
                    &mut state.ctrl,
                    &mut state.bounce,
                    data.as_ref(),
                    &mut reply,
                )
            } else {
                reply[0] = types::block_wire::BlockOutcome::NotIssued.to_byte();
                1 // not initialised
            };
            let _ = ostd::ipc::reply_to(sender_tid, &reply[..len]);
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
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
