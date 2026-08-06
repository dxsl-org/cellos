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

use crate::resource_registry::{DEV_GPIO, DEV_UART};
use crate::task::cap::CapSet;

mod selftest;

/// Boot self-test of the table's load-bearing properties — that it is per-path
/// rather than a union, and that no row is over-tightened. See `selftest`.
pub use selftest::run as self_test;

/// Block regions VFS must be able to hold after request ∩ ceiling ∩ policy:
/// P1 | P4 | SRV | cell-store.
///
/// `/bin/vfs` now requests bit 3 in `CapSet::with_path_caps`, the boot ceiling
/// must preserve it, and the signed `/POLICY.BIN` row must also preserve it.
/// A `0b111` ceiling here would still zero the cell-store bit before policy
/// runs, so this row remains `0b1111`.
const VFS_REGIONS: u8 = 0b1111;

/// The two console peripherals that path-triggered demo rows may request.
const CONSOLE_MMIO: u8 = DEV_GPIO | DEV_UART;

/// The expected ceiling for `path`, or `None` when `path` is not a boot cell.
///
/// Every row carries the reason that path holds those caps. Prefer a row that is
/// marginally too generous over a missing one: one cap too many is a follow-up,
/// a cell that cannot start is a dead system.
pub fn lookup(path: &str) -> Option<CapSet> {
    let caps = match path {
        // Root authority. Not spawned from a path at all — the kernel writes
        // these caps directly — but it is listed here so there is exactly one
        // table describing boot authority. It is the widest row because a
        // spawner's caps bound its children, and this cell brings up every
        // service below. `platform` is deliberately absent: the Platform Cell is
        // kernel-spawned, and `CapSet::apply_to` never writes `platform_cap`
        // (the singleton latch owns it), so the root authority never actually
        // held it and cannot delegate it.
        "/bin/init" => CapSet {
            block_io: true,             // delegated to /bin/vfs
            network: true,              // delegated to /bin/net, /bin/net-broker
            spawn: true,                // delegated to /bin/shell, /bin/supervisor
            hypervisor: true,           // delegated to /bin/silo, /bin/hypervisor
            mmio_devices: CONSOLE_MMIO, // delegated to /bin/shell
            block_regions: VFS_REGIONS, // delegated to /bin/vfs
            pcie_driver: true,          // delegated to the block/NIC/GPU/input drivers
            platform: false,
            supervisor: true, // delegated to /bin/supervisor
        },
        // PCIe ECAM enumeration + BAR registration. Kernel-spawned before init.
        // Its manifest declares nothing; `with_path_caps` is the request signal,
        // and `try_grant_platform` still enforces one holder ever.
        "/bin/platform" => CapSet {
            platform: true,
            ..CapSet::EMPTY
        },
        // Filesystem service: raw block syscalls plus its partitions.
        "/bin/vfs" => CapSet {
            block_io: true,
            block_regions: VFS_REGIONS,
            ..CapSet::EMPTY
        },
        // Network stack and the cluster broker; both declare `network` only.
        "/bin/net" | "/bin/net-broker" => CapSet {
            network: true,
            ..CapSet::EMPTY
        },
        // Interactive shell: no ambient lifecycle authority. Exact launch edges
        // are reviewed in `loader::launch_profile`; the shell itself holds none.
        "/bin/shell" => CapSet::EMPTY,
        // Hotswap orchestration: re-spawns cells (spawn) and freezes/resumes
        // them (supervisor, from `with_path_caps`).
        "/bin/supervisor" => CapSet {
            spawn: true,
            supervisor: true,
            ..CapSet::EMPTY
        },
        // Both declare `hypervisor`. `CapSet::from_manifest` additionally gates
        // that bit on the CPU actually reporting H-ext/EL2/VMX, so this row
        // cannot conjure it on hardware that lacks virtualisation.
        "/bin/silo" | "/bin/hypervisor" => CapSet {
            hypervisor: true,
            ..CapSet::EMPTY
        },
        // Driver cells: each claims a BAR/MMIO range and authorises DMA, which
        // needs `pcie_driver`. Their manifests declare nothing — the install
        // path is the request signal (`with_path_caps`).
        "/bin/block" | "/bin/nvme" | "/bin/e1000" | "/bin/virtio-net" | "/bin/virtio-gpu"
        | "/bin/input" => CapSet {
            pcie_driver: true,
            ..CapSet::EMPTY
        },
        // Known boot cells that need no authority at all: pure IPC clients. They
        // are listed rather than left to the unknown-path fallback so a denial
        // report can distinguish "needs nothing" from "row missing".
        "/bin/config" | "/bin/compositor" | "/bin/fb-console" | "/bin/silo-test"
        | "/bin/vfs-test" | "/bin/srv-test" => CapSet::EMPTY,
        _ => return None,
    };
    Some(caps)
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
    if requested.supervisor && !granted.supervisor {
        log::error!("[loader]   refused: supervisor");
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
