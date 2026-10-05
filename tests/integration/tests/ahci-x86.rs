//! x86_64 ICH9 AHCI integration tests — Driver Cell architecture.
//!
//! The Platform Cell (`/bin/platform`, spawned by the kernel) scans ECAM and
//! registers devices/BARs; init spawns the AHCI Driver Cell (`/bin/ahci`),
//! which locates the `01:06:01` SATA controller via `sys_find_pcie_device`,
//! claims its ABAR (the first *memory* BAR — ICH9 exposes it at BAR5) through
//! `sys_request_mmio`, resets the HBA, brings a port up, and completes one
//! polled IDENTIFY DEVICE over DMA.
//!
//! Part B adds the data path and block registration: after IDENTIFY the cell
//! calls `sys_register_block_driver` (the same surface NVMe/virtio-blk use) and
//! serves the DrvRequest IPC with READ/WRITE DMA EXT and FLUSH CACHE EXT. The
//! oracles are:
//!   `[ahci] controller bound`
//!   `[ahci] IDENTIFY DEVICE ok`
//!   `[driver_cell] ahci storage driver ready`
//!   `[driver_cell] block driver registered`
//!   `[vfs] FAT32 /mnt/sd volume mounted`
//! and the two-boot persistence test: a marker written through the shell in the
//! first boot must be readable in a second boot on the same image.
//!
//! Tests skip gracefully when the ISO or `qemu-system-x86_64` is absent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 15;

/// `api::disk::PART_FAT32_BASE_LBA` — byte offset 1 MiB.
const FAT_BASE_OFFSET: u64 = 2_048 * 512;
/// `api::disk::PART_FAT32_SECTORS`.
const FAT_SECTORS: u64 = 524_288;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The ISO carrying the AHCI lane's `/bin/ahci`. `VICELL_AHCI_ISO` overrides;
/// otherwise the phase gate's lane ISO is preferred in build order:
/// `vicell-x86-ahci-b.iso` (the phase-02b gate artifact), then
/// `vicell-x86-ahci.iso`, falling back to the shared production ISO (which skips
/// if that older image has no AHCI cell).
fn iso_path() -> String {
    if let Ok(path) = std::env::var("VICELL_AHCI_ISO") {
        return path;
    }
    for name in ["vicell-x86-ahci-b.iso", "vicell-x86-ahci.iso"] {
        let lane = repo_root().join("build/x86-pc-lane").join(name);
        if lane.exists() {
            return lane.to_string_lossy().into_owned();
        }
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

fn python_binary() -> Option<&'static str> {
    ["python", "python3"].into_iter().find(|binary| {
        std::process::Command::new(binary)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

/// Removes the temp SATA image on every exit path, including a panic inside the
/// runner constructor (which would otherwise leak a 64 MiB file per attempt).
struct TempImage(PathBuf);

impl TempImage {
    /// A zeroed raw image is enough for bring-up/registration: IDENTIFY DEVICE
    /// reads device identity, and a zeroed volume simply fails to mount.
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

/// A raw SATA image carrying a FAT32 volume at `PART_FAT32_BASE_LBA`, sized so
/// the volume's sector range fits (the `/bin` cell-store and `/data` littlefs
/// partitions live past the end and are simply absent on this lane).
struct FatSataImage(PathBuf);

impl FatSataImage {
    /// Format a FAT32 volume with `tools/mkfat32_inplace.py` and splice it into
    /// an otherwise-empty raw disk. Returns `None` (caller skips) when the
    /// formatter is missing or fails.
    fn new(python: &str) -> Option<Self> {
        let volume = std::env::temp_dir().join(format!(
            "vicell_sata_fat32_{}_{}.img",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&volume)
                .expect("create FAT32 scratch image");
            f.set_len(FAT_SECTORS * 512).expect("size FAT32 scratch image");
        }
        let status = std::process::Command::new(python)
            .arg(repo_root().join("tools/mkfat32_inplace.py"))
            .arg(&volume)
            .arg(FAT_SECTORS.to_string())
            .status()
            .expect("run mkfat32_inplace.py");
        if !status.success() {
            let _ = std::fs::remove_file(&volume);
            return None;
        }

        let disk = std::env::temp_dir().join(format!(
            "vicell_sata_x86_fat_{}_{}.img",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
                + 1
        ));
        {
            use std::io::{Seek, SeekFrom, Write};
            let fat_bytes = std::fs::read(&volume).expect("read FAT32 volume");
            let mut d = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&disk)
                .expect("create SATA disk image");
            d.set_len(FAT_BASE_OFFSET + FAT_SECTORS * 512 + 1024 * 1024)
                .expect("size SATA disk image");
            d.seek(SeekFrom::Start(FAT_BASE_OFFSET))
                .expect("seek to partition base");
            d.write_all(&fat_bytes).expect("write FAT32 volume");
        }
        let _ = std::fs::remove_file(&volume);
        Some(Self(disk))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for FatSataImage {
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

/// The AHCI Driver Cell must register as the system block driver through the
/// same surface NVMe uses: `[driver_cell] block driver registered`.
#[test]
fn ahci_driver_cell_registers_block_driver_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = TempImage::new();
    let qemu = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk.path().to_string_lossy());

    qemu.wait_for("[driver_cell] block driver registered", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "AHCI Driver Cell did not register as block driver within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: IDENTIFY ok → DmaBuf::authorize(sector buffer) → \
                 sys_register_block_driver → kernel role + service::BLOCK_DRIVER.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
}

/// Booting with a SATA image attached must still reach the interactive shell —
/// the AHCI bring-up and registration must not hang or fault the boot.
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

/// A FAT32 volume served by the AHCI Driver Cell must mount, accept a marker
/// written through the shell, and serve that marker back after a full reboot on
/// the **same** raw image.
///
/// This is the phase-02b persistence oracle: it exercises the full DrvRequest
/// chain over AHCI DMA (BPB probe, FAT, root dir, file data) in one direction,
/// then proves the WRITE DMA EXT output is durable by re-reading it from a fresh
/// QEMU instance.
///
/// Skips when Python is absent — the FAT32 formatter is a Python tool.
#[test]
fn ahci_fat32_persistence_reboot_x86() {
    if !prerequisites_ok() {
        return;
    }
    let Some(python) = python_binary() else {
        // A missing formatter is an environment gap: skip locally, hard-fail in
        // CI (otherwise the only oracle that touches writes can vanish while the
        // job stays green).
        let _ = vicell_integration_tests::ci_guard(false);
        eprintln!("SKIP ahci_fat32_persistence_reboot_x86: Python 3 not on PATH");
        return;
    };
    let image = FatSataImage::new(python).unwrap_or_else(|| {
        panic!(
            "mkfat32_inplace.py failed: the persistence oracle cannot run, which is a test \
             failure (the in-tree formatter is the test's own fixture), not an environment skip"
        )
    });
    let disk_path = image.path().to_string_lossy().into_owned();

    // ── First boot: mount, write, and read the marker back ───────────────────
    let mut qemu = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk_path);
    for marker in [
        "[ahci] IDENTIFY DEVICE ok",
        "[driver_cell] block driver registered",
        "[vfs] FAT32 /mnt/sd volume mounted",
        "Cellos >",
    ] {
        qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|e| {
            panic!(
                "first boot marker {marker:?} not seen within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: AHCI bring-up → register → VFS blk_router DrvRequest reads over \
                 READ DMA EXT → fatfs BPB accept.\n--- serial output ---\n{}",
                qemu.dump()
            )
        });
    }

    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("vwrite /mnt/sd/probe.txt phase02b-ahci-ok");
    qemu.wait_for_after("Cellos >", checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("AHCI vwrite did not complete: {e}\n{}", qemu.dump()));

    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("vcat /mnt/sd/probe.txt");
    qemu.wait_for_after("phase02b-ahci-ok", checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("AHCI read-back marker missing: {e}\n{}", qemu.dump()));

    // Stop boot 1. On x86 `shutdown` halts the CPU rather than powering QEMU
    // off, so the marker is durable because every FAT write was written through
    // to the device; dropping the runner kills QEMU and releases the image.
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("shutdown");
    let _ = qemu.wait_for_after("System shutting down", checkpoint, CMD_TIMEOUT);
    let first_boot = qemu.dump();
    drop(qemu);
    std::thread::sleep(std::time::Duration::from_millis(500));

    // ── Second boot: the SAME image must still hold the marker ───────────────
    let mut qemu2 = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk_path);
    for marker in [
        "[driver_cell] block driver registered",
        "[vfs] FAT32 /mnt/sd volume mounted",
        "Cellos >",
    ] {
        qemu2.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|e| {
            panic!(
                "second boot marker {marker:?} not seen: {e}\n--- first boot ---\n{}\n\
                 --- second boot ---\n{}",
                first_boot,
                qemu2.dump()
            )
        });
    }

    let checkpoint = qemu2.output_checkpoint();
    qemu2.send_line("vcat /mnt/sd/probe.txt");
    qemu2.wait_for_after("phase02b-ahci-ok", checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "persistence failed: marker absent after reboot: {e}\n\
                 --- first boot ---\n{}\n--- second boot ---\n{}",
                first_boot,
                qemu2.dump()
            )
        });
}

/// With both an NVMe controller and a SATA disk present, init spawns `/bin/nvme`
/// and `/bin/ahci`, so two cells call `sys_register_block_driver` in one boot.
///
/// Asserts the observed **registration** contract: neither call is refused, and
/// the later TID replaces the earlier one in the kernel role slot and the
/// service registry (`driver_cell.rs:84-91`, registry `insert`) — printed in
/// order so a spawn race is visible.
///
/// Does **not** assert which cell serves filesystem I/O: `service-vfs` resolves
/// the provider through a cached TID, so if VFS looks it up before the second
/// cell registers, the replacement does not redirect traffic. Which cell wins on
/// a two-storage-device machine is therefore scheduling-dependent — recorded as
/// a plan risk, and every other lane attaches exactly one storage device.
#[test]
fn ahci_and_nvme_both_register_x86() {
    if !prerequisites_ok() {
        return;
    }
    let sata = TempImage::new();
    let nvme = TempImage::new();
    let qemu = QemuRunner::boot_x86_bios_with_sata_and_nvme(
        &iso_path(),
        &sata.path().to_string_lossy(),
        &nvme.path().to_string_lossy(),
    );

    qemu.wait_for("[driver_cell] ahci storage driver ready", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "AHCI cell not ready on the combined lane: {e}\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached on the combined lane: {e}\n{}", qemu.dump()));

    let output = qemu.dump();
    let registrations: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("[driver_cell] block driver registered"))
        .collect();
    println!(
        "observed block-driver registrations on the NVMe+SATA lane (in order): {registrations:?}"
    );
    assert!(
        registrations.len() >= 2,
        "expected both NVMe and AHCI to register (last-wins), observed {}: {registrations:?}\n{}",
        registrations.len(),
        output
    );
}
