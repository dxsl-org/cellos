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
//! binds the block route of its PCI function (`sys_register_block_device`, the
//! same surface NVMe/virtio-blk use) and serves the DrvRequest IPC with
//! READ/WRITE DMA EXT and FLUSH CACHE EXT. The oracles are:
//!   `[ahci] controller bound`
//!   `[ahci] IDENTIFY DEVICE ok`
//!   `[driver_cell] ahci storage driver ready`
//!   `[driver_cell] block driver registered: tid=T device=pci-00-1f-2 route=R`
//!   `[vfs] FAT32 /mnt/sd volume mounted`
//! and the two-boot persistence test: a marker written through the shell in the
//! first boot must be readable in a second boot on the same image.
//!
//! The 08A–08C and 09C witnesses need the fault-injection ISO
//! (`bash scripts/x86/make-ahci-fault-iso.sh`, `VICELL_AHCI_FAULT_ISO`) and
//! skip without it.
//!
//! Tests skip gracefully when the ISO or `qemu-system-x86_64` is absent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::block_09c::fault_iso;
use vicell_integration_tests::block_09c_lanes::{fresh_lease_and_cold_readback, Lane};
use vicell_integration_tests::fat_disk::{python_binary, FatDisk};
use vicell_integration_tests::{block_09c_cycles, block_09c_two_disks, qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 15;

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
    // No lane-ISO preference: `build/x86-pc-lane/vicell-x86-*.iso` is a
    // build-time artifact that goes stale against the cell set (a part-A-only
    // lane ISO silently lacked the part-B cell). The production ISO is what CI
    // assembles; pin a lane ISO explicitly through `VICELL_AHCI_ISO`.
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
            "SKIP ahci-x86: ISO does not carry /bin/ahci ({})\n  Rebuild: pwsh scripts/build-x86_64-cells.ps1, then the x86_64 kernel + ISO",
            iso_path()
        );
    }
    if !iso_ok {
        eprintln!(
            "SKIP ahci-x86: x86_64 ISO not built ({})\n  Run: pwsh scripts/build-x86_64-cells.ps1, then build the x86_64 kernel + ISO",
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

/// The AHCI Driver Cell must bind the block route of its PCI function through
/// the same surface NVMe uses: `[driver_cell] block driver registered`. Its
/// IDENTIFY already completed on the ICH9 MSI (phase 04b).
#[test]
fn ahci_driver_cell_registers_block_driver_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = TempImage::new();
    let qemu = QemuRunner::boot_x86_bios_with_sata(&iso_path(), &disk.path().to_string_lossy());

    qemu.wait_for("[driver_cell] block driver registered: tid=", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "AHCI Driver Cell did not register as block driver within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: IDENTIFY ok → DmaBuf::authorize(sector buffer) → \
                 sys_register_block_device(Pci 00:1f.2) → kernel block route.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    let serial = qemu.dump();
    assert!(
        serial.contains("[ahci] irq=msi"),
        "AHCI cell did not take MSI completions:\n{serial}"
    );
    assert!(
        !serial.contains("[ahci] irq=polled"),
        "AHCI cell fell back to polling on q35:\n{serial}"
    );
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
    let image = FatDisk::new(python).unwrap_or_else(|| {
        panic!(
            "mkfat32_inplace.py failed: the persistence oracle cannot run, which is a test \
             failure (the in-tree formatter is the test's own fixture), not an environment skip"
        )
    });
    let disk_path = image.arg();

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
    qemu.send_line("/bin/shutdown");
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
/// and `/bin/ahci`; each binds the route of the PCI function it owns, so the two
/// registrations name distinct devices and route indices. VFS mounts the
/// primary (the lowest device key: these fixtures carry no Cellos MBR) at
/// `/mnt/sd` and the other disk at `/mnt/disk/<name>`. Both are usable: one
/// file is written and read back on each mount.
#[test]
fn ahci_and_nvme_both_mounted_x86() {
    if !prerequisites_ok() {
        return;
    }
    let Some(python) = python_binary() else {
        let _ = vicell_integration_tests::ci_guard(false);
        eprintln!("SKIP ahci_and_nvme_both_mounted_x86: Python 3 not on PATH");
        return;
    };
    let sata = FatDisk::new(python).expect("mkfat32_inplace.py formats the SATA fixture");
    let nvme = FatDisk::new(python).expect("mkfat32_inplace.py formats the NVMe fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_sata_and_nvme(&iso_path(), &sata.arg(), &nvme.arg());
    let disks = block_09c_two_disks::discover(&qemu);
    block_09c_two_disks::print_discovery("combined lane", &qemu);

    for path in ["/mnt/sd/both.txt".to_string(), format!("{}/both.txt", disks.secondary_mount())] {
        let checkpoint = qemu.output_checkpoint();
        qemu.send_line(&format!("vwrite {path} both-mounted-ok"));
        qemu.wait_for_after("Cellos >", checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("vwrite {path}: {e}\n{}", qemu.dump()));
        let checkpoint = qemu.output_checkpoint();
        qemu.send_line(&format!("vcat {path}"));
        qemu.wait_for_after("both-mounted-ok", checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("vcat {path}: {e}\n{}", qemu.dump()));
    }
}

/// Phase 08A witness: the block outcome carries issue certainty from the AHCI
/// driver to the VFS write path, and only a proven `NotIssued` write is retried.
///
/// Needs the private fault-injection ISO (`bash scripts/x86/make-ahci-fault-iso.sh`)
/// named by `VICELL_AHCI_FAULT_ISO`; its `/bin/ahci` faults one write per payload
/// marker (`cells/drivers/ahci/src/fault_inject.rs`). Skips when the variable is
/// unset: the production ISO has no injection seam to drive.
///
/// One boot, three files on the SATA FAT32 volume:
/// - `F08A-OK`: completed control, readable back.
/// - `F08A-NI`: refused before `PxCI` once, then retried by VFS and completed;
///   readable back.
/// - `F08A-IND`: issued, completion suppressed. The caller sees the
///   `Indeterminate` outcome and nothing re-issues that payload.
#[test]
fn ahci_write_outcome_issue_certainty_x86() {
    let Ok(iso) = std::env::var("VICELL_AHCI_FAULT_ISO") else {
        eprintln!(
            "SKIP ahci_write_outcome_issue_certainty_x86: VICELL_AHCI_FAULT_ISO unset \
             (build: bash scripts/x86/make-ahci-fault-iso.sh)"
        );
        return;
    };
    assert!(PathBuf::from(&iso).exists(), "VICELL_AHCI_FAULT_ISO={iso} does not exist");
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    let image = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_sata(&iso, &image.arg());
    for marker in [
        "[driver_cell] block driver registered",
        "[vfs] FAT32 /mnt/sd volume mounted",
        "Cellos >",
    ] {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("boot marker {marker:?}: {e}\n{}", qemu.dump()));
    }

    // Run one shell command; return the serial output it produced.
    let run = |qemu: &mut QemuRunner, command: &str, expect: &str| -> String {
        let checkpoint = qemu.output_checkpoint();
        qemu.send_line(command);
        qemu.wait_for_after(expect, checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("{command:?}: {e}\n{}", qemu.dump()));
        qemu.wait_for_after("Cellos >", checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("{command:?} prompt: {e}\n{}", qemu.dump()));
        qemu.dump().get(checkpoint..).unwrap_or_default().to_string()
    };

    run(&mut qemu, "vwrite /mnt/sd/ok.txt F08A-OK-control", "Cellos >");
    run(&mut qemu, "vcat /mnt/sd/ok.txt", "F08A-OK-control");

    let not_issued = run(&mut qemu, "vwrite /mnt/sd/ni.txt F08A-NI-retried", "outcome=Completed");
    assert!(
        not_issued.contains("[ahci] fault-inject: WRITE DMA EXT refused before issue")
            && not_issued.contains("outcome=NotIssued"),
        "NotIssued control did not refuse before issue then complete on retry:\n{not_issued}"
    );
    run(&mut qemu, "vcat /mnt/sd/ni.txt", "F08A-NI-retried");

    let indeterminate = run(&mut qemu, "vwrite /mnt/sd/ind.txt F08A-IND-once", "not replayed");
    assert!(
        indeterminate
            .contains("[ahci] fault-inject: WRITE DMA EXT completion suppressed after PxCI")
            && indeterminate.contains("outcome=Indeterminate"),
        "the marked write was not issued and reported Indeterminate:\n{indeterminate}"
    );
    assert_eq!(
        indeterminate.matches("outcome=Indeterminate").count(),
        1,
        "the Indeterminate write reached the driver more than once:\n{indeterminate}"
    );
    // A later write must not drag the uncertain payload back to the device either.
    run(&mut qemu, "vwrite /mnt/sd/after.txt F08A-OK-after", "Cellos >");
    let output = qemu.dump();
    assert!(
        !output.contains("REPLAY of indeterminate write"),
        "an Indeterminate write was re-issued:\n{output}"
    );
    let evidence: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("fault-inject") || line.contains("[blk-router]"))
        .collect();
    println!("08A witness evidence: {evidence:#?}");
}

/// Phase 08B: when the AHCI Driver Cell retires, the kernel quiesces its PCI
/// function (Bus Master off, INTx disabled) before the BDF is released. The
/// ICH9 AHCI function on q35 is 00:1f.2.
///
/// The suite shell cannot kill a service, so the fault-injection ISO (see
/// [`ahci_write_outcome_issue_certainty_x86`]) supplies the death: a write
/// carrying `F08B-DIE` makes `/bin/ahci` exit with its HBA still bus-mastering.
#[test]
fn ahci_driver_retirement_quiesces_function_x86() {
    let Ok(iso) = std::env::var("VICELL_AHCI_FAULT_ISO") else {
        eprintln!(
            "SKIP ahci_driver_retirement_quiesces_function_x86: VICELL_AHCI_FAULT_ISO unset \
             (build: bash scripts/x86/make-ahci-fault-iso.sh)"
        );
        return;
    };
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    let image = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_sata(&iso, &image.arg());
    let registered = "[driver_cell] block driver registered: tid=";
    for marker in [registered, "[vfs] FAT32 /mnt/sd volume mounted", "Cellos >"] {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("boot marker {marker:?}: {e}\n{}", qemu.dump()));
    }
    let output = qemu.dump();
    let tid: usize = output
        .lines()
        .find_map(|line| line.split(registered).nth(1))
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or_else(|| panic!("AHCI driver tid not logged\n{output}"));

    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("vwrite /mnt/sd/die.txt F08B-DIE");
    let quiesced = format!("[pcie] quiesced 00:1f.2 for tid {tid}: ");
    qemu.wait_for_after(&quiesced, checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("AHCI function not quiesced on retirement: {e}\n{}", qemu.dump()));
    let output = qemu.dump();
    let line = output
        .lines()
        .find(|line| line.contains(&quiesced))
        .expect("quiesce line present");
    assert!(
        line.contains("bme=0 intx_disable=1"),
        "quiesce did not report Bus Master off and INTx disabled: {line}"
    );
    assert!(
        !output.contains("not quiescent"),
        "quiescence failed and the BDF was retained:\n{output}"
    );
    println!("08B witness: {line}");
}

/// Phase 08C: a dead AHCI driver is restarted by init, the replacement claims
/// the quiesced function again, resets the HBA, re-registers, and serves the
/// volume: the file committed before the death reads back and a new write
/// completes through the new incarnation.
#[test]
fn ahci_driver_restart_resets_and_serves_again_x86() {
    let Ok(iso) = std::env::var("VICELL_AHCI_FAULT_ISO") else {
        eprintln!("SKIP ahci_driver_restart_resets_and_serves_again_x86: VICELL_AHCI_FAULT_ISO unset");
        return;
    };
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    let image = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_sata(&iso, &image.arg());
    for marker in ["[vfs] FAT32 /mnt/sd volume mounted", "Cellos >"] {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("boot marker {marker:?}: {e}\n{}", qemu.dump()));
    }
    let run = |qemu: &mut QemuRunner, command: &str, expect: &str| {
        let checkpoint = qemu.output_checkpoint();
        qemu.send_line(command);
        qemu.wait_for_after(expect, checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("{command:?}: {e}\n{}", qemu.dump()));
        qemu.wait_for_after("Cellos >", checkpoint, CMD_TIMEOUT)
            .unwrap_or_else(|e| panic!("{command:?} prompt: {e}\n{}", qemu.dump()));
    };
    run(&mut qemu, "vwrite /mnt/sd/keep.txt F08C-KEEP", "Cellos >");

    let death = qemu.output_checkpoint();
    qemu.send_line("vwrite /mnt/sd/die.txt F08B-DIE");
    for marker in [
        "[ahci] fault-inject: driver exiting",
        "[pcie] quiesced 00:1f.2",
        "Init: service restarted.",
        "[ahci] HBA reset complete",
        "[driver_cell] ahci storage driver ready",
    ] {
        qemu.wait_for_after(marker, death, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("restart marker {marker:?}: {e}\n{}", qemu.dump()));
    }
    qemu.wait_for_after("Cellos >", death, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell after the driver death: {e}\n{}", qemu.dump()));

    run(&mut qemu, "vcat /mnt/sd/keep.txt", "F08C-KEEP");
    run(&mut qemu, "vwrite /mnt/sd/after.txt F08C-AFTER", "Cellos >");
    run(&mut qemu, "vcat /mnt/sd/after.txt", "F08C-AFTER");
    let output = qemu.dump();
    let after = output.get(death..).unwrap_or_default();
    assert!(
        !after.contains("not quiescent") && !after.contains("restart storm"),
        "the function was retained or init gave up:\n{after}"
    );
    let evidence: Vec<&str> = after
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.contains("[pcie] quiesced")
                || line.contains("Init: service")
                || line.contains("HBA reset")
                || line.contains("block driver registered")
                || line.contains("granted DMA BDF=00:1f.2")
        })
        .collect();
    println!("08C AHCI witness: {evidence:#?}");
    if let Some(report) = vicell_integration_tests::bench_milestones::retire_report("ahci", "tcg", after) {
        println!("P08-RETIRE-LATENCY {report}");
    }
}

/// Phase 09C E1+E2+E3 on AHCI, 1 vCPU. E1: an `F08B-DIE` death is quiesced,
/// init logs `Init: ahci retirement Contained (…)` before the replacement's
/// DMA grant, and the replacement resets the HBA and rebinds the same route
/// under a new tid. E2: a record committed in boot 1 and never read in boot 2
/// is read cache-cold through the new lease, and boot 3 reads both records from
/// the medium. E3: `F09C-DIE-ISSUED` dies after `PxCI`; VFS reports the write
/// indeterminate and never replays it; the post-restart `vcat` equals boot 3's.
#[test]
fn ahci_09c_fresh_lease_cold_readback_uncertain_write_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::Ahci, &iso, 1, true);
    }
}

/// Phase 09C E5: the AHCI E1–E3 lane on 2 vCPUs (`[x86-smp] topology online=2`).
#[test]
fn ahci_09c_fresh_lease_cold_readback_uncertain_write_smp2_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::Ahci, &iso, 2, true);
    }
}

/// Phase 09C E4(d): rapid AHCI deaths exhaust init's crash budget; after
/// `restart storm` the device's I/O fails with nothing sent.
#[test]
fn ahci_09c_crash_budget_gives_up_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::crash_budget(Lane::Ahci, &iso);
    }
}

/// Phase 09C E4(b): an AHCI death holding an untracked SHM export retires
/// `Unproven`; init logs `node recovery required` and does not restart it.
#[test]
fn ahci_09c_untracked_export_blocks_restart_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::untracked_export_blocks_restart(&iso);
    }
}

/// Phase 09C E8: ten AHCI replacements spaced past init's 1000-tick window,
/// each verdict `Contained … dma=0 qpages=0 leaked=0`, each read served.
#[test]
fn ahci_09c_ten_replacement_cycles_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::repeated_cycles(Lane::Ahci, &iso);
    }
}

/// Phase 09C E9 on SATA+NVMe, 1 vCPU: both disks mounted on their own routes;
/// killing either driver leaves the other disk serving, refuses the dead
/// disk's I/O unsent, and the replacement rebinds the same route; a final boot
/// and the raw media show each disk holds only its own files.
#[test]
fn two_disks_09c_independent_failure_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_two_disks::two_disks_independent(&iso, 1);
    }
}

/// Phase 09C E9 on 2 vCPUs.
#[test]
fn two_disks_09c_independent_failure_smp2_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_two_disks::two_disks_independent(&iso, 2);
    }
}
