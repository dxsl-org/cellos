//! Per-path capability ceiling for the cells named in the boot manifest.
//!
//! This is the ceiling applied to a **kernel/boot-initiated** spawn
//! (`Spawner::Root`) and the source of the root authority's own capability
//! write. It replaces a single "full authority" constant.
//!
//! **Invariant — the table is per-path, never a union.** A ceiling only ever
//! constrains when it is *smaller* than the request; the union of every row is
//! by definition a superset of every row, so a union-shaped ceiling would admit
//! everything while looking like a restriction. Each row therefore states the
//! authority that ONE path is expected to hold, and nothing else. `self_test`
//! pins that property so a later edit cannot quietly collapse the table into a
//! union.
//!
//! A path with no row is **unknown**, and an unknown path gets `CapSet::EMPTY`
//! (fail-closed). `lookup` returns `None` for it so the diagnostics can say
//! "missing entry" rather than "entry grants nothing" — the two look identical
//! in the resulting CapSet and need very different fixes.
//!
//! Rows are derived from what each cell's `__ViCell_manifest` requests plus the
//! path-triggered caps `CapSet::with_path_caps` layers on. A row is therefore
//! the *expected* authority of that binary, not a guess.

use crate::task::cap::CapSet;

#[cfg(not(cellos_posture = "release"))]
mod selftest;

/// Boot self-test of the table's load-bearing properties — that it is per-path
/// rather than a union, and that no row is over-tightened. See `selftest`.
#[cfg(not(cellos_posture = "release"))]
pub use selftest::run as self_test;

/// The expected ceiling for `path`, or `None` when `path` is not a boot cell.
/// Rows live in [`crate::catalog`] (`boot` field); the sandbox prefixes yield `EMPTY`.
pub fn lookup(path: &str) -> Option<CapSet> {
    crate::catalog::boot(path)
}

/// The ceiling for `path`; an unknown path yields `CapSet::EMPTY` (fail-closed).
pub fn boot_ceiling(path: &str) -> CapSet {
    lookup(path).unwrap_or(CapSet::EMPTY)
}

/// Report, at `error` level, every cap the boot ceiling refused for `path`.
///
/// Written for a reader who has only a boot log: it prints whether the path is
/// missing from the table at all, the three sets involved, and one line per
/// refused cap named exactly as the table field to add. Nothing here needs to be
/// re-derived from the source to act on it.
pub fn log_refusal(path: &str, requested: CapSet, ceiling: CapSet, granted: CapSet) {
    if lookup(path).is_none() {
        log::error!(
            "[loader] boot-ceiling: {:?} has NO ROW in boot_ceiling::lookup — ceiling is EMPTY. Add a row.",
            path
        );
    } else {
        log::error!(
            "[loader] boot-ceiling: {:?} narrowed by its row — widen the row if the cap is legitimate.",
            path
        );
    }
    log::error!("[loader]   requested = {:?}", requested);
    log::error!("[loader]   ceiling   = {:?}", ceiling);
    log::error!("[loader]   granted   = {:?}", granted);
    if requested.block_io && !granted.block_io {
        log::error!("[loader]   refused: block_io");
    }
    if requested.network && !granted.network {
        log::error!("[loader]   refused: network");
    }
    if requested.spawn && !granted.spawn {
        log::error!("[loader]   refused: spawn");
    }
    if requested.hypervisor && !granted.hypervisor {
        log::error!("[loader]   refused: hypervisor");
    }
    if requested.pcie_driver && !granted.pcie_driver {
        log::error!("[loader]   refused: pcie_driver");
    }
    if requested.platform && !granted.platform {
        log::error!("[loader]   refused: platform");
    }
    if requested.usb_driver && !granted.usb_driver {
        log::error!("[loader]   refused: usb_driver");
    }
    if requested.supervisor && !granted.supervisor {
        log::error!("[loader]   refused: supervisor");
    }
    if requested.net_resolve && !granted.net_resolve {
        log::error!("[loader]   refused: net_resolve");
    }
    if requested.net_connect && !granted.net_connect {
        log::error!("[loader]   refused: net_connect");
    }
    if requested.net_listen && !granted.net_listen {
        log::error!("[loader]   refused: net_listen");
    }
    if requested.net_l2 && !granted.net_l2 {
        log::error!("[loader]   refused: net_l2");
    }
    if requested.observe_tasks && !granted.observe_tasks {
        log::error!("[loader]   refused: observe_tasks");
    }
    if requested.observe_memory && !granted.observe_memory {
        log::error!("[loader]   refused: observe_memory");
    }
    if requested.vfs_mutate && !granted.vfs_mutate {
        log::error!("[loader]   refused: vfs_mutate");
    }
    if requested.config_write && !granted.config_write {
        log::error!("[loader]   refused: config_write");
    }
    if requested.kms_ssh_sign && !granted.kms_ssh_sign {
        log::error!("[loader]   refused: kms_ssh_sign");
    }
    if requested.kms_ssh_admin && !granted.kms_ssh_admin {
        log::error!("[loader]   refused: kms_ssh_admin");
    }
    if requested.mmio_devices != granted.mmio_devices {
        log::error!(
            "[loader]   refused: mmio_devices {:#07b}",
            requested.mmio_devices & !granted.mmio_devices
        );
    }
    if requested.block_regions != granted.block_regions {
        log::error!(
            "[loader]   refused: block_regions {:#06b}",
            requested.block_regions & !granted.block_regions
        );
    }
}
