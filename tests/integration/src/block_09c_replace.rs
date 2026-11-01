//! The E1 chain of one verdict-gated block-driver replacement: the dead
//! driver's function is quiesced, init logs the retirement verdict, the
//! replacement resets the controller, is granted DMA for the same BDF only after
//! the verdict, and rebinds the **same** route under a new tid.

use crate::block_09c::{find_after, line_at, pci_bdf, registrations, wait_after, wait_until, Death, Registration};
use crate::block_09c::{CMD_TIMEOUT, RESTART_TIMEOUT};
use crate::QemuRunner;

/// Evidence of one replacement.
pub struct Replacement {
    pub old: Registration,
    pub new: Registration,
    /// Init's `Init: {driver} retirement {kind} (…)` line.
    pub verdict: String,
    /// Every chain line after the death, in serial order.
    pub evidence: Vec<String>,
}

fn reset_marker(driver: &str) -> &'static str {
    match driver {
        "ahci" => "[ahci] HBA reset complete",
        "nvme" => "[nvme] controller reset complete",
        other => panic!("no reset marker for driver {other:?}"),
    }
}

/// Wait for the replacement of `old.device` after `death` and assert the chain.
/// `verdicts` lists the acceptable retirement kinds (`Contained`, `Quarantined`).
pub fn finish_replacement(qemu: &QemuRunner, death: &Death, old: &Registration, verdicts: &[&str]) -> Replacement {
    let new = wait_until(qemu, RESTART_TIMEOUT, "the replacement's route registration", |output| {
        registrations(output, death.at).into_iter().find(|r| r.device == old.device && r.tid != old.tid)
    });
    wait_after(qemu, "Init: service restarted.", death.at, CMD_TIMEOUT);
    let output = qemu.dump();
    let fail = |what: &str| -> ! { panic!("E1 chain: {what}\n--- serial after the death ---\n{}", &output[death.at..]) };
    let bdf = pci_bdf(&old.device);

    let quiesce = find_after(&output, death.at, &format!("[pcie] quiesced {bdf} for tid {}:", old.tid))
        .unwrap_or_else(|| fail("no quiesce line for the dead driver's function"));
    let verdict_at = find_after(&output, death.at, &format!("Init: {} retirement ", death.driver))
        .unwrap_or_else(|| fail("no retirement verdict line from init"));
    let verdict = line_at(&output, verdict_at).to_string();
    if !verdicts.iter().any(|kind| verdict.contains(&format!("retirement {kind} ("))) {
        fail(&format!("verdict {verdict:?} is not one of {verdicts:?}"));
    }
    find_after(&output, verdict_at, "Init: service restarted.")
        .unwrap_or_else(|| fail("no `Init: service restarted.` after the verdict"));
    let reset = find_after(&output, verdict_at, reset_marker(&death.driver))
        .unwrap_or_else(|| fail("no controller reset by the replacement"));
    let grant = find_after(&output, death.at, &format!("[iommu] Cell {} granted DMA BDF={bdf} ", new.tid))
        .unwrap_or_else(|| fail("no DMA grant for the replacement on the same BDF"));

    if !(quiesce < verdict_at && verdict_at < grant && grant < new.at && verdict_at < reset && reset < new.at) {
        fail(&format!(
            "order quiesce={quiesce} verdict={verdict_at} reset={reset} grant={grant} registered={}",
            new.at
        ));
    }
    if new.route != old.route {
        fail(&format!("route moved from {} to {}", old.route, new.route));
    }
    let foreign: Vec<Registration> = registrations(&output, death.at).into_iter().filter(|r| r.device != old.device).collect();
    if !foreign.is_empty() {
        fail(&format!("another device re-registered during this restart: {foreign:?}"));
    }
    let after = &output[death.at..];
    if after.contains("not quiescent") || after.contains("restart storm") {
        fail("the function was retained or init gave up");
    }

    let evidence = after
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.contains("fault-inject")
                || line.contains("[pcie] quiesced")
                || line.starts_with("Init:")
                || line.contains(reset_marker(&death.driver))
                || line.contains(&format!("granted DMA BDF={bdf} "))
                || line.contains("block driver registered")
                || line.contains("[vfs] block device")
                || line.contains("[blk-router]")
        })
        .map(str::to_string)
        .collect();
    Replacement { old: old.clone(), new, verdict, evidence }
}

/// Wait for VFS to re-resolve `device` to `tid` after `from`; returns
/// `(cell, gen)` from `[vfs] block device {device} provider tid={tid} cell=C gen=G`.
pub fn wait_provider(qemu: &QemuRunner, from: usize, device: &str, tid: usize) -> (String, String) {
    let needle = format!("[vfs] block device {device} provider tid={tid} ");
    let at = wait_until(qemu, CMD_TIMEOUT, &needle, |output| find_after(output, from, &needle));
    let output = qemu.dump();
    let line = line_at(&output, at);
    let cell = crate::block_09c::field(line, "cell=").unwrap_or_default().to_string();
    let generation = crate::block_09c::field(line, "gen=").unwrap_or_default().to_string();
    (cell, generation)
}

/// Print `09C {row} {line}` for each evidence line.
pub fn print_evidence<S: AsRef<str>>(row: &str, lines: impl IntoIterator<Item = S>) {
    for line in lines {
        println!("09C {row} {}", line.as_ref());
    }
}
