//! x86_64 PCIe NVMe integration tests — Driver Cell architecture.
//!
//! Since the Kernel Boundary migration the kernel drives no block hardware:
//! the Platform Cell (`/bin/platform`, spawned by the kernel) scans ECAM and
//! registers devices/BARs; init spawns the NVMe Driver Cell (`/bin/nvme`),
//! which locates the controller via `sys_find_pcie_device`, claims BAR0 MMIO,
//! initialises the controller (admin + IO queues over DMA), and binds the block
//! route of its PCI function via `sys_register_block_device`.
//!
//! The oracle is the kernel's registration marker:
//!   `[driver_cell] block driver registered: tid=T device=pci-BB-DD-F route=R`
//! which is only reachable after the FULL chain (ECAM scan → BAR registration
//! → find → MMIO claim → controller RDY → Identify DMA round-trip → IO queue
//! creation) has succeeded.
//!
//! The 08C and 09C witnesses need the fault-injection ISO
//! (`bash scripts/x86/make-ahci-fault-iso.sh`, `VICELL_AHCI_FAULT_ISO`) and
//! skip without it.
//!
//! Tests skip gracefully when the ISO or `qemu-system-x86_64` is absent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::block_09c::fault_iso;
use vicell_integration_tests::block_09c_cycles;
use vicell_integration_tests::block_09c_lanes::{fresh_lease_and_cold_readback, Lane};
use vicell_integration_tests::fat_disk::{python_binary, FatDisk};
use vicell_integration_tests::{qemu_x86_binary, x86_iso_path, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 15;

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
/// register its block route, with its completions MSI-X driven (phase 04b).
///
/// Proves the whole PCIe storage chain under ring-3: Platform Cell ECAM scan,
/// BAR registration, `sys_find_pcie_device`, user-mapped BAR0 MMIO, controller
/// reset/enable, Identify Controller + Namespace DMA round-trips, and IO
/// queue-pair creation.
#[test]
fn nvme_driver_cell_registers_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("[driver_cell] block driver registered: tid=", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            let _ = std::fs::remove_file(&disk);
            panic!(
                "NVMe Driver Cell did not register within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: platform ECAM scan → find_pcie_device(01:08:02) → BAR0 MMIO \
                 claim → controller init → sys_register_block_device.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    // Every admin command after the allocation, Create I/O CQ included, already
    // completed through `WaitCompletion(IRQ)`; q35 never refuses MSI-X.
    let serial = qemu.dump();
    let _ = std::fs::remove_file(&disk);
    assert!(
        serial.contains("[nvme] irq=msix"),
        "NVMe cell did not take MSI-X completions:\n{serial}"
    );
    assert!(
        !serial.contains("[nvme] irq=polled"),
        "NVMe cell fell back to polling on q35:\n{serial}"
    );
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
    if !prerequisites_ok() {
        return;
    }
    let Some(python) = python_binary() else {
        eprintln!(
            "SKIP nvme_fat32_mount_x86: Python 3 not on PATH (needed for mkfat32_inplace.py)"
        );
        return;
    };
    let disk = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");

    // 3. Boot and assert the mount marker.
    let mut qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.arg());
    qemu.wait_for("[vfs] FAT32 /mnt/sd volume mounted", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "FAT32-on-NVMe mount not seen within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: nvme Driver Cell binds its route pre-VFS → VFS blk_router IPC → \
                 DrvRequest sector reads over NVMe DMA → fatfs BPB accept.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });

    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached after NVMe mount: {e}\n{}", qemu.dump()));

    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("vwrite /mnt/sd/probe.txt phase05-nvme-ok");
    qemu.wait_for_after("Cellos >", checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("NVMe write command did not complete: {e}\n{}", qemu.dump()));

    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("vcat /mnt/sd/probe.txt");
    qemu.wait_for_after("phase05-nvme-ok", checkpoint, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("NVMe read-back marker missing: {e}\n{}", qemu.dump()));
}

/// Phase 08C: a dead NVMe driver is restarted by init. The replacement claims
/// the quiesced function again, resets the controller (CC.EN=0 → CSTS.RDY=0),
/// re-registers, and serves the volume: data committed before the death reads
/// back and a new write completes. Needs the fault-injection ISO
/// (`bash scripts/x86/make-ahci-fault-iso.sh`, `VICELL_AHCI_FAULT_ISO`), whose
/// `/bin/nvme` exits on a write carrying `F08B-DIE`.
#[test]
fn nvme_driver_restart_resets_and_serves_again_x86() {
    let Ok(iso) = std::env::var("VICELL_AHCI_FAULT_ISO") else {
        eprintln!(
            "SKIP nvme_driver_restart_resets_and_serves_again_x86: VICELL_AHCI_FAULT_ISO unset"
        );
        return;
    };
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    let disk = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_nic(&iso, &disk.arg());
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
        "[nvme] fault-inject: driver exiting",
        "[pcie] quiesced",
        "Init: service restarted.",
        "[nvme] controller reset complete",
        "[driver_cell] block driver registered: tid=",
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
                || line.contains("controller reset")
                || line.contains("block driver registered")
        })
        .collect();
    println!("08C NVMe witness: {evidence:#?}");
    if let Some(report) =
        vicell_integration_tests::bench_milestones::retire_report("nvme", "tcg", after)
    {
        println!("P08-RETIRE-LATENCY {report}");
    }
}

/// Boot with an NVMe controller attached must still reach the interactive
/// shell — the Driver Cell path must not hang or fault the boot.
#[test]
fn nvme_boot_reaches_shell_x86() {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        let _ = std::fs::remove_file(&disk);
        panic!(
            "shell prompt not reached with NVMe attached: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });

    let _ = std::fs::remove_file(&disk);
}

/// Phase 09C E1+E2+E3 on NVMe (+e1000), 1 vCPU: the verdict-gated
/// replacement rebinds the same route under a new tid after
/// `Init: nvme retirement Contained (…)`, reads boot 1's record cache-cold,
/// and a write that died after the SQ doorbell (`F09C-DIE-ISSUED`) is reported
/// indeterminate and never replayed.
#[test]
fn nvme_09c_fresh_lease_cold_readback_uncertain_write_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::Nvme, &iso, 1, true);
    }
}

/// Phase 09C E5: the NVMe E1–E3 lane on 2 vCPUs.
#[test]
fn nvme_09c_fresh_lease_cold_readback_uncertain_write_smp2_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::Nvme, &iso, 2, true);
    }
}

/// Phase 09C E4(d): rapid NVMe deaths exhaust init's crash budget; after
/// `restart storm` the device's I/O fails with nothing sent.
#[test]
fn nvme_09c_crash_budget_gives_up_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::crash_budget(Lane::Nvme, &iso);
    }
}

/// Phase 09C E6: the e1000 owner (`/bin/e1000` on the fault ISO) keeps its tid
/// and BDF grant across an NVMe replacement; its function is never quiesced.
#[test]
fn nvme_09c_nic_owner_survives_replacement_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::nic_survives_nvme_replacement(&iso);
    }
}

/// Phase 09C E7: E1+E2 for NVMe behind Intel VT-d (real IOTLB invalidation).
#[test]
fn nvme_09c_vtd_fresh_lease_cold_readback_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::NvmeVtd, &iso, 1, false);
    }
}

/// Phase 09C E7 on 2 vCPUs.
#[test]
fn nvme_09c_vtd_fresh_lease_cold_readback_smp2_x86() {
    if let Some(iso) = fault_iso() {
        fresh_lease_and_cold_readback(Lane::NvmeVtd, &iso, 2, false);
    }
}

/// Phase 09C E8: ten NVMe replacements spaced past init's 1000-tick window,
/// each verdict `Contained … dma=0 qpages=0 leaked=0`, each read served.
#[test]
fn nvme_09c_ten_replacement_cycles_x86() {
    if let Some(iso) = fault_iso() {
        block_09c_cycles::repeated_cycles(Lane::Nvme, &iso);
    }
}

/// Phase 12 (CP1 §4.2): a block driver that is alive but never answers must not
/// hang VFS. The fault-injection `/bin/nvme` freezes on a write carrying
/// `F12-FREEZE`: VFS gives that request up within its bound, refuses the
/// stalled device at once afterwards, keeps serving everything else, and keeps
/// its heartbeat, so the kernel never kills it. Before Phase 12 the receive on
/// the driver had no bound and VFS hung with it. Needs `VICELL_AHCI_FAULT_ISO`.
#[test]
fn a_frozen_block_driver_does_not_hang_vfs_x86() {
    let Some(iso) = fault_iso() else {
        return;
    };
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    let disk = FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture");
    let mut qemu = QemuRunner::boot_x86_bios_with_nic(&iso, &disk.arg());
    for marker in ["[vfs] FAT32 /mnt/sd volume mounted", "Cellos >"] {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("boot marker {marker:?}: {e}\n{}", qemu.dump()));
    }
    let run = |qemu: &mut QemuRunner, command: &str, expect: &str, timeout: u64| {
        let checkpoint = qemu.output_checkpoint();
        qemu.send_line(command);
        qemu.wait_for_after(expect, checkpoint, timeout)
            .unwrap_or_else(|e| panic!("{command:?}: {e}\n{}", qemu.dump()));
        qemu.wait_for_after("Cellos >", checkpoint, timeout)
            .unwrap_or_else(|e| panic!("{command:?} prompt: {e}\n{}", qemu.dump()));
    };

    run(
        &mut qemu,
        "vwrite /tmp/alive.txt F12-ALIVE",
        "Cellos >",
        CMD_TIMEOUT,
    );
    let frozen = qemu.output_checkpoint();
    qemu.send_line("vwrite /mnt/sd/freeze.txt F12-FREEZE");
    qemu.wait_for_after("[nvme] fault-inject: driver frozen", frozen, CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("freeze marker: {e}\n{}", qemu.dump()));
    qemu.wait_for_after("Cellos >", frozen, 60)
        .unwrap_or_else(|e| panic!("shell after the frozen write: {e}\n{}", qemu.dump()));
    // VFS answers again once the frozen request ran out its 3 500-tick (35 s)
    // bound; before Phase 12 it waited on the driver forever.
    run(&mut qemu, "vcat /tmp/alive.txt", "F12-ALIVE", 60);
    let stalled = qemu.dump();
    assert!(
        stalled[frozen..].contains("did not reply within 3500 ticks; device stalled"),
        "VFS did not report the stalled device:\n{}",
        &stalled[frozen..]
    );
    // The stalled disk is refused at once; memory-backed paths keep working.
    run(
        &mut qemu,
        "vwrite /mnt/sd/after.txt F12-AFTER",
        "Cellos >",
        CMD_TIMEOUT,
    );
    run(
        &mut qemu,
        "vwrite /tmp/later.txt F12-LATER",
        "Cellos >",
        CMD_TIMEOUT,
    );
    run(&mut qemu, "vcat /tmp/later.txt", "F12-LATER", CMD_TIMEOUT);

    // Past one heartbeat interval (60 s) after the freeze, VFS is still the same
    // live task: no heartbeat kill and no restart.
    std::thread::sleep(std::time::Duration::from_secs(70));
    run(&mut qemu, "vcat /tmp/alive.txt", "F12-ALIVE", CMD_TIMEOUT);
    let output = qemu.dump();
    let after = output.get(frozen..).unwrap_or_default();
    assert!(
        !after.contains("missed liveness deadline") && !after.contains("Init: service restarted"),
        "VFS was killed or restarted after the freeze:\n{after}"
    );
}
