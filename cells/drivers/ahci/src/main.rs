//! AHCI Driver Cell — Tier-1 Privileged Driver Cell (x86_64 PC lane).
//!
//! Owns the SATA controller in AHCI mode exclusively and performs the whole
//! AHCI family:
//!   1. `sys_find_pcie_device(0x01/0x06/0x01)` locates the controller. A SATA
//!      controller whose prog-if is not AHCI (vendor/RAID firmware) **fails
//!      closed** with a log line naming the prog-if.
//!   2. Claims the ABAR MMIO window via `sys_request_mmio`. ICH9/PCH expose it at
//!      BAR5, so this cell uses `PcieDeviceInfo::bar_mem_base` (the first memory
//!      BAR), not `bar0_base`.
//!   3. Resets the HBA, starts the first port carrying an ATA disk, and completes
//!      one IDENTIFY DEVICE. All DMA structures go through `DmaBuf` +
//!      `authorize` so the phase-05 IOMMU path applies without rework. Command
//!      completion waits park on an MSI when `AllocPcieIrq` grants one
//!      (`irq.rs`) and poll otherwise.
//!   4. Publishes the block route of its controller's BDF via
//!      `sys_register_block_device(BlockDevice::Pci { .. })` and serves the
//!      DrvRequest IPC (512-byte sector read/write/flush) — the same wire NVMe
//!      and virtio-blk use, so VFS and littlefs `/data` need no device-specific
//!      path. Only the registered VFS may send block requests.
//!
//! Law 4 exception: this cell uses `unsafe` for DMA memory access; MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/ahci` launch edge —
//! not via a manifest flag (all 8 flag bits in v1 are occupied).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod controller;
mod dispatch;
mod dma;
#[cfg(feature = "fault-inject")]
mod fault_inject;
mod irq;

use crate::dma::AuthorizedDma;
use controller::AhciController;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::app::{AppContext, AppEvent};
use ostd::dma::DmaBuf;
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{sys_find_pcie_device, sys_register_block_device, BlockDevice, PcieDeviceInfo};
use types::ViError;

/// SATA controller class triple.
const SATA_CLASS: u8 = 0x01;
const SATA_SUB: u8 = 0x06;
/// AHCI 1.0 interface. Any other prog-if is refused.
const AHCI_PROGIF: u8 = 0x01;
/// Non-AHCI prog-ifs probed only to name the refusal: 0x00 vendor-specific,
/// 0x02 Serial Storage Bus (RAID-only firmware mode).
const NON_AHCI_PROGIFS: [u8; 2] = [0x00, 0x02];

/// ICH9/PCH AHCI MMIO window (ABAR). The cell never touches registers beyond it.
const AHCI_BAR_LEN: usize = 0x2000;
/// Smallest window that still covers the generic host control block and the
/// first port register block.
const AHCI_BAR_MIN: usize = 0x400;

/// The live controller plus its reusable sector-I/O DMA buffer.
struct AhciState {
    ctrl: AhciController,
    /// One authorized 512-byte sector buffer shared by every DrvRequest, so no
    /// per-request allocation happens on the block path.
    io_buf: AuthorizedDma<DmaBuf>,
}

static STATE: Mutex<Option<AhciState>> = Mutex::new(None);

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
            // The Platform Cell registers devices concurrently with this cell's
            // spawn; retry for a bounded window before concluding absent.
            let mut info = PcieDeviceInfo::zeroed();
            let mut found = false;
            for _ in 0..200 {
                if let Ok(true) = sys_find_pcie_device(SATA_CLASS, SATA_SUB, AHCI_PROGIF, &mut info)
                {
                    found = true;
                    break;
                }
                ostd::task::yield_now();
            }
            if !found {
                if let Some(prog_if) = refuse_non_ahci_progif() {
                    // Fail closed: RST/RAID-only firmware has no AHCI register
                    // interface, so no driver in this cell can be correct.
                    let _ = print_fmt(format_args!(
                        "[ahci] refusing SATA controller prog-if 0x{:02x} (AHCI 0x01 required)\n",
                        prog_if
                    ));
                    ostd::syscall::sys_exit(1);
                }
                // Exit 0: nothing to claim (no AHCI controller on this machine).
                let _ = print_fmt(format_args!(
                    "[ahci] no AHCI controller present; driver cell idle\n"
                ));
                ostd::syscall::sys_exit(0);
            }

            let bar_base = info.bar_mem_base as usize;
            let bar_len = if info.bar_mem_len == 0 {
                AHCI_BAR_LEN
            } else {
                (info.bar_mem_len as usize).min(AHCI_BAR_LEN)
            };
            if bar_base == 0 || bar_len < AHCI_BAR_MIN {
                let _ = print_fmt(format_args!(
                    "[ahci] controller has no usable MMIO BAR (base=0x{:x} len=0x{:x})\n",
                    bar_base, bar_len
                ));
                ostd::syscall::sys_exit(1);
            }

            let region = match mmio::request_region(bar_base, bar_len) {
                Ok(r) => r,
                Err(error) => {
                    let _ = print_fmt(format_args!("[ahci] ABAR request failed: {:?}\n", error));
                    ostd::syscall::sys_exit(1);
                }
            };
            let _ = print_fmt(format_args!(
                "[ahci] ABAR claim ok base=0x{:x} len=0x{:x}\n",
                bar_base, bar_len
            ));

            match AhciController::new(region, info.bdf) {
                Ok(ctrl) => {
                    // Reusable DMA buffer for sector transfers, authorized through
                    // the same `DmaBuf::authorize` path as the HBA structures so
                    // phase-05's IOMMU work applies without rework.
                    let io_buf = match DmaBuf::alloc(1) {
                        Some(buf) => buf,
                        None => {
                            let _ = print_fmt(format_args!(
                                "[ahci] sector I/O buffer allocation failed\n"
                            ));
                            ostd::syscall::sys_exit(1)
                        }
                    };
                    let io_buf = match AuthorizedDma::authorize(io_buf, |b| b.authorize(info.bdf)) {
                        Ok(buf) => buf,
                        Err(_) => {
                            let _ = print_fmt(format_args!(
                                "[ahci] sector I/O buffer authorization failed\n"
                            ));
                            ostd::syscall::sys_exit(1);
                        }
                    };
                    let port = ctrl.port();
                    let sectors = ctrl.capacity_sectors();
                    // Publish state before registration: VFS may route requests
                    // as soon as the syscall succeeds.
                    *STATE.lock() = Some(AhciState { ctrl, io_buf });
                    if let Err(error) = sys_register_block_device(BlockDevice::Pci {
                        bdf: info.bdf as u16,
                    }) {
                        *STATE.lock() = None;
                        let _ = print_fmt(format_args!(
                            "[ahci] block driver registration failed: {:?}\n",
                            error
                        ));
                        ostd::syscall::sys_exit(1);
                    }
                    let _ = print_fmt(format_args!(
                        "[driver_cell] ahci storage driver ready (port={} sectors={} identify ok)\n",
                        port, sectors
                    ));
                }
                // A controller with no ATA disk attached (q35 always exposes
                // ICH9 AHCI, even with no `-device ide-hd`) is an idle machine,
                // not a driver failure. Exit 0: nothing to claim, so every
                // diskless x86 boot stays clean.
                Err(ViError::NotFound) => {
                    let _ = print_fmt(format_args!(
                        "[ahci] no SATA disk attached; driver cell idle\n"
                    ));
                    ostd::syscall::sys_exit(0);
                }
                Err(error) => {
                    let _ = print_fmt(format_args!(
                        "[ahci] controller bring-up failed: {:?}\n",
                        error
                    ));
                    ostd::syscall::sys_exit(1);
                }
            }
        }

        // VFS speaks the raw DrvRequest protocol (no 0xAC App-SDK envelope), so
        // requests arrive as RawMessage; accept Message too — layout is identical.
        // Without the RawMessage arm the request falls into `_ => {}` and VFS
        // blocks forever in its reply recv (the x86 FAT-on-NVMe boot hang).
        // Only the registered VFS may issue block requests; any other sender gets
        // `NotIssued` without touching the device.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if !sender_is_vfs(sender_tid) {
                let _ = print_fmt(format_args!(
                    "[ahci] refused block request from tid={}: sender is not the registered VFS\n",
                    sender_tid
                ));
                reply[0] = types::block_wire::BlockOutcome::NotIssued.to_byte();
                1
            } else if let Some(state) = STATE.lock().as_mut() {
                dispatch::handle(&mut state.ctrl, &state.io_buf, data.as_ref(), &mut reply)
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

/// If a SATA-class controller is present with a non-AHCI prog-if, return it so
/// the caller can log the refusal. Probes only the two known non-AHCI values.
fn refuse_non_ahci_progif() -> Option<u8> {
    for prog_if in NON_AHCI_PROGIFS {
        let mut info = PcieDeviceInfo::zeroed();
        for _ in 0..200 {
            if let Ok(true) = sys_find_pcie_device(SATA_CLASS, SATA_SUB, prog_if, &mut info) {
                return Some(prog_if);
            }
            ostd::task::yield_now();
        }
    }
    None
}

ostd::run_app!(handler);

// ── Capability manifest ───────────────────────────────────────────────────────
// PcieDriverCap is granted by init via the reviewed `/bin/ahci` launch edge
// (not a manifest flag). This manifest declares NO privileged flags.
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// ── Syscall allowlist ─────────────────────────────────────────────────────────
// `run_app!` emits a manifest but NOT a `VICELL_SYSCALLS` section (see
// libs/ostd/src/lib.rs), so without this the kernel leaves the cell at its TCB's
// `u64::MAX` allowlist (kernel/src/task/syscall.rs). List exactly what the cell
// calls: the AppContext base set (its `run()` loop uses Recv; `LookupServiceBound`
// also resolves the VFS sender), the PCIe device path, block-route
// registration, request replies, the timeout polling in controller.rs
// (`GetTime`), the exit path, the DMA calls behind `DmaBuf::alloc`/
// `authorize`, and the MSI completion path (`AllocPcieIrq` at init,
// `WaitCompletion(IRQ)` parks in irq.rs). `Exit`/`Yield` carry no allowlist
// bit and are always permitted; naming them documents the intent (the macro
// skips them). Fault-injection builds add `ShmAlloc`/`ShmMap` for the
// `F09C-DIE-SHM` seam only.
macro_rules! ahci_syscalls {
    ($($extra:ident),*) => {
        abi::declare_syscalls![
            // AppContext event-loop base set.
            Send,
            SendGather,
            Recv,
            TryRecv,
            Log,
            Heartbeat,
            LookupServiceBound,
            GetTime,
            RecvTimeout,
            // PCIe device path.
            FindPcieDevice,
            RequestMmio,
            // Block-route registration (syscall 416).
            RegisterBlockDriver,
            // Clean shutdown returns through the exit path.
            Exit,
            // DMA: `DmaBuf::alloc` -> GrantAlloc, `DmaBuf::authorize` -> GrantDma.
            GrantAlloc,
            GrantDma,
            // MSI completion: allocation (435) and the bounded IRQ park (242).
            AllocPcieIrq,
            WaitCompletion
            $(, $extra)*
        ];
    };
}
#[cfg(not(feature = "fault-inject"))]
ahci_syscalls!();
#[cfg(feature = "fault-inject")]
ahci_syscalls!(ShmAlloc, ShmMap);
