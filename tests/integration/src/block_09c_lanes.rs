//! Single-disk 09C lanes on the fault ISO: E1 fresh lease, E2 cache-cold
//! readback across three boots of one image, E3 uncertain write not replayed.
//! Parameterised by controller, vCPU count (E5) and IOMMU lane (E7).

use crate::block_09c::{begin_death, find_after, observe, read_ok, shutdown, wait_after, wait_single_disk_boot, write_ok};
use crate::block_09c::CMD_TIMEOUT;
use crate::block_09c_replace::{finish_replacement, print_evidence, wait_provider};
use crate::fat_disk::{python_binary, FatDisk};
use crate::QemuRunner;

/// A single-storage-device q35 lane.
#[derive(Clone, Copy, Debug)]
pub enum Lane {
    /// ICH9 AHCI at 00:1f.2 (`boot_x86_bios_with_sata[_smp]`).
    Ahci,
    /// NVMe plus e1000 (`boot_x86_bios_with_nic[_smp]`).
    Nvme,
    /// NVMe plus e1000 behind Intel VT-d (`boot_x86_bios_with_vtd[_smp]`).
    NvmeVtd,
}

impl Lane {
    pub fn boot(self, iso: &str, disk: &FatDisk, cpus: usize) -> QemuRunner {
        let disk = disk.arg();
        match self {
            Lane::Ahci => QemuRunner::boot_x86_bios_with_sata_smp(iso, &disk, cpus),
            Lane::Nvme => QemuRunner::boot_x86_bios_with_nic_smp(iso, &disk, cpus),
            Lane::NvmeVtd => QemuRunner::boot_x86_bios_with_vtd_smp(iso, &disk, cpus),
        }
    }
}

/// A fresh FAT32 disk; the formatter is the test's own fixture, so a failure
/// is a test failure, not a skip.
pub fn fixture() -> FatDisk {
    let python = python_binary().expect("Python 3 formats the FAT32 fixture");
    FatDisk::new(python).expect("mkfat32_inplace.py formats the fixture")
}

const R0: &str = "F09C-E2-R0-COMMITTED";
const R1: &str = "F09C-E2-R1-NEW-LEASE";
const ISSUED: &str = "F09C-DIE-ISSUED";
/// Prefix of both post-issue exit lines (`after PxCI` / `after doorbell`).
const ISSUED_EXIT: &str = "fault-inject: driver exiting after ";

/// E1 + E2 over three boots of one image, plus E3 when `uncertain_write`.
pub fn fresh_lease_and_cold_readback(lane: Lane, iso: &str, cpus: usize, uncertain_write: bool) {
    let row = format!("[{lane:?} smp={cpus}]");
    let disk = fixture();

    // Boot 1: commit R0 and stop.
    let mut qemu = lane.boot(iso, &disk, cpus);
    wait_single_disk_boot(&qemu, cpus);
    write_ok(&mut qemu, "/mnt/sd/r0.txt", R0);
    let boot1 = shutdown(qemu);
    print_evidence(&format!("E5 {row}"), boot1.lines().filter(|l| l.contains("[x86-smp] topology")));

    // Boot 2: R0 is never read before the death, so its data sectors are not in
    // the fresh VFS page cache and the read below is served by the new lease.
    let mut qemu = lane.boot(iso, &disk, cpus);
    let t0 = wait_single_disk_boot(&qemu, cpus);
    let death = begin_death(&mut qemu, "vwrite /mnt/sd/die.txt F08B-DIE");
    let e1 = finish_replacement(&qemu, &death, &t0, &["Contained"]);
    assert_ne!(e1.new.tid, t0.tid, "the replacement reused the dead tid");
    let read_at = qemu.output_checkpoint();
    read_ok(&mut qemu, "/mnt/sd/r0.txt", R0);
    let (cell1, gen1) = wait_provider(&qemu, read_at, &t0.device, e1.new.tid);
    let output = qemu.dump();
    let old_provider = format!("[vfs] block device {} provider tid={} ", t0.device, t0.tid);
    if let Some(at) = find_after(&output, 0, &old_provider) {
        let line = crate::block_09c::line_at(&output, at);
        let old = (crate::block_09c::field(line, "cell="), crate::block_09c::field(line, "gen="));
        assert_ne!(old, (Some(cell1.as_str()), Some(gen1.as_str())), "the replacement kept the dead (cell, gen)");
    }
    print_evidence(&format!("E1 {row}"), &e1.evidence);
    println!(
        "09C E1 {row} T0={} T1={} device={} route={} cell={cell1} gen={gen1} verdict={:?}",
        t0.tid, e1.new.tid, t0.device, e1.new.route, e1.verdict
    );
    write_ok(&mut qemu, "/mnt/sd/r1.txt", R1);
    read_ok(&mut qemu, "/mnt/sd/r1.txt", R1);

    let observed_after_restart = uncertain_write.then(|| {
        let death = begin_death(&mut qemu, &format!("vwrite /mnt/sd/d.txt {ISSUED}"));
        assert!(death.output.contains("vwrite: failed to write"), "the uncertain write did not fail:\n{}", death.output);
        wait_after(&qemu, "driver died before replying; outcome indeterminate", death.at, CMD_TIMEOUT);
        wait_after(&qemu, "indeterminate; not replayed", death.at, CMD_TIMEOUT);
        let e3 = finish_replacement(&qemu, &death, &e1.new, &["Contained", "Quarantined"]);
        let observed = observe(&mut qemu, "/mnt/sd/d.txt");
        // A later write must not drag the uncertain payload back to the device.
        write_ok(&mut qemu, "/mnt/sd/after-d.txt", "F09C-E3-AFTER");
        let output = qemu.dump();
        assert_eq!(
            output.matches(ISSUED_EXIT).count(),
            1,
            "the issued-then-died write reached a driver more than once (VFS replay):\n{output}"
        );
        assert!(!output.contains("REPLAY of indeterminate write"), "an indeterminate write was replayed:\n{output}");
        print_evidence(&format!("E3 {row}"), &e3.evidence);
        println!("09C E3 {row} verdict={:?} vcat d.txt after restart: {observed:?}", e3.verdict);
        observed
    });
    shutdown(qemu);

    // Boot 3: fresh QEMU on the same image reads the medium.
    let mut qemu = lane.boot(iso, &disk, cpus);
    wait_single_disk_boot(&qemu, cpus);
    read_ok(&mut qemu, "/mnt/sd/r0.txt", R0);
    read_ok(&mut qemu, "/mnt/sd/r1.txt", R1);
    println!("09C E2 {row} boot 3 read R0={R0} and R1={R1} from the medium");
    if let Some(observed) = observed_after_restart {
        let later = observe(&mut qemu, "/mnt/sd/d.txt");
        println!("09C E3 {row} vcat d.txt in boot 3: {later:?}");
        assert_eq!(observed, later, "boot 3 read a different value than the post-restart read (cached image served)");
    }
    shutdown(qemu);
}
