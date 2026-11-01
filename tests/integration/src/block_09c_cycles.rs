//! 09C single-disk lanes beyond one replacement: E4(d) crash budget, E6 a
//! sibling PCI owner survives, E8 repeated cycles without leaks.

use crate::block_09c::{begin_death, field, find_after, line_at, observe, pci_bdf, read_ok, registrations, shutdown};
use crate::block_09c::{wait_single_disk_boot, wait_until, write_fails, write_ok, BOOT_TIMEOUT, RESTART_TIMEOUT};
use crate::block_09c_lanes::{fixture, Lane};
use crate::block_09c_replace::{finish_replacement, print_evidence};
use std::time::Duration;

/// More deaths than init's budget (5 restarts per 1000-tick window) allows.
const MAX_STORM_DEATHS: usize = 12;
/// Init's restart window (1000 scheduler ticks of 10 ms) plus margin: deaths
/// spaced by this never share a window, so the budget never trips.
const INIT_WINDOW_SPACING: Duration = Duration::from_secs(12);
const KEEP: &str = "F09C-E8-KEEP";
const NIC_REGISTERED: &str = "[driver_cell] NIC driver registered: tid=";

/// E4(d): rapid `F08B-DIE` deaths until init prints `restart storm`; the
/// device then has no provider and its I/O fails without reaching a driver.
pub fn crash_budget(lane: Lane, iso: &str) {
    let disk = fixture();
    let mut qemu = lane.boot(iso, &disk, 1);
    let mut current = wait_single_disk_boot(&qemu, 1);
    let start = qemu.output_checkpoint();
    let mut storm_at = None;
    for death_no in 0..MAX_STORM_DEATHS {
        let death = begin_death(&mut qemu, &format!("vwrite /mnt/sd/storm{death_no}.txt F08B-DIE"));
        let next = wait_until(&qemu, RESTART_TIMEOUT, "a replacement or `restart storm`", |output| {
            if let Some(at) = find_after(output, death.at, "restart storm") {
                return Some(Err(at));
            }
            registrations(output, death.at)
                .into_iter()
                .find(|r| r.device == current.device && r.tid != current.tid)
                .map(Ok)
        });
        match next {
            Ok(replacement) => current = replacement,
            Err(at) => {
                storm_at = Some(at);
                break;
            }
        }
    }
    let output = qemu.dump();
    print_evidence(
        &format!("E4d [{lane:?}]"),
        output[start..].lines().map(str::trim).filter(|l| {
            l.contains("Init: storm state") || l.contains("restart storm") || l.contains(" retirement ")
        }),
    );
    let Some(storm_at) = storm_at else {
        crate::skip_notice("E4d unqualified: restarts too slow to form a storm inside init's window");
        println!("09C E4d [{lane:?}] UNQUALIFIED: {MAX_STORM_DEATHS} deaths never exhausted the budget");
        return;
    };

    let write = write_fails(&mut qemu, "/mnt/sd/after-storm.txt", "F09C-E4D-AFTER");
    let seen = observe(&mut qemu, "/mnt/sd/after-storm.txt");
    assert!(
        seen.as_deref().is_none_or(|text| !text.contains("F09C-E4D-AFTER")),
        "I/O succeeded after init gave up: {seen:?}"
    );
    let output = qemu.dump();
    assert!(registrations(&output, storm_at).is_empty(), "a driver registered after `restart storm`:\n{output}");
    let sticky = format!("[blk-router] {}: no live provider; not issued", current.device);
    assert!(find_after(&output, storm_at, &sticky).is_some(), "no sticky refusal {sticky:?}:\n{output}");
    print_evidence(&format!("E4d [{lane:?}]"), write.lines().map(str::trim).filter(|l| l.contains("vwrite") || l.contains("[blk-router]")));
    println!("09C E4d [{lane:?}] after storm: write failed, vcat {seen:?}");
}

/// E4(b): an AHCI driver that dies holding an SHM mapping (no borrower record)
/// retires `Unproven`; init refuses the restart and the device stays unavailable.
pub fn untracked_export_blocks_restart(iso: &str) {
    let disk = fixture();
    let mut qemu = Lane::Ahci.boot(iso, &disk, 1);
    let old = wait_single_disk_boot(&qemu, 1);
    let death = begin_death(&mut qemu, "vwrite /mnt/sd/shm.txt F09C-DIE-SHM");
    let refusal = "Init: ahci node recovery required: containment unproven, no restart";
    let refused_at = wait_until(&qemu, RESTART_TIMEOUT, refusal, |output| find_after(output, death.at, refusal));
    let write = write_fails(&mut qemu, "/mnt/sd/after-shm.txt", "F09C-E4B-AFTER");
    let output = qemu.dump();
    assert!(find_after(&output, death.at, "driver exiting holding SHM").is_some(), "the SHM seam did not fire:\n{output}");
    assert!(find_after(&output, death.at, "Init: service died").is_none(), "init restarted an Unproven retirement:\n{output}");
    assert!(registrations(&output, death.at).is_empty(), "a driver registered after the refusal:\n{output}");
    let sticky = format!("[blk-router] {}: no live provider; not issued", old.device);
    assert!(find_after(&output, refused_at, &sticky).is_some(), "no sticky refusal {sticky:?}:\n{write}");
    print_evidence(
        "E4b [Ahci]",
        output[death.at..].lines().map(str::trim).filter(|l| {
            l.contains("holding SHM") || l.contains("node recovery") || l.contains("Init: storm state")
        }),
    );
    println!("09C E4b [Ahci] T{} died holding SHM: no restart, later write failed unsent", old.tid);
}

/// E6: an NVMe replacement leaves the e1000 owner alone: no re-registration,
/// no quiesce of the NIC function, no DMA grant for it to any other tid.
pub fn nic_survives_nvme_replacement(iso: &str) {
    let disk = fixture();
    let mut qemu = Lane::Nvme.boot(iso, &disk, 1);
    let nvme = wait_single_disk_boot(&qemu, 1);
    let (nic_tid, nic_bdf) = wait_until(
        &qemu,
        BOOT_TIMEOUT,
        "the e1000 registration and its DMA grant (the fault ISO must carry /bin/e1000)",
        |output| {
            let tid: usize = field(line_at(output, output.find(NIC_REGISTERED)?), "tid=")?.parse().ok()?;
            let grant = output.find(&format!("[iommu] Cell {tid} granted DMA BDF="))?;
            Some((tid, field(line_at(output, grant), "BDF=")?.to_string()))
        },
    );
    assert_ne!(nic_bdf, pci_bdf(&nvme.device), "the NIC and NVMe share a BDF");

    let death = begin_death(&mut qemu, "vwrite /mnt/sd/die.txt F08B-DIE");
    let replacement = finish_replacement(&qemu, &death, &nvme, &["Contained"]);
    write_ok(&mut qemu, "/mnt/sd/e6.txt", "F09C-E6-AFTER");
    read_ok(&mut qemu, "/mnt/sd/e6.txt", "F09C-E6-AFTER");

    let output = qemu.dump();
    let after = &output[death.at..];
    assert!(!after.contains(NIC_REGISTERED), "the e1000 re-registered across the NVMe restart:\n{after}");
    assert!(!after.contains(&format!("[pcie] quiesced {nic_bdf} ")), "the NIC function was quiesced:\n{after}");
    for (at, _) in after.match_indices(" granted DMA BDF=") {
        let line = line_at(after, after[..at].rfind('\n').map_or(0, |i| i + 1));
        if field(line, "BDF=") == Some(nic_bdf.as_str()) {
            assert_eq!(field(line, "Cell "), Some(nic_tid.to_string().as_str()), "NIC BDF granted to another tid: {line}");
        }
    }
    print_evidence("E6 [Nvme+e1000]", &replacement.evidence);
    println!(
        "09C E6 e1000 tid={nic_tid} BDF={nic_bdf} unchanged; NVMe {} T{}->T{}",
        nvme.device, nvme.tid, replacement.new.tid
    );
    shutdown(qemu);
}

/// E8: ten `F08B-DIE` cycles spaced past init's window; every verdict is
/// `Contained` with nothing leaked, and the committed file reads back each time.
pub fn repeated_cycles(lane: Lane, iso: &str) {
    let disk = fixture();
    let mut qemu = lane.boot(iso, &disk, 1);
    let mut current = wait_single_disk_boot(&qemu, 1);
    write_ok(&mut qemu, "/mnt/sd/keep.txt", KEEP);
    for cycle in 1..=10 {
        if cycle > 1 {
            std::thread::sleep(INIT_WINDOW_SPACING);
        }
        let death = begin_death(&mut qemu, &format!("vwrite /mnt/sd/die{cycle}.txt F08B-DIE"));
        let replacement = finish_replacement(&qemu, &death, &current, &["Contained"]);
        assert!(
            replacement.verdict.contains("dma=0 qpages=0 leaked=0)"),
            "cycle {cycle} verdict is not leak-free: {}",
            replacement.verdict
        );
        read_ok(&mut qemu, "/mnt/sd/keep.txt", KEEP);
        println!(
            "09C E8 [{lane:?}] cycle {cycle}: T{}->T{} route={} {}",
            current.tid,
            replacement.new.tid,
            replacement.new.route,
            replacement.verdict.trim()
        );
        current = replacement.new;
    }
    shutdown(qemu);
}
