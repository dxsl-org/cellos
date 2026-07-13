//! Boot self-test for the P-TRUST spawn trust-model repair.
//!
//! Verifies the invariant that closes the live DMA-anywhere / LBI bypass: the
//! privileged path-triggered caps (`pcie_driver` / `platform` / `supervisor`) are
//! now carried in `CapSet` and bounded by the spawn-time ceiling intersection,
//! instead of being minted by a raw `path ==` match after (and blind to) it.
//!
//! Exercises the exact functions the loader runs — `CapSet::with_path_caps` (the
//! request) and `CapSet::intersect` (the ceiling gate) — on the real target/arch.
//! The positive end-to-end path (init's Root ceiling grants driver cells their
//! caps) is already covered by boot: the block/NIC/GPU/input Driver Cells only
//! register if they received `pcie_driver` and could then claim MMIO. This test
//! adds the negative direction: a non-privileged spawner cannot mint them.

use super::cap::CapSet;

/// One (path, cap-accessor) case exercised by `self_test`.
type PrivCapCase = (&'static str, fn(&CapSet) -> bool);

/// Returns true iff the ceiling correctly bounds every privileged path-cap.
pub fn self_test() -> bool {
    let mut ok = true;

    // A spawner that holds SpawnCap but none of the privileged caps — the actor
    // the C1 exploit assumed (any SpawnCap holder reaching sys_spawn_from_elf).
    let non_priv = CapSet { spawn: true, ..CapSet::EMPTY };

    // Each privileged cap: path REQUESTS it, non-priv ceiling DROPS it (C1 closed),
    // init's ALL ceiling PRESERVES it (the legitimate cell still works).
    let cases: [PrivCapCase; 3] = [
        ("/bin/nvme",       |c| c.pcie_driver),
        ("/bin/platform",   |c| c.platform),
        ("/bin/supervisor", |c| c.supervisor),
    ];

    for (path, sel) in cases {
        let requested = CapSet::EMPTY.with_path_caps(path);
        if !sel(&requested) {
            ok = false;
            log::error!("[selftest] P-TRUST: FAIL — path {} did not request its cap", path);
        }
        if sel(&requested.intersect(non_priv)) {
            ok = false;
            log::error!("[selftest] P-TRUST: FAIL — {} cap survived a non-privileged ceiling (C1 OPEN)", path);
        }
        if !sel(&requested.intersect(CapSet::ALL)) {
            ok = false;
            log::error!("[selftest] P-TRUST: FAIL — {} cap lost under init's Root ceiling (over-tighten)", path);
        }
    }

    if ok {
        log::info!("[selftest] P-TRUST: PASS (privileged path-caps bounded by ceiling)");
    } else {
        log::error!("[selftest] P-TRUST: FAIL");
    }
    ok
}
