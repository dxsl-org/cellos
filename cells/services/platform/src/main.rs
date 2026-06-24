//! Platform Cell — PCIe ECAM enumeration (Tier-1 Trusted Cell).
//!
//! This cell holds the singleton `PlatformCap` (path-granted by the kernel
//! loader for `/bin/platform`). It:
//!
//!   1. Claims the per-arch ECAM bus-0 MMIO window via `sys_request_mmio`.
//!   2. Walks all 32 device slots, decodes MMIO BARs, and registers each via
//!      `sys_register_pcie_bar` (which populates `resource_registry::PCIE_BARS`).
//!   3. Releases the ECAM MMIO claim by dropping the `MmioRegion` (H3 one-shot
//!      semantics — no cell can re-claim ECAM after this).
//!   4. Exits cleanly so no resources are held.
//!
//! After this cell exits, Driver Cells (NVMe, e1000, virtio-net, …) can call
//! `sys_request_mmio` for individual device BARs they own via `PcieDriverCap`.
//!
//! # Architecture notes
//! - `#![forbid(unsafe_code)]`: MMIO access goes through `ostd::mmio::MmioRegion`
//!   (bounds-checked, volatile, safe-wrapped).
//! - No manifest privilege flags: `PlatformCap` is path-granted, not manifest-based.
//! - ARM64 virt uses VirtIO MMIO, not PCIe ECAM — this cell exits immediately on
//!   that architecture.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

mod scan;

use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::mmio::request_region;
use ostd::syscall::sys_exit;

// Syscall allowlist: only the syscalls this cell actually calls.
// Must come before run_app! (run_app! does not emit VICELL_SYSCALLS).
api::declare_syscalls![Log, RequestMmio, RegisterPcieBar];

// No privileged manifest flags: PlatformCap is granted by path match, not here.
api::declare_manifest!(
    block_io = false, network = false, spawn = false,
    gpio = false, uart = false, hypervisor = false
);

/// ECAM bus-0 window size: 1 MiB (32 devices × 8 functions × 4 KiB).
const ECAM_BUS0_SIZE: usize = 0x10_0000;

ostd::run_app!(handle_event);

fn handle_event(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            scan_ecam();
            sys_exit(0);
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

fn scan_ecam() {
    #[cfg(target_arch = "x86_64")]
    const ECAM_BASE: usize = 0xB000_0000;
    #[cfg(target_arch = "riscv64")]
    const ECAM_BASE: usize = 0x3000_0000;
    // ARM64 virt machine uses VirtIO MMIO, not PCIe ECAM — skip.
    #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
    const ECAM_BASE: usize = 0;

    if ECAM_BASE == 0 {
        println("[platform] no PCIe ECAM on this architecture — exiting");
        return;
    }

    // Claim the ECAM bus-0 window. This call goes through the PlatformCap bypass
    // in the kernel RequestMmio handler (no allowlist check, overlap check only).
    let region = match request_region(ECAM_BASE, ECAM_BUS0_SIZE) {
        Ok(r)  => r,
        Err(_) => {
            println("[platform] ECAM MMIO claim failed — kernel fallback active");
            return;
        }
    };

    println("[platform] ECAM scan bus 0 starting");
    scan::scan_and_register(&region);
    println("[platform] ECAM scan complete");

    // `region` drops here — releases the ECAM MMIO claim (H3 one-shot semantics).
    // No other cell can re-claim this range after Platform Cell exits.
}
