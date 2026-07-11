//! x86_64 PCIe NVMe integration tests — Driver Cell architecture.
//!
//! Since the Kernel Boundary migration the kernel drives no block hardware:
//! the Platform Cell (`/bin/platform`, spawned by the kernel) scans ECAM and
//! registers devices/BARs; init spawns the NVMe Driver Cell (`/bin/nvme`),
//! which locates the controller via `sys_find_pcie_device`, claims BAR0 MMIO,
//! initialises the controller (admin + IO queues over DMA), and announces
//! itself via `sys_register_block_driver`.
//!
//! The oracle is the kernel's registration marker:
//!   `[driver_cell] block driver registered`
//! which is only reachable after the FULL chain (ECAM scan → BAR registration
//! → find → MMIO claim → controller RDY → Identify DMA round-trip → IO queue
//! creation) has succeeded.
//!
//! Tests skip gracefully when the ISO or `qemu-system-x86_64` is absent.

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
            "SKIP nvme-x86: x86_64 ISO not built ({})\n  Run: scripts/build-x86_64-cells.ps1 then .\\run-x86.ps1 -NoQemu",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP nvme-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "vicell_nvme_x86_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ));
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&path)
        .expect("create NVMe disk image");
    f.set_len(64 * 1024 * 1024).expect("set NVMe disk size");
    path
}

/// The NVMe Driver Cell must bind the QEMU `-device nvme` controller and
/// register as the system block driver.
///
/// Proves the whole PCIe storage chain under ring-3: Platform Cell ECAM scan,
/// BAR registration, `sys_find_pcie_device`, user-mapped BAR0 MMIO, controller
/// reset/enable, Identify Controller + Namespace DMA round-trips, and IO
/// queue-pair creation.
#[test]
fn nvme_driver_cell_registers_x86() {
    if !prerequisites_ok() { return; }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("[driver_cell] block driver registered", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "NVMe Driver Cell did not register within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: platform ECAM scan → find_pcie_device(01:08:02) → BAR0 MMIO \
                 claim → controller init → sys_register_block_driver.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    let _ = std::fs::remove_file(&disk);
}

/// Boot with an NVMe controller attached must still reach the interactive
/// shell — the Driver Cell path must not hang or fault the boot.
#[test]
fn nvme_boot_reaches_shell_x86() {
    if !prerequisites_ok() { return; }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "shell prompt not reached with NVMe attached: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    let _ = std::fs::remove_file(&disk);
}
