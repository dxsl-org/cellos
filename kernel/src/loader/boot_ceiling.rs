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

use crate::resource_registry::{DEV_DISPLAY, DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};
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
const ROOT_MMIO: u8 = CONSOLE_MMIO | DEV_I2C | DEV_SPI | DEV_DISPLAY;
/// The pre-annex task/memory observation and VFS-mutation rights (annex
/// S2-T/S2-W §3, D-TW1). Every boot row keeps them; only the user-app sandbox
/// prefixes go without.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;
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
            spawn: true,                // delegated to /bin/cellbox, /bin/supervisor
            hypervisor: true,           // delegated to /bin/silo, /bin/hypervisor
            mmio_devices: ROOT_MMIO,    // delegated through reviewed launch profiles
            block_regions: VFS_REGIONS, // delegated to /bin/vfs
            pcie_driver: true,          // delegated to the block/NIC/GPU/input drivers
            platform: false,
            supervisor: true, // delegated to /bin/supervisor
            usb_driver: true, // delegated to /bin/dwc2-usb
            serial_port: true, // delegated to /bin/serial
            // Network rights are not delegated through init's own CapSet: launch
            // edges bound children by their reviewed ceiling alone, and init sends
            // no network request itself.
            net_resolve: false,
            net_connect: false,
            net_listen: false,
            net_l2: false,
            // S2-T/S2-W rights: init lists and supervises cells and writes its
            // own state, exactly as before the annex.
            observe_tasks: true,
            observe_memory: true,
            vfs_mutate: true,
            // Q is not delegated through init's own CapSet (launch edges bound
            // children by their reviewed ceiling), and init writes no Config key.
            config_write: false,
        },
        // PCIe ECAM enumeration + BAR registration. Kernel-spawned before init.
        // Its manifest declares nothing; `with_path_caps` is the request signal,
        // and the owned reservation still enforces one holder ever.
        "/bin/platform" => CapSet {
            platform: true,
            ..LEGACY
        },
        // Filesystem service: raw block syscalls plus its partitions.
        "/bin/vfs" => CapSet {
            block_io: true,
            block_regions: VFS_REGIONS,
            ..LEGACY
        },
        // Network stack: NetworkCap drives the NIC; it IS the service that
        // enforces the coarse network rights, so it holds none of them.
        "/bin/net" => CapSet {
            network: true,
            ..LEGACY
        },
        // Cluster broker: `network`, plus beacon/multicast, STUN and C2C TCP
        // through the net service (resolve + connect + listen).
        "/bin/net-broker" => CapSet {
            network: true,
            net_resolve: true,
            net_connect: true,
            net_listen: true,
            ..LEGACY
        },
        // Interactive shell (`cellbox sh`): no ambient lifecycle authority. Exact
        // launch edges are reviewed in `loader::launch_profile`; it holds none.
        // Q (Phase 01 CP1, D-Q5): `export` may write the shell key namespace of
        // service-config; the service bounds the keys.
        "/bin/cellbox" => CapSet {
            config_write: true,
            ..LEGACY
        },
        // Hotswap orchestration: re-spawns cells (spawn) and freezes/resumes
        // them (supervisor, from `with_path_caps`).
        "/bin/supervisor" => CapSet {
            spawn: true,
            supervisor: true,
            ..LEGACY
        },
        // `/bin/silo` and `/bin/hypervisor` both declare `hypervisor`.
        // `CapSet::from_manifest` additionally gates that bit on the CPU actually
        // reporting H-ext/EL2/VMX, so neither row can conjure it on hardware that
        // lacks virtualisation.
        "/bin/silo" => CapSet {
            hypervisor: true,
            ..LEGACY
        },
        // The VMM's virtual NIC backend also sends and receives raw frames
        // (`net_l2`); silo has no NIC backend and holds no network right.
        "/bin/hypervisor" => CapSet {
            hypervisor: true,
            net_l2: true,
            ..LEGACY
        },
        // PCIe driver cells: each claims a BAR/MMIO range and authorises DMA.
        // `pcie_driver` + `DEV_DISPLAY` are set here; `with_path_caps` adds them
        // to the request before the ceiling intersection.
        "/bin/block" | "/bin/nvme" | "/bin/e1000" | "/bin/igb" | "/bin/virtio-net" | "/bin/virtio-gpu"
        | "/bin/input" | "/bin/ahci" => CapSet {
            pcie_driver: true,
            mmio_devices: DEV_DISPLAY,
            ..LEGACY
        },
        // USB host controller (xHCI) Driver Cell: `pcie_driver` locates the
        // controller through `sys_find_pcie_device` (0C:03:30) and claims its
        // BAR; `usb_driver` is the USB-host authority the input service's
        // producer gate verifies before it accepts raw key events, the same
        // mechanism `/bin/dwc2-usb` uses. `with_path_caps` requests both.
        "/bin/xhci" => CapSet {
            pcie_driver: true,
            usb_driver: true,
            mmio_devices: DEV_DISPLAY,
            ..LEGACY
        },
        // x86 serial Driver Cell: port-I/O authority over the 16550 ports the
        // kernel probed. No PCIe, no MMIO, no DMA — legacy I/O space only, and
        // only the ports that answered the probe.
        "/bin/serial" => CapSet {
            serial_port: true,
            ..LEGACY
        },
        // BCM uses the statically allowlisted VideoCore mailbox, never PCIe.
        "/bin/bcm-display" => CapSet {
            mmio_devices: DEV_DISPLAY,
            ..LEGACY
        },
        // The host owns the only DWC2 MMIO/DMA authority. Its sole child is the
        // capability-free LAN9514 protocol front-end.
        "/bin/dwc2-usb" => CapSet {
            spawn: true,
            usb_driver: true,
            ..LEGACY
        },
        "/bin/lan9514" => LEGACY,
        // Known boot cells that need no authority at all beyond the pre-annex
        // X/M/W rights: pure IPC clients. They are listed rather than left to the
        // unknown-path fallback so a denial report can distinguish "needs
        // nothing" from "row missing".
        "/bin/ocel" | "/bin/ocel-js" | "/bin/ocel-quickjs" | "/bin/ocel-pdf" | "/bin/chrome" | "/bin/vm-runner" | "/bin/files" | "/bin/terminal" => LEGACY,
        "/bin/config" | "/bin/compositor" | "/bin/fb-console" | "/bin/kms" | "/bin/silo-test"
        | "/bin/vfs-test" | "/bin/srv-test" | "/bin/std-smoke" | "/bin/desktop"
        | "/bin/tier2-exploit" | "/bin/tier2-smoke" | "/bin/cpp-smoke"
        | "/bin/tier2-rpc-provider" | "/bin/tier2-rpc-driver" => LEGACY,
        // SSH server: inbound listener only — no DNS, no outbound connect.
        "/bin/sshd" => CapSet {
            net_listen: true,
            ..LEGACY
        },
        _ => {
            // User-app sandbox: no authority at all, including no S2-T/S2-W
            // rights (annex D-TW1).
            if path.starts_with("/apps/") || path.starts_with("/data/bin/") || path.starts_with("/home/") {
                return Some(CapSet::EMPTY);
            }
            return None;
        }
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
