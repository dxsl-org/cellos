//! E9: two storage devices on two routes, both mounted; a driver death affects
//! only its own device (Shape 2, `phase-09c-cp1.md` §4.4.8). Lane:
//! `boot_x86_bios_with_sata_and_nvme[_smp]` with two FAT32 fixtures, neither
//! carrying a Cellos MBR, so the primary is the lowest device key.

use crate::block_09c::{begin_death, field, find_after, line_at, read_fails, read_ok, registrations, run, shutdown};
use crate::block_09c::{wait_until, write_ok, Registration, BOOT_TIMEOUT, PROMPT, REGISTERED};
use crate::block_09c_lanes::fixture;
use crate::block_09c_replace::{finish_replacement, print_evidence, wait_provider};
use crate::QemuRunner;

/// The q35 ICH9 AHCI function is fixed at 00:1f.2.
const AHCI_DEVICE: &str = "pci-00-1f-2";
const P_ONLY: &str = "F09C-E9-PRIMARY-ONLY";
const S_ONLY: &str = "F09C-E9-SECONDARY-ONLY";
const P_EARLY: &str = "F09C-E9-PRIMARY-EARLY";
const S_EARLY: &str = "F09C-E9-SECONDARY-EARLY";

/// The two routes as VFS classified them.
pub struct Disks {
    pub primary: Registration,
    pub secondary: Registration,
}

impl Disks {
    pub fn secondary_mount(&self) -> String {
        format!("/mnt/disk/{}", self.secondary.device)
    }
}

/// `pci-BB-DD-F` → its 16-bit BDF key.
fn bdf_key(device: &str) -> u16 {
    let parts: Vec<u16> = device
        .trim_start_matches("pci-")
        .split('-')
        .map(|part| u16::from_str_radix(part, 16).expect("hex PCI name part"))
        .collect();
    (parts[0] << 8) | (parts[1] << 3) | parts[2]
}

/// E9(a): two registrations on distinct routes, VFS logs both routes and the
/// primary `(lowest key)`, and mounts `/mnt/sd` and `/mnt/disk/<secondary>`.
pub fn discover(qemu: &QemuRunner) -> Disks {
    let regs = wait_until(qemu, BOOT_TIMEOUT, "two block-route registrations", |output| {
        let regs = registrations(output, 0);
        (regs.len() >= 2).then_some(regs)
    });
    assert_eq!(regs.len(), 2, "exactly one route per disk: {regs:?}");
    assert!(regs[0].device != regs[1].device && regs[0].route != regs[1].route, "routes not distinct: {regs:?}");
    let primary_name = wait_until(qemu, BOOT_TIMEOUT, "`[vfs] primary disk`", |output| {
        let line = line_at(output, output.find("[vfs] primary disk ")?);
        assert!(line.contains("(lowest key)"), "fixtures carry no Cellos MBR, so the primary is the lowest key: {line}");
        Some(field(line, "primary disk ")?.to_string())
    });
    let (primary, secondary) = if regs[0].device == primary_name {
        (regs[0].clone(), regs[1].clone())
    } else {
        (regs[1].clone(), regs[0].clone())
    };
    assert_eq!(primary.device, primary_name, "the primary is not a registered route: {regs:?}");
    assert!(bdf_key(&primary.device) < bdf_key(&secondary.device), "primary is not the lowest key");
    let disks = Disks { primary, secondary };
    for route in [&disks.primary, &disks.secondary] {
        let needle = format!("[vfs] block route {} {} tid=", route.route, route.device);
        wait_until(qemu, BOOT_TIMEOUT, &needle, |output| {
            let tid = field(line_at(output, output.find(&needle)?), "tid=")?;
            assert_eq!(tid, route.tid.to_string(), "VFS discovered {} with another provider", route.device);
            Some(())
        });
    }
    for marker in [
        "[vfs] FAT32 /mnt/sd volume mounted".to_string(),
        format!("[vfs] FAT32 {} volume mounted", disks.secondary_mount()),
        PROMPT.to_string(),
    ] {
        qemu.wait_for(&marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("E9 discovery marker {marker:?}: {e}\n{}", qemu.dump()));
    }
    disks
}

/// Print the discovery lines of one boot.
pub fn print_discovery(row: &str, qemu: &QemuRunner) {
    let output = qemu.dump();
    print_evidence(
        row,
        output.lines().map(str::trim).filter(|l| {
            l.contains(REGISTERED) || l.contains("[vfs] block route") || l.contains("[vfs] primary disk") || l.contains("[vfs] FAT32")
        }),
    );
}

/// Kill `victim`'s driver; `survivor` keeps serving during and after the gap,
/// a victim write inside the gap is refused unsent, and the replacement serves
/// `early` (committed in boot 1, unread this boot) cache-cold. Returns the
/// victim's new registration.
fn kill_one(qemu: &mut QemuRunner, row: &str, victim: (&Registration, &str), survivor: (&Registration, &str), early: (&str, &str)) -> Registration {
    let ((victim, victim_mount), (survivor, survivor_mount)) = (victim, survivor);
    let death = begin_death(qemu, &format!("vwrite {victim_mount}/die.txt F08B-DIE"));
    let gap_file = format!("{survivor_mount}/gap-{}.txt", victim.route);
    write_ok(qemu, &gap_file, "F09C-E9-SURVIVOR-GAP");
    read_ok(qemu, &gap_file, "F09C-E9-SURVIVOR-GAP");
    let gap_cmd = qemu.output_checkpoint();
    let out = run(qemu, &format!("vwrite {victim_mount}/gap.txt F09C-E9-VICTIM-GAP"));
    let output = qemu.dump();
    let sticky = format!("[blk-router] {}: no live provider; not issued", victim.device);
    let reregistered = registrations(&output, death.at).iter().any(|r| r.device == victim.device);
    if reregistered {
        // The replacement won the race with this write; the dead device's own follow-up
        // writes (FAT metadata after the failed data write) were refused unsent.
        let sticky_at = find_after(&output, death.at, &sticky)
            .unwrap_or_else(|| panic!("no {sticky:?} while {} had no provider:\n{}", victim.device, &output[death.at..]));
        println!("09C E9 {row} victim gap write raced the replacement; sticky refusal seen at the death: {}", line_at(&output, sticky_at));
    } else {
        assert!(out.contains("vwrite: failed to write"), "a write to the dead device succeeded:\n{out}");
        assert!(find_after(&output, gap_cmd, &sticky).is_some(), "no {sticky:?} for the gap write:\n{out}");
    }

    let replacement = finish_replacement(qemu, &death, victim, &["Contained"]);
    read_ok(qemu, &format!("{victim_mount}/{}", early.0), early.1);
    // The first request after the restart (possibly the gap write) resolves the provider.
    let (cell, generation) = wait_provider(qemu, death.at, &victim.device, replacement.new.tid);
    write_ok(qemu, &format!("{victim_mount}/after.txt"), "F09C-E9-VICTIM-AFTER");
    read_ok(qemu, &format!("{victim_mount}/after.txt"), "F09C-E9-VICTIM-AFTER");
    read_ok(qemu, &gap_file, "F09C-E9-SURVIVOR-GAP");

    let output = qemu.dump();
    let survivor_provider = format!("[vfs] block device {} provider", survivor.device);
    assert!(find_after(&output, death.at, &survivor_provider).is_none(), "the survivor's provider changed:\n{output}");
    print_evidence(row, &replacement.evidence);
    println!(
        "09C E9 {row} {} route={} T{}->T{} cell={cell} gen={generation}; survivor {} tid={} unchanged",
        victim.device, victim.route, victim.tid, replacement.new.tid, survivor.device, survivor.tid
    );
    replacement.new
}

/// The whole E9 row: discovery and independent I/O (boot 1), secondary then
/// primary killed (boot 2), and per-disk isolation (boot 3 and the raw media).
pub fn two_disks_independent(iso: &str, cpus: usize) {
    let (sata, nvme) = (fixture(), fixture());
    let boot = || {
        let qemu = QemuRunner::boot_x86_bios_with_sata_and_nvme_smp(iso, &sata.arg(), &nvme.arg(), cpus);
        if cpus > 1 {
            qemu.wait_for(&format!("[x86-smp] topology online={cpus}"), BOOT_TIMEOUT)
                .unwrap_or_else(|e| panic!("E9 smp={cpus} lane unqualified: {e}\n{}", qemu.dump()));
        }
        qemu
    };

    let mut qemu = boot();
    let disks = discover(&qemu);
    let secondary = disks.secondary_mount();
    print_discovery(&format!("E9a [smp={cpus}]"), &qemu);
    for (path, text) in [
        ("/mnt/sd/p-only.txt".to_string(), P_ONLY),
        ("/mnt/sd/p-early.txt".to_string(), P_EARLY),
        (format!("{secondary}/s-only.txt"), S_ONLY),
        (format!("{secondary}/s-early.txt"), S_EARLY),
    ] {
        write_ok(&mut qemu, &path, text);
        read_ok(&mut qemu, &path, text);
    }
    shutdown(qemu);

    let mut qemu = boot();
    let disks2 = discover(&qemu);
    assert_eq!(disks2.primary.device, disks.primary.device, "the primary changed between boots");
    let secondary_new = kill_one(
        &mut qemu,
        &format!("E9c [smp={cpus}]"),
        (&disks2.secondary, &secondary),
        (&disks2.primary, "/mnt/sd"),
        ("s-early.txt", S_EARLY),
    );
    kill_one(
        &mut qemu,
        &format!("E9d [smp={cpus}]"),
        (&disks2.primary, "/mnt/sd"),
        (&secondary_new, &secondary),
        ("p-early.txt", P_EARLY),
    );
    shutdown(qemu);

    let mut qemu = boot();
    discover(&qemu);
    read_ok(&mut qemu, "/mnt/sd/p-only.txt", P_ONLY);
    read_ok(&mut qemu, &format!("{secondary}/s-only.txt"), S_ONLY);
    read_fails(&mut qemu, "/mnt/sd/s-only.txt");
    read_fails(&mut qemu, &format!("{secondary}/p-only.txt"));
    shutdown(qemu);

    let (primary_medium, secondary_medium) = if disks.primary.device == AHCI_DEVICE { (&sata, &nvme) } else { (&nvme, &sata) };
    assert!(primary_medium.medium_contains(P_ONLY.as_bytes()) && !primary_medium.medium_contains(S_ONLY.as_bytes()));
    assert!(secondary_medium.medium_contains(S_ONLY.as_bytes()) && !secondary_medium.medium_contains(P_ONLY.as_bytes()));
    println!("09C E9e [smp={cpus}] boot 3 and the raw media hold each disk's files only on that disk");
}
