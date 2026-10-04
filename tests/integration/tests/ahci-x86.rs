//! x86_64 ICH9 AHCI integration tests — Driver Cell architecture (phase 02a).
//!
//! The Platform Cell (`/bin/platform`, spawned by the kernel) scans ECAM and
//! registers devices/BARs; init spawns the AHCI Driver Cell (`/bin/ahci`),
//! which locates the `01:06:01` SATA controller via `sys_find_pcie_device`,
//! claims its ABAR (the first *memory* BAR — ICH9 exposes it at BAR5) through
//! `sys_request_mmio`, resets the HBA, brings a port up, and completes one
//! polled IDENTIFY DEVICE over DMA.
//!
//! The oracles are the cell's own markers:
//!   `[ahci] controller bound`
//!   `[ahci] IDENTIFY DEVICE ok`
//!   `[driver_cell] ahci storage driver ready`
//! which are only reachable after the full chain (ECAM scan → BAR registration
//! → find → ABAR claim → HBA reset → port link-up → IDENTIFY DMA round-trip)
//! has succeeded.
//!
//! Phase 02a has no data path and no block registration, so these tests assert
//! bring-up only. Tests skip gracefully when the ISO or `qemu-system-x86_64` is
//! absent.

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

/// The ISO carrying the AHCI lane's `/bin/ahci`. `VICELL_AHCI_ISO` overrides;
/// otherwise the phase gate's lane ISO is used, falling back to the shared
/// production ISO (which skips if that older image has no AHCI cell).
fn iso_path() -> String {
    if let Ok(path) = std::env::var("VICELL_AHCI_ISO") {
        return path;
    }
    let lane = repo_root().join("build/x86-pc-lane/vicell-x86-ahci.iso");
    if lane.exists() {
        return lane.to_string_lossy().into_owned();
    }
    repo_root()
        .join("build/vicell-x86.iso")
        .to_string_lossy()
        .into_owned()
}

/// Provenance check for the ISO: the kernel embeds its launch-path table (and
/// the cell FAT image), so a kernel built with the AHCI lane carries the literal
/// `/bin/ahci`. An ISO without it cannot exercise the driver, and treating that
/// as a pass would be a false green.
fn iso_carries_ahci_cell() -> bool {
    std::fs::read(iso_path())
        .map(|bytes| bytes.windows(9).any(|w| w == b"/bin/ahci"))
        .unwrap_or(false)
}

fn prerequisites_ok() -> bool {
    let iso_ok = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    let cell_ok = iso_ok && iso_carries_ahci_cell();
    if iso_ok && !cell_ok {
        eprintln!(
            "SKIP ahci-x86: ISO does not carry /bin/ahci ({})\n  Rebuild: pwsh scripts/build-x86_64-cells.ps1, then the board-x86-pc kernel + ISO",
            iso_path()
        );
    }
    if !iso_ok {
        eprintln!(
            "SKIP ahci-x86: x86_64 ISO not built ({})\n  Run: pwsh scripts/build-x86_64-cells.ps1, then build the board-x86-pc kernel + ISO",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP ahci-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok && cell_ok)
}

/// Removes the temp SATA image on every exit path, including a panic inside the
/// runner constructor (which would otherwise leak a 64 MiB file per attempt).
struct TempImage(PathBuf);

impl TempImage {
    /// A zeroed raw image is enough: IDENTIFY DEVICE reads device identity, not
    /// sector data (the data path is phase 02b).
    fn new() -> Self {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vicell_sata_x86_{}_{}.img",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)
            .expect("create SATA disk image");
        f.set_len(64 * 1024 * 1024).expect("set SATA disk size");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The AHCI Driver Cell must bind the q35 ICH9 SATA controller, bring a port
/// up, and complete IDENTIFY DEVICE.
#[test]
fn ahci_driver_cell_identifies_sata_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = TempImage::new();
    let qemu = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk.path().to_string_lossy());

    for marker in [
        "[ahci] controller bound",
        "[ahci] IDENTIFY DEVICE ok",
        "[driver_cell] ahci storage driver ready",
    ] {
        qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|e| {
            panic!(
                "AHCI Driver Cell marker {marker:?} not seen within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: platform ECAM scan → find_pcie_device(01:06:01) → first-memory-BAR \
                 (BAR5) MMIO claim → HBA reset → port link-up → IDENTIFY DEVICE DMA.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    }

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached after AHCI IDENTIFY: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
}

/// Booting with a SATA image attached must still reach the interactive shell —
/// the AHCI bring-up must not hang or fault the boot.
#[test]
fn ahci_boot_reaches_shell_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = TempImage::new();
    let qemu = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk.path().to_string_lossy());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached with SATA attached: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
}
