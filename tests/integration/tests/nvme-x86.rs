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

/// VFS must mount a FAT32 volume served by the NVMe Driver Cell.
///
/// Builds a disk with a FAT32 filesystem at `PART_FAT32_BASE_LBA` (2048 → byte
/// offset 1 MiB) using `tools/mkfat32_inplace.py`, boots with it attached as
/// the NVMe drive, and asserts the mount marker. This exercises real sector
/// reads over the DrvRequest IPC + NVMe DMA path (BPB probe, FAT, root dir) —
/// the chain that silently broke when the nvme cell only matched
/// `AppEvent::Message` (raw DrvRequests arrive as `RawMessage`).
///
/// Skips (in addition to the usual ISO/QEMU checks) when `python` is not on
/// PATH — the FAT32 formatter is a Python tool.
#[test]
fn nvme_fat32_mount_x86() {
    if !prerequisites_ok() { return; }
    let python_ok = std::process::Command::new("python")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !python_ok {
        eprintln!("SKIP nvme_fat32_mount_x86: python not on PATH (needed for mkfat32_inplace.py)");
        return;
    }

    // 1. Format a 524,288-sector (256 MiB) FAT32 volume in a temp file.
    const FAT_SECTORS: u64 = 524_288; // == api::disk::PART_FAT32_SECTORS
    const FAT_BASE_OFFSET: u64 = 2_048 * 512; // PART_FAT32_BASE_LBA in bytes
    let fat_img = std::env::temp_dir().join(format!(
        "vicell_fat32_{}_{}.img",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
    ));
    {
        let f = std::fs::OpenOptions::new()
            .write(true).create(true).open(&fat_img)
            .expect("create FAT32 scratch image");
        f.set_len(FAT_SECTORS * 512).expect("size FAT32 scratch image");
    }
    let mkfat = repo_root().join("tools/mkfat32_inplace.py");
    let status = std::process::Command::new("python")
        .arg(&mkfat)
        .arg(&fat_img)
        .arg(FAT_SECTORS.to_string())
        .status()
        .expect("run mkfat32_inplace.py");
    assert!(status.success(), "mkfat32_inplace.py failed");

    // 2. Splice the volume into an NVMe disk at the partition offset.
    let disk = make_nvme_disk();
    {
        use std::io::{Seek, SeekFrom, Write};
        let fat_bytes = std::fs::read(&fat_img).expect("read FAT32 image");
        let mut d = std::fs::OpenOptions::new()
            .write(true).open(&disk)
            .expect("open NVMe disk for splice");
        d.set_len(FAT_BASE_OFFSET + FAT_SECTORS * 512 + 1024 * 1024)
            .expect("grow NVMe disk");
        d.seek(SeekFrom::Start(FAT_BASE_OFFSET)).expect("seek to partition base");
        d.write_all(&fat_bytes).expect("write FAT32 volume");
    }
    let _ = std::fs::remove_file(&fat_img);

    // 3. Boot and assert the mount marker.
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());
    qemu.wait_for("[vfs] FAT32 /mnt/sd volume mounted", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "FAT32-on-NVMe mount not seen within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: nvme Driver Cell registers pre-VFS → VFS blk_router IPC → \
                 DrvRequest sector reads over NVMe DMA → fatfs BPB accept.\n\
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
