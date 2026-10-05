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
//!      one polled IDENTIFY DEVICE. All DMA structures go through `DmaBuf` +
//!      `authorize` so the phase-05 IOMMU path applies without rework.
//!   4. Registers as the system block driver via `sys_register_block_driver` and
//!      serves the DrvRequest IPC (512-byte sector read/write/flush) — the same
//!      registration surface NVMe and virtio-blk use, so VFS and littlefs `/data`
//!      need no device-specific path.
//!
//! Law 4 exception: this cell uses `unsafe` for DMA memory access; MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/ahci` launch edge —
//! not via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
extern crate alloc;

mod controller;
mod dispatch;
mod dma;

use crate::dma::AuthorizedDma;
use controller::AhciController;
use ostd::app::{AppContext, AppEvent};
use ostd::dma::DmaBuf;
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device, sys_register_block_driver, sys_send, PcieDeviceInfo,
};
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

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // The Platform Cell registers devices concurrently with this cell's
            // spawn; retry for a bounded window before concluding absent.
            let mut info = PcieDeviceInfo::zeroed();
            let mut found = false;
            for _ in 0..200 {
                if let Ok(true) = sys_find_pcie_device(SATA_CLASS, SATA_SUB, AHCI_PROGIF, &mut info) {
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
                    let io_buf = match AuthorizedDma::authorize(io_buf, |b| b.authorize(info.bdf))
                    {
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
                    if let Err(error) = sys_register_block_driver() {
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
                // not a driver failure: log and exit cleanly so every diskless
                // x86 boot stays clean.
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
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
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
api::declare_manifest!(
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
// calls: the AppContext base set (its `run()` loop uses Recv), the PCIe device
// path, block-driver registration, request replies, the timeout polling in
// controller.rs (`GetTime`), the exit path, and the DMA calls behind
// `DmaBuf::alloc`/`authorize`. `Exit`/`Yield` carry no allowlist bit and are
// always permitted; naming them documents the intent (the macro skips them).
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
    // System block-driver role.
    RegisterBlockDriver,
    // Clean shutdown returns through the exit path.
    Exit,
    // DMA: `DmaBuf::alloc` -> GrantAlloc, `DmaBuf::authorize` -> GrantDma.
    GrantAlloc,
    GrantDma
];
