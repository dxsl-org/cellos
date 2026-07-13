//! x86_64 PCIe NIC (e1000) + Intel VT-d integration tests — Driver Cell
//! architecture.
//!
//! The kernel drives no NIC hardware (Kernel Boundary Law): the Platform Cell
//! scans ECAM, init spawns the e1000 Driver Cell (`/bin/e1000`), which claims
//! BAR0 MMIO and announces itself via `sys_register_nic_driver`. The oracle is
//! the kernel registration marker `[driver_cell] NIC driver registered`.
//!
//! `nic_x86_e1000_init` — boots QEMU q35 with `-device e1000` and asserts the
//! Driver Cell registration.
//!
//! `nic_x86_vtd_enabled` — same boot plus `-device intel-iommu`; asserts the
//! deferred VT-d activation (`[vtd] Intel VT-d: DMA isolation ACTIVE`, fired
//! from the Platform Cell's RegisterPciDevice path) AND that BOTH Driver
//! Cells still register with translation enabled. The NVMe registration is
//! the strong oracle: it requires Identify/queue DMA round-trips through the
//! per-Cell VT-d SLPT, proving DMA isolation actually translates (a malformed
//! context entry — the original AW-in-lo bug — fails exactly this).
//!
//! Both tests skip gracefully when the x86_64 ISO is not built or
//! `qemu-system-x86_64` is not on PATH.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn iso_path() -> String {
    repo_root()
        .join("build/vicell-x86.iso")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let iso_ok = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();

    if !iso_ok {
        eprintln!(
            "SKIP nic-x86: x86_64 ISO not built ({})\n\
             Build with: scripts/build-x86_64-cells.ps1 then build/make-iso.sh",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP nic-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "vicell_nic_x86_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ));
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&path)
        .expect("create NVMe disk image");
    f.set_len(64 * 1024 * 1024).expect("set NVMe disk size");
    let _ = f.write_all(b"");
    path
}

/// The e1000 Driver Cell must bind the QEMU `-device e1000` NIC and register
/// as the system NIC driver.
///
/// Proves: Platform Cell ECAM scan finds class 02:00:00, the Driver Cell
/// claims BAR0 via user-mapped MMIO, initialises the controller, and calls
/// `sys_register_nic_driver`.
#[test]
fn nic_x86_e1000_init() {
    if !prerequisites_ok() { return; }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("[driver_cell] NIC driver registered", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "e1000 Driver Cell did not register within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: platform ECAM scan → find_pcie_device(02:00:00) → BAR0 MMIO \
                 claim → sys_register_nic_driver.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    let _ = std::fs::remove_file(&disk);
}

/// Intel VT-d deferred activation + e1000 Driver Cell on x86_64 q35.
///
/// Verifies the deferred IOMMU init fires from the Platform Cell's device
/// registration (GCAP probe, root/context tables, GCMD.SRTP + TE), and the
/// e1000 Driver Cell still registers with translation enabled.
#[test]
fn nic_x86_vtd_enabled() {
    if !prerequisites_ok() { return; }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_vtd(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("[vtd] Intel VT-d: DMA isolation ACTIVE", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "VT-d not activated within {BOOT_TIMEOUT}s: {e}\n\
                 Deferred init fires from RegisterPciDevice — check the Platform \
                 Cell spawned and -device intel-iommu precedes endpoint devices.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    // The NVMe Driver Cell must register after VT-d is active — its controller
    // init does real DMA (Identify + queue creation) THROUGH the per-Cell SLPT,
    // so this is the proof that VT-d translation actually works.
    qemu.wait_for("[driver_cell] block driver registered", 20)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "NVMe Driver Cell did not register under VT-d (DMA through SLPT broken): {e}\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    // The e1000 Driver Cell must also register after VT-d is active.
    qemu.wait_for("[driver_cell] NIC driver registered", 15)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "e1000 Driver Cell did not register after VT-d activation: {e}\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    let _ = std::fs::remove_file(&disk);
}
