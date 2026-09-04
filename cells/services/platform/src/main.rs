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

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::string::String;

mod scan;

use ostd::app::{AppContext, AppEvent};
use ostd::io::{print, println};
use ostd::mmio::request_region;
use ostd::syscall::sys_exit;

/// ECAM bus-0 window size: 1 MiB (32 devices × 8 functions × 4 KiB).
const ECAM_BUS0_SIZE: usize = 0x10_0000;

// Syscall allowlist stays explicit: app_entry!'s generated set (app_syscall_set)
// lacks RequestMmio/RegisterPcieBar/RegisterPciDevice, which this cell needs.
// Must come before run_app! (run_app! does not emit VICELL_SYSCALLS).
api::declare_syscalls![Log, RequestMmio, RegisterPcieBar, RegisterPciDevice, StateRestore];

// No privileged manifest flags: PlatformCap is granted by path match, not here.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// One-shot cell: scan ECAM in Init, then exit before the recv loop starts.
// run_app! fires AppEvent::Init once before the first sys_recv (run_with_lifecycle);
// sys_exit(0) is -> ! so the event loop never runs.
#[cfg(not(test))]
ostd::run_app!(on_event);

fn on_event(_ctx: &mut AppContext, event: AppEvent) {
    if let AppEvent::Init = event {
        scan_ecam();
        sys_exit(0);
    }
}

fn scan_ecam() {
    let (ecam_base, bus_start, bus_end) = get_ecam_config();

    if ecam_base == 0 {
        println("[platform] ECAM unavailable; x86 requires kernel ACPI MCFG discovery");
        return;
    }

    // Claim the ECAM bus-0 window. This call goes through the PlatformCap bypass
    // in the kernel RequestMmio handler (no allowlist check, overlap check only).
    let region = match request_region(ecam_base, ECAM_BUS0_SIZE) {
        Ok(r) => r,
        Err(_) => {
            println("[platform] ECAM MMIO claim failed — kernel fallback active");
            return;
        }
    };

    if bus_end > bus_start {
        print("[platform] ECAM scan buses ");
        print_u8(bus_start);
        print("-");
        print_u8(bus_end);
        println(" starting (bus 0 active)");
    } else {
        println("[platform] ECAM scan bus 0 starting");
    }
    scan::scan_and_register(&region);
    println("[platform] ECAM scan complete");

    // `region` drops here — releases the ECAM MMIO claim (H3 one-shot semantics).
    // No other cell can re-claim this range after Platform Cell exits.
}

fn get_ecam_config() -> (usize, u8, u8) {
    let args = ostd::args::args();
    parse_ecam_args(&args)
}

fn parse_ecam_args(args: &[String]) -> (usize, u8, u8) {
    let mut base = 0usize;
    let mut bus_start = 0u8;
    let mut bus_end = 0u8;

    for arg in args {
        if let Some(val) = arg.strip_prefix("--ecam-base=") {
            if let Some(b) = parse_address(val) {
                base = b;
            }
        } else if let Some(val) = arg.strip_prefix("--bus-start=") {
            if let Ok(b) = val.parse::<u8>() {
                bus_start = b;
            }
        } else if let Some(val) = arg.strip_prefix("--bus-end=") {
            if let Ok(b) = val.parse::<u8>() {
                bus_end = b;
            }
        }
    }

    for i in 0..args.len() {
        if args[i] == "--ecam-base" && i + 1 < args.len() {
            if let Some(b) = parse_address(&args[i + 1]) {
                base = b;
            }
        } else if args[i] == "--bus-start" && i + 1 < args.len() {
            if let Ok(b) = args[i + 1].parse::<u8>() {
                bus_start = b;
            }
        } else if args[i] == "--bus-end" && i + 1 < args.len() {
            if let Ok(b) = args[i + 1].parse::<u8>() {
                bus_end = b;
            }
        }
    }

    if base == 0 {
        #[cfg(target_arch = "riscv64")]
        {
            base = 0x3000_0000;
        }
    }

    (base, bus_start, bus_end)
}

fn parse_address(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<usize>().ok()
    }
}

fn print_u8(mut val: u8) {
    if val == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 3];
    let mut i = 0;
    while val > 0 {
        buf[i] = b'0' + (val % 10);
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        let c = buf[i] as char;
        let mut b = [0u8; 4];
        print(c.encode_utf8(&mut b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn parse_address_handles_hex_and_dec() {
        assert_eq!(parse_address("0xb0000000"), Some(0xb000_0000));
        assert_eq!(parse_address("0XB0000000"), Some(0xb000_0000));
        assert_eq!(parse_address("0x30000000"), Some(0x3000_0000));
        assert_eq!(parse_address("2952790016"), Some(0xb000_0000));
        assert_eq!(parse_address("0"), Some(0));
        assert_eq!(parse_address("invalid"), None);
    }

    #[test]
    fn parse_ecam_args_equals_syntax() {
        let args = vec![
            "--ecam-base=0xb0000000".to_string(),
            "--bus-start=0".to_string(),
            "--bus-end=255".to_string(),
        ];
        let (base, bus_start, bus_end) = parse_ecam_args(&args);
        assert_eq!(base, 0xb000_0000);
        assert_eq!(bus_start, 0);
        assert_eq!(bus_end, 255);
    }

    #[test]
    fn parse_ecam_args_space_separated() {
        let args = vec![
            "--ecam-base".to_string(),
            "0xb0000000".to_string(),
            "--bus-start".to_string(),
            "0".to_string(),
            "--bus-end".to_string(),
            "1".to_string(),
        ];
        let (base, bus_start, bus_end) = parse_ecam_args(&args);
        assert_eq!(base, 0xb000_0000);
        assert_eq!(bus_start, 0);
        assert_eq!(bus_end, 1);
    }

    #[test]
    fn parse_ecam_args_empty_defaults() {
        let args = vec![];
        let (base, bus_start, bus_end) = parse_ecam_args(&args);
        #[cfg(target_arch = "riscv64")]
        assert_eq!(base, 0x3000_0000);
        #[cfg(not(target_arch = "riscv64"))]
        assert_eq!(base, 0);
        assert_eq!(bus_start, 0);
        assert_eq!(bus_end, 0);
    }
}
