//! x86 q35 PCIe multi-bus integration proof.
//!
//! Places the DMA-backed NVMe endpoint behind a root port at `01:00.0` and
//! verifies both ordinary discovery and Intel VT-d queue/Identify DMA from that
//! same requester ID. A VMware-shaped topology (AHCI behind a conventional PCI
//! bridge, NICs behind root ports) checks that a function on bus ≥ 1 keeps every
//! BAR and meets the same Ethernet family gate as bus 0.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, x86_iso_path, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const BUS1_NVME: &str = "[platform] registered bus 1 device 0 function 0 class 1:8:2";
const BUS1_DMA: &str = "[nvme] DMA authorized for bus 1 device 0 function 0";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn iso_path() -> String {
    x86_iso_path(&repo_root())
}

fn prerequisites_ok() -> bool {
    let iso_ok = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_ok {
        eprintln!("SKIP pcie-multibus-x86: x86 ISO not built ({})", iso_path());
    }
    if !qemu_ok {
        eprintln!("SKIP pcie-multibus-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "cellos_pcie_multibus_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .expect("create NVMe disk image");
    file.set_len(64 * 1024 * 1024).expect("set NVMe disk size");
    path
}

#[test]
fn bus1_nvme_is_discovered_and_initialized() {
    if !prerequisites_ok() {
        return;
    }
    let disk = make_nvme_disk();
    let qemu =
        QemuRunner::boot_x86_bios_with_multibus_nvme(&iso_path(), &disk.to_string_lossy(), false);
    qemu.wait_for(BUS1_NVME, BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("bus-1 NVMe was not registered: {error}\n{}", qemu.dump()));
    qemu.wait_for("[driver_cell] block driver registered", BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("bus-1 NVMe did not initialize: {error}\n{}", qemu.dump()));
    let serial = qemu.dump();
    assert!(!serial.contains("[KERNEL PANIC]"), "{serial}");
    let _ = std::fs::remove_file(disk);
}

#[test]
fn bus1_nvme_dma_completes_under_vtd() {
    if !prerequisites_ok() {
        return;
    }
    let disk = make_nvme_disk();
    let qemu =
        QemuRunner::boot_x86_bios_with_multibus_nvme(&iso_path(), &disk.to_string_lossy(), true);
    qemu.wait_for("[vtd] Intel VT-d: DMA isolation ACTIVE", BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("VT-d did not activate: {error}\n{}", qemu.dump()));
    qemu.wait_for(BUS1_NVME, BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("bus-1 NVMe was not registered: {error}\n{}", qemu.dump()));
    qemu.wait_for(BUS1_DMA, BOOT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!(
                "bus-1 VT-d DMA mapping was not authorized: {error}\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("[driver_cell] block driver registered", BOOT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!("bus-1 NVMe DMA did not complete: {error}\n{}", qemu.dump())
        });

    let serial = qemu.dump();
    let active = serial
        .find("[vtd] Intel VT-d: DMA isolation ACTIVE")
        .expect("VT-d marker exists");
    let mapping = serial
        .find(BUS1_DMA)
        .expect("bus-1 DMA authorization exists");
    let initialized = serial
        .find("[driver_cell] block driver registered")
        .expect("block driver marker exists");
    assert!(
        active < mapping && mapping < initialized,
        "invalid VT-d/DMA order:\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "{serial}");
    let _ = std::fs::remove_file(disk);
}

/// The VMware guest topology (S3 findings S1/S2), reproduced on q35: AHCI
/// behind a conventional PCI bridge decodes its ABAR at BAR5, which the
/// Platform-Cell registration once dropped (`no usable MMIO BAR (base=0x0)`).
/// Ethernet functions behind root ports follow the kernel's family gate: the
/// 82540EM registers and binds, the PCnet is refused by ID.
#[test]
fn bridged_ahci_and_root_port_nics_follow_the_scan_rules() {
    if !prerequisites_ok() {
        return;
    }
    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_bridged_devices(&iso_path(), &disk.to_string_lossy());
    for marker in [
        "[platform] registered bus 1 device 1 function 0 class 1:6:1",
        "[platform] registered bus 2 device 0 function 0 class 2:0:0",
        "[platform] registration refused bus 3 device 0 function 0 id 1022:2000 class 2:0:0",
        "[ahci] controller bound",
        "[ahci] IDENTIFY DEVICE ok",
        "[driver_cell] NIC driver registered",
        "[net] DHCP acquired",
        "Cellos >",
    ] {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|error| panic!("{marker:?} not seen: {error}\n{}", qemu.dump()));
    }
    let serial = qemu.dump();
    assert!(!serial.contains("no usable MMIO BAR"), "{serial}");
    assert!(!serial.contains("[KERNEL PANIC]"), "{serial}");
    let _ = std::fs::remove_file(disk);
}
