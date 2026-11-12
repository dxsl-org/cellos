// SPDX-License-Identifier: MPL-2.0
//! The cell catalog: every authority the kernel keys on an install path.
//!
//! One row per path holds what used to live in four separate match tables:
//! - `request` — authority the path asks for on top of its manifest, because no
//!   manifest bit exists for it (P-TRUST driver authority, network rights, Q,
//!   KSSH). Still intersected with the ceiling and the operator policy.
//! - `boot` — the ceiling of a kernel/boot-initiated spawn (`Spawner::Root`).
//! - `user_target` — the child ceiling of a reviewed launch edge
//!   (`loader::launch_profile`); which callers may use it is edge logic, not data.
//! - `trusted_core` — keeps its caps under a `DenyAll` policy (field recovery).
//!
//! Rules that are not per path stay as functions here: the user-app sandbox
//! prefixes, the S2-T/S2-W observe/mutate request, and the legacy
//! manifest-less fallback. `scripts/catalog.generated.json` is exported from
//! this module by a host test; `scripts/sign-policy.py` reads that file instead
//! of parsing Rust.
//!
//! **Invariant — every ceiling is per path, never a union.** A union of rows is
//! a superset of every row and would admit everything while looking like a
//! restriction (`loader::boot_ceiling::self_test` pins this). An unknown path has
//! no boot row and gets `CapSet::EMPTY` (fail-closed).

use crate::resource_registry::{DEV_DISPLAY, DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};
use crate::task::cap::CapSet;

/// One install path's authority rows. See the module documentation.
#[derive(Copy, Clone, Debug)]
pub struct Cell {
    pub path: &'static str,
    pub request: CapSet,
    pub boot: Option<CapSet>,
    pub user_target: Option<CapSet>,
    pub trusted_core: bool,
}

const ROW: Cell = Cell {
    path: "",
    request: CapSet::EMPTY,
    boot: None,
    user_target: None,
    trusted_core: false,
};

/// Install-path prefixes of the user-app sandbox: empty boot and target ceiling,
/// and no S2-T/S2-W request (annex D-N2, D-TW1).
pub const SANDBOX_PREFIXES: [&str; 3] = ["/apps/", "/data/bin/", "/home/"];

pub fn is_sandbox(path: &str) -> bool {
    SANDBOX_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

// ── named ceilings ──────────────────────────────────────────────────────────

/// The pre-annex task/memory observation and VFS-mutation rights (annex
/// S2-T/S2-W §3, D-TW1). Every non-sandbox row keeps them.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;
/// VFS block regions after request ∩ ceiling ∩ policy: P1 | P4 | SRV | cell-store.
/// A `0b111` ceiling would zero the cell-store bit before policy runs.
pub const VFS_REGIONS: u8 = 0b1111;
const CONSOLE_MMIO: u8 = DEV_GPIO | DEV_UART;
const ROOT_MMIO: u8 = CONSOLE_MMIO | DEV_I2C | DEV_SPI | DEV_DISPLAY;
/// `pcie_driver` + the display MMIO class: a PCIe driver claims a BAR and authorises DMA.
const PCIE: CapSet = CapSet {
    pcie_driver: true,
    mmio_devices: DEV_DISPLAY,
    ..CapSet::EMPTY
};
/// Outbound network client: `Resolve` plus outbound sockets (annex S2-N §5).
const NET_CLIENT: CapSet = CapSet {
    net_resolve: true,
    net_connect: true,
    ..CapSet::EMPTY
};
/// Inbound-only server: listening sockets, no DNS and no outbound connect.
const NET_LISTENER: CapSet = CapSet {
    net_listen: true,
    ..CapSet::EMPTY
};
/// Client and server in one Cell (`nc -l`, the cluster broker).
const NET_PEER: CapSet = CapSet {
    net_resolve: true,
    net_connect: true,
    net_listen: true,
    ..CapSet::EMPTY
};
const NET_L2: CapSet = CapSet {
    net_l2: true,
    ..CapSet::EMPTY
};

/// Field-wise OR. Used to layer a row's request and to write rows as
/// "named set + extra rights".
pub const fn union(a: CapSet, b: CapSet) -> CapSet {
    CapSet {
        block_io: a.block_io | b.block_io,
        network: a.network | b.network,
        spawn: a.spawn | b.spawn,
        hypervisor: a.hypervisor | b.hypervisor,
        mmio_devices: a.mmio_devices | b.mmio_devices,
        block_regions: a.block_regions | b.block_regions,
        pcie_driver: a.pcie_driver | b.pcie_driver,
        platform: a.platform | b.platform,
        supervisor: a.supervisor | b.supervisor,
        usb_driver: a.usb_driver | b.usb_driver,
        serial_port: a.serial_port | b.serial_port,
        net_resolve: a.net_resolve | b.net_resolve,
        net_connect: a.net_connect | b.net_connect,
        net_listen: a.net_listen | b.net_listen,
        net_l2: a.net_l2 | b.net_l2,
        observe_tasks: a.observe_tasks | b.observe_tasks,
        observe_memory: a.observe_memory | b.observe_memory,
        vfs_mutate: a.vfs_mutate | b.vfs_mutate,
        config_write: a.config_write | b.config_write,
        kms_ssh_sign: a.kms_ssh_sign | b.kms_ssh_sign,
        kms_ssh_admin: a.kms_ssh_admin | b.kms_ssh_admin,
    }
}

const fn legacy_with(extra: CapSet) -> CapSet {
    union(LEGACY, extra)
}

const fn mmio(mask: u8) -> CapSet {
    CapSet {
        mmio_devices: mask,
        ..LEGACY
    }
}

/// A capability-free boot cell that is also a reviewed launch target.
const fn plain(path: &'static str) -> Cell {
    Cell {
        path,
        boot: Some(LEGACY),
        user_target: Some(LEGACY),
        ..ROW
    }
}

/// A capability-free reviewed launch target that is not a boot cell.
const fn target(path: &'static str) -> Cell {
    Cell {
        path,
        user_target: Some(LEGACY),
        ..ROW
    }
}

/// A capability-free boot cell that is not a reviewed launch target.
const fn boot_only(path: &'static str) -> Cell {
    Cell {
        path,
        boot: Some(LEGACY),
        ..ROW
    }
}

/// A PCIe Driver Cell: requests and may hold `pcie_driver` + `DEV_DISPLAY`.
const fn pcie_driver(path: &'static str) -> Cell {
    Cell {
        path,
        request: PCIE,
        boot: Some(legacy_with(PCIE)),
        ..ROW
    }
}

/// An outbound network client launched by a reviewed edge.
const fn net_client(path: &'static str) -> Cell {
    Cell {
        path,
        request: NET_CLIENT,
        user_target: Some(legacy_with(NET_CLIENT)),
        ..ROW
    }
}

/// A Cell whose reviewed edge keeps lifecycle authority (`spawn`).
const fn spawner_target(path: &'static str) -> Cell {
    Cell {
        path,
        user_target: Some(legacy_with(CapSet {
            spawn: true,
            ..CapSet::EMPTY
        })),
        ..ROW
    }
}

/// Every production catalog row.
pub const CELLS: &[Cell] = &[
    // ── root authority ──────────────────────────────────────────────────────
    // Not spawned from a path (the kernel writes init's caps directly), listed
    // so one table describes boot authority. Widest row: a spawner's caps bound
    // its children. `platform` is absent: the Platform Cell is kernel-spawned
    // and the singleton latch owns `platform_cap`. Network, Q and KSSH rights
    // are not delegated through init's CapSet (launch edges bound children by
    // their reviewed ceiling alone).
    Cell {
        path: "/bin/init",
        boot: Some(CapSet {
            block_io: true,             // delegated to /bin/vfs
            network: true,              // delegated to /bin/net, /bin/net-broker
            spawn: true,                // delegated to /bin/cellbox, /bin/supervisor
            hypervisor: true,           // delegated to /bin/silo, /bin/hypervisor
            mmio_devices: ROOT_MMIO,    // delegated through reviewed launch profiles
            block_regions: VFS_REGIONS, // delegated to /bin/vfs
            pcie_driver: true,          // delegated to the block/NIC/GPU/input drivers
            supervisor: true,           // delegated to /bin/supervisor
            usb_driver: true,           // delegated to /bin/dwc2-usb
            serial_port: true,          // delegated to /bin/serial
            ..LEGACY
        }),
        ..ROW
    },
    // PCIe ECAM enumeration + BAR registration, kernel-spawned before init. Its
    // manifest declares nothing; the owned reservation enforces one holder ever.
    Cell {
        path: "/bin/platform",
        request: CapSet {
            platform: true,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            platform: true,
            ..CapSet::EMPTY
        })),
        trusted_core: true,
        ..ROW
    },
    // ── core services ───────────────────────────────────────────────────────
    // Filesystem service: raw block syscalls plus its partitions. Requests the
    // cell-store region (bit 3) so ceiling and policy bound it like every cap.
    Cell {
        path: "/bin/vfs",
        request: CapSet {
            block_regions: 0b1000,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            block_io: true,
            block_regions: VFS_REGIONS,
            ..CapSet::EMPTY
        })),
        trusted_core: true,
        ..ROW
    },
    // Network stack: NetworkCap drives the NIC; it IS the service that enforces
    // the coarse network rights, so it holds none of them.
    Cell {
        path: "/bin/net",
        boot: Some(legacy_with(CapSet {
            network: true,
            ..CapSet::EMPTY
        })),
        trusted_core: true,
        ..ROW
    },
    // Cluster broker: `network`, plus beacon/multicast, STUN and C2C TCP through
    // the net service (resolve + connect + listen).
    Cell {
        path: "/bin/net-broker",
        request: NET_PEER,
        boot: Some(legacy_with(union(
            NET_PEER,
            CapSet {
                network: true,
                ..CapSet::EMPTY
            },
        ))),
        user_target: Some(legacy_with(union(
            NET_PEER,
            CapSet {
                network: true,
                ..CapSet::EMPTY
            },
        ))),
        ..ROW
    },
    // Interactive shell: no ambient lifecycle authority (its exact launch edges
    // are in `loader::launch_profile`). Q (Phase 01 CP1, D-Q5): `export` may
    // write the shell key namespace of service-config.
    Cell {
        path: "/bin/cellbox",
        request: CapSet {
            config_write: true,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            config_write: true,
            ..CapSet::EMPTY
        })),
        trusted_core: true,
        ..ROW
    },
    // The genuine `cellbox ssh` install path (KSSH K3): only it requests
    // `kms_ssh_sign`. No boot row: the shell launches it through its edge.
    Cell {
        path: "/bin/cellbox-ssh",
        request: union(
            NET_CLIENT,
            CapSet {
                kms_ssh_sign: true,
                ..CapSet::EMPTY
            },
        ),
        ..ROW
    },
    // KSSH K3/K7: only this path requests `kms_ssh_admin`; no boot row or
    // edge grants it yet, so the request is refused until one does.
    Cell {
        path: "/bin/kms-ssh-admin",
        request: CapSet {
            kms_ssh_admin: true,
            ..CapSet::EMPTY
        },
        ..ROW
    },
    // Hotswap orchestration: re-spawns cells (spawn) and freezes/resumes them.
    Cell {
        path: "/bin/supervisor",
        request: CapSet {
            supervisor: true,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            spawn: true,
            supervisor: true,
            ..CapSet::EMPTY
        })),
        ..ROW
    },
    // `/bin/silo` and `/bin/hypervisor` declare `hypervisor`;
    // `CapSet::from_manifest` also gates it on the CPU reporting H-ext/EL2/VMX.
    Cell {
        path: "/bin/silo",
        boot: Some(legacy_with(CapSet {
            hypervisor: true,
            ..CapSet::EMPTY
        })),
        ..ROW
    },
    // The VMM's virtual NIC backend sends and receives raw frames (`net_l2`).
    // No generic `user_target`: only init, the shell's `hv` edge and the Chrome
    // launcher's edge start it, each with exactly this boot ceiling
    // (`launch_profile::profiles::hypervisor_edge`). A user-target row would let
    // every reviewed launcher (desktop route, Hypha tool-spawn) hand it out.
    Cell {
        path: "/bin/hypervisor",
        request: NET_L2,
        boot: Some(legacy_with(union(
            NET_L2,
            CapSet {
                hypervisor: true,
                ..CapSet::EMPTY
            },
        ))),
        ..ROW
    },
    // ── PCIe / device Driver Cells ──────────────────────────────────────────
    pcie_driver("/bin/block"),
    pcie_driver("/bin/nvme"),
    pcie_driver("/bin/e1000"),
    pcie_driver("/bin/e1000e"),
    pcie_driver("/bin/igb"),
    pcie_driver("/bin/virtio-net"),
    pcie_driver("/bin/virtio-gpu"),
    pcie_driver("/bin/virtio-scsi"),
    pcie_driver("/bin/vmxnet3"),
    pcie_driver("/bin/pvscsi"),
    pcie_driver("/bin/input"),
    pcie_driver("/bin/ahci"),
    // xHCI: `pcie_driver` finds and claims the controller; `usb_driver` is the
    // USB-host authority the input service's producer gate verifies.
    Cell {
        path: "/bin/xhci",
        request: union(
            PCIE,
            CapSet {
                usb_driver: true,
                ..CapSet::EMPTY
            },
        ),
        boot: Some(legacy_with(union(
            PCIE,
            CapSet {
                usb_driver: true,
                ..CapSet::EMPTY
            },
        ))),
        ..ROW
    },
    // x86 serial Driver Cell: port-I/O over the 16550 ports the kernel probed;
    // no PCIe, MMIO or DMA.
    Cell {
        path: "/bin/serial",
        request: CapSet {
            serial_port: true,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            serial_port: true,
            ..CapSet::EMPTY
        })),
        ..ROW
    },
    // BCM uses the statically allowlisted VideoCore mailbox, never PCIe.
    Cell {
        path: "/bin/bcm-display",
        request: CapSet {
            mmio_devices: DEV_DISPLAY,
            ..CapSet::EMPTY
        },
        boot: Some(mmio(DEV_DISPLAY)),
        ..ROW
    },
    // The DWC2 host owns the only DWC2 MMIO/DMA authority; its sole child is
    // the capability-free LAN9514 front-end. `spawn` without `supervisor`
    // publishes no well-known service id.
    Cell {
        path: "/bin/dwc2-usb",
        request: CapSet {
            usb_driver: true,
            ..CapSet::EMPTY
        },
        boot: Some(legacy_with(CapSet {
            spawn: true,
            usb_driver: true,
            ..CapSet::EMPTY
        })),
        user_target: Some(legacy_with(CapSet {
            usb_driver: true,
            ..CapSet::EMPTY
        })),
        ..ROW
    },
    boot_only("/bin/lan9514"),
    // ── capability-free boot cells and reviewed targets ─────────────────────
    // Listed rather than left to the unknown-path fallback so a denial report
    // can tell "needs nothing" from "row missing".
    plain("/bin/ocel-js"),
    plain("/bin/ocel-quickjs"),
    plain("/bin/ocel-pdf"),
    plain("/bin/chrome"),
    plain("/bin/vm-runner"),
    plain("/bin/files"),
    plain("/bin/terminal"),
    plain("/bin/vfs-test"),
    plain("/bin/std-smoke"),
    plain("/bin/desktop"),
    plain("/bin/tier2-exploit"),
    plain("/bin/tier2-smoke"),
    plain("/bin/cpp-smoke"),
    plain("/bin/tier2-rpc-provider"),
    plain("/bin/tier2-rpc-driver"),
    // x86 Tier-2 runtime witnesses (owner H-1).
    plain("/bin/mlibc-smoke"),
    plain("/bin/mlibc-posix-x86"),
    plain("/bin/cpp-hosted-smoke"),
    plain("/bin/drm-lite-x86"),
    boot_only("/bin/config"),
    boot_only("/bin/compositor"),
    boot_only("/bin/fb-console"),
    boot_only("/bin/kms"),
    boot_only("/bin/silo-test"),
    boot_only("/bin/srv-test"),
    target("/bin/adc-demo"),
    target("/bin/ai-test"),
    target("/bin/audio-demo"),
    target("/bin/bench-probe"),
    target("/bin/c2c-render-worker"),
    target("/bin/c2c-render-domain-worker"),
    target("/bin/can-demo"),
    target("/bin/cat"),
    target("/bin/cfi-test"),
    target("/bin/c-pthread"),
    target("/bin/tls-test"),
    target("/bin/futex-test"),
    target("/bin/pipe-test"),
    target("/bin/pipe-peer"),
    target("/bin/c-spawn"),
    target("/bin/backend-worker"),
    // Phase 09B: the recovery worker and the client driving it.
    target("/bin/recovery-worker"),
    target("/bin/recovery-client"),
    target("/bin/doom"),
    target("/bin/echo"),
    target("/bin/free"),
    target("/bin/gpio-test-rv"),
    target("/bin/http-smoke"),
    target("/bin/input-test"),
    target("/bin/window-policy-probe"),
    target("/bin/ls"),
    target("/bin/ps"),
    target("/bin/robot-dashboard"),
    target("/bin/viui-demo"),
    target("/bin/tetris"),
    target("/bin/tetris-c"),
    target("/bin/tetris-lua"),
    target("/bin/wx-test"),
    target("/bin/python"),
    // `/bin/lua` keeps no network right (annex S2-N D-N3); `/bin/lua-net` is
    // the same signed binary on a client-network edge (user decision 2026-11-10).
    target("/bin/lua"),
    target("/bin/tool-fs"),
    target("/bin/tool-sys"),
    // ── network clients and servers of the net service ──────────────────────
    // They hold no NetworkCap; their ceiling carries only the coarse rights the
    // net service enforces. A non-empty ceiling makes the caller-owned ELF route
    // refuse the edge, so a shell cannot relabel bytes as a granted worker.
    net_client("/bin/curl"),
    net_client("/bin/wget"),
    net_client("/bin/mqtt"),
    net_client("/bin/llm-gateway"),
    net_client("/bin/https-demo"),
    net_client("/bin/posix-shim-test"),
    net_client("/bin/lua-net"),
    // Ocel is a network client and a capability-free boot cell.
    Cell {
        path: "/bin/ocel",
        request: NET_CLIENT,
        boot: Some(LEGACY),
        user_target: Some(legacy_with(NET_CLIENT)),
        ..ROW
    },
    Cell {
        path: "/bin/nc",
        request: NET_PEER,
        user_target: Some(legacy_with(NET_PEER)),
        ..ROW
    },
    Cell {
        path: "/bin/httpd",
        request: NET_LISTENER,
        user_target: Some(legacy_with(NET_LISTENER)),
        ..ROW
    },
    // SSH server: inbound listener only — no DNS, no outbound connect.
    Cell {
        path: "/bin/sshd",
        request: NET_LISTENER,
        boot: Some(legacy_with(NET_LISTENER)),
        ..ROW
    },
    Cell {
        path: "/bin/robot-demo",
        request: NET_CLIENT,
        user_target: Some(legacy_with(union(
            NET_CLIENT,
            CapSet {
                network: true,
                mmio_devices: DEV_GPIO,
                ..CapSet::EMPTY
            },
        ))),
        ..ROW
    },
    // ── peripheral demos on reviewed edges ──────────────────────────────────
    Cell {
        path: "/bin/periph-demo",
        user_target: Some(mmio(CONSOLE_MMIO)),
        ..ROW
    },
    Cell {
        path: "/bin/periph-test",
        user_target: Some(mmio(CONSOLE_MMIO)),
        ..ROW
    },
    Cell {
        path: "/bin/pwm-demo",
        user_target: Some(mmio(DEV_GPIO)),
        ..ROW
    },
    Cell {
        path: "/bin/sensor-demo",
        user_target: Some(mmio(DEV_GPIO | DEV_I2C)),
        ..ROW
    },
    Cell {
        path: "/bin/spi-demo",
        user_target: Some(mmio(DEV_GPIO | DEV_SPI)),
        ..ROW
    },
    // ── lifecycle parents on reviewed edges (keep `spawn`) ──────────────────
    spawner_target("/bin/bench"),
    spawner_target("/bin/c2c-render"),
    spawner_target("/bin/capacity-probe"),
    spawner_target("/bin/hypha"),
    spawner_target("/bin/tool-spawn"),
    // The B0 supervisor must reach the shell edge with SpawnCap (ADR-0021 §2.4).
    spawner_target("/bin/backend-supervisor"),
    spawner_target("/bin/hotswap-demo-v1"),
    spawner_target("/bin/hotswap-demo-v2"),
    // Phase 09B state authority: lifecycle parent of its worker.
    spawner_target("/bin/recovery-authority"),
];

/// ADR-0033 CP1 §7 QEMU `edu` lanes, test-hooks images only: the export holder
/// drives QEMU's `edu` device; the client owns Grants and exports them.
#[cfg(feature = "test-hooks")]
pub const TEST_HOOK_CELLS: &[Cell] = &[
    pcie_driver("/bin/edu-holder"),
    boot_only("/bin/export-client"),
];

/// Private S2-T/S2-W positive control (annex §6, E1): the probe binary the
/// sandbox copy at `/data/bin/s2tw-probe` runs, on a reviewed edge with X/M/W.
#[cfg(feature = "phase04-driver-profile")]
pub const PHASE04_DRIVER_CELLS: &[Cell] = &[target("/bin/s2tw-probe")];

/// Every row compiled into this kernel, production rows first.
pub fn rows() -> impl Iterator<Item = &'static Cell> {
    let rows = CELLS.iter();
    #[cfg(feature = "test-hooks")]
    let rows = rows.chain(TEST_HOOK_CELLS.iter());
    #[cfg(feature = "phase04-driver-profile")]
    let rows = rows.chain(PHASE04_DRIVER_CELLS.iter());
    rows
}

pub fn find(path: &str) -> Option<&'static Cell> {
    rows().find(|row| row.path == path)
}

/// Authority `path` requests on top of its manifest: the row's `request`, plus
/// the S2-T/S2-W observe/mutate rights for every non-sandbox path (annex §3).
pub fn request(path: &str) -> CapSet {
    let row = find(path).map_or(CapSet::EMPTY, |row| row.request);
    if is_sandbox(path) {
        row
    } else {
        union(row, LEGACY)
    }
}

/// Boot ceiling row, `Some(EMPTY)` for the sandbox, `None` for an unknown path.
pub fn boot(path: &str) -> Option<CapSet> {
    match find(path).and_then(|row| row.boot) {
        Some(caps) => Some(caps),
        None if is_sandbox(path) => Some(CapSet::EMPTY),
        None => None,
    }
}

/// Reviewed launch-target ceiling, `Some(EMPTY)` for the sandbox, else `None`.
pub fn user_target(path: &str) -> Option<CapSet> {
    match find(path).and_then(|row| row.user_target) {
        Some(caps) => Some(caps),
        None if is_sandbox(path) => Some(CapSet::EMPTY),
        None => None,
    }
}

/// Install paths whose bytes are another program's signed image, so the image
/// name would claim that program's launch edges. `/bin/cellbox-ssh` is the
/// signed `cellbox` file run as the network-facing SSH client; it must not act
/// as the operator shell (KSSH K7: the sign holder never reaches the admin Cell).
pub const IDENTITY_ALIASES: &[&str] = &["/bin/cellbox-ssh"];

/// Whether a Cell launched by the kernel from `path` may be named by its signed
/// image (Phase 06): a `/bin/` install path (the same rule that admits a
/// privileged manifest, `governed_spawn`) that is not an alias. Callers must
/// also have resolved `path` in the kernel (Path, Pinned or boot route); a
/// caller-supplied ELF's advisory path never qualifies.
pub fn names_launch_identity(path: &str) -> bool {
    path.starts_with("/bin/") && !IDENTITY_ALIASES.contains(&path)
}

pub fn is_trusted_core(path: &str) -> bool {
    find(path).is_some_and(|row| row.trusted_core)
}

/// Legacy grants for a cell without a `__ViCell_manifest`, still bounded by the
/// launch request. Suffix match on `/bin/` paths only (historic behaviour).
pub fn legacy_manifestless(path: &str) -> CapSet {
    let mut c = CapSet::EMPTY;
    if path.starts_with("/bin/") {
        if path.ends_with("/bin/vfs") {
            c.block_io = true;
            c.block_regions = 0b111; // legacy fallback: P1 + P4 + SRV
        }
        if path.ends_with("/bin/net") {
            c.network = true;
        }
        if path.ends_with("/bin/cellbox") || path.ends_with("/bin/init") {
            c.spawn = true;
        }
    }
    c
}

// ── PCI functions each driver program may claim (H-3) ───────────────────────

/// One PCI match a driver program may query with 418 (class triple) or 424
/// (vendor:device). `prog_if: None` admits every programming interface.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PciMatch {
    Class {
        class: u8,
        subclass: u8,
        prog_if: Option<u8>,
    },
    Id {
        vendor: u16,
        device: u16,
    },
}

const fn id(vendor: u16, device: u16) -> PciMatch {
    PciMatch::Id { vendor, device }
}

const VIRTIO: u16 = 0x1AF4;
const VMWARE: u16 = 0x15AD;
const INTEL: u16 = 0x8086;

/// PCI functions per **measured program name** (`__ViCell_image`, the name the
/// kernel attests for the caller's Cell — never an install path or task name).
/// Holding `pcie_driver` is necessary but no longer sufficient: 418/424 also
/// refuse a query outside the caller's row, so one driver cannot claim another
/// driver's controller by asking first. A program with no row claims nothing.
pub const PCI_DRIVERS: &[(&str, &[PciMatch])] = &[
    (
        "nvme",
        &[PciMatch::Class {
            class: 0x01,
            subclass: 0x08,
            prog_if: Some(0x02),
        }],
    ),
    // AHCI, plus the two non-AHCI SATA prog-ifs it probes only to log a refusal.
    (
        "ahci",
        &[PciMatch::Class {
            class: 0x01,
            subclass: 0x06,
            prog_if: None,
        }],
    ),
    (
        "xhci",
        &[PciMatch::Class {
            class: 0x0C,
            subclass: 0x03,
            prog_if: Some(0x30),
        }],
    ),
    ("virtio-blk", &[id(VIRTIO, 0x1042), id(VIRTIO, 0x1001)]),
    ("virtio-scsi", &[id(VIRTIO, 0x1048), id(VIRTIO, 0x1004)]),
    ("virtio-net", &[id(VIRTIO, 0x1041), id(VIRTIO, 0x1000)]),
    ("vmxnet3", &[id(VMWARE, 0x07B0)]),
    ("pvscsi", &[id(VMWARE, 0x07C0)]),
    ("e1000", &[id(INTEL, 0x100E), id(INTEL, 0x100F)]),
    ("e1000e", &[id(INTEL, 0x10D3)]),
    (
        "igb",
        &[id(INTEL, 0x10C9), id(INTEL, 0x1533), id(INTEL, 0x1521)],
    ),
    // ADR-0033 CP1 §7 QEMU `edu` holder; only test-hooks kernels give it
    // `pcie_driver`, so the row is inert elsewhere.
    ("edu-holder", &[id(0x1234, 0x11E8)]),
];

fn pci_row(program: &str) -> &'static [PciMatch] {
    PCI_DRIVERS
        .iter()
        .find(|(name, _)| *name == program)
        .map_or(&[], |(_, matches)| *matches)
}

/// Whether `program` may query the class triple with 418.
pub fn pci_class_allowed(program: &str, class: u8, subclass: u8, prog_if: u8) -> bool {
    pci_row(program).iter().any(|m| {
        matches!(*m, PciMatch::Class { class: c, subclass: s, prog_if: p }
            if c == class && s == subclass && p.is_none_or(|p| p == prog_if))
    })
}

/// Whether `program` may query `vendor:device` with 424.
pub fn pci_id_allowed(program: &str, vendor: u16, device: u16) -> bool {
    pci_row(program).contains(&PciMatch::Id { vendor, device })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Ethernet ID the kernel hands out is claimable by the program
    /// installed at the path the table names, and by no other NIC program.
    #[test]
    fn ethernet_ids_belong_to_their_driver_program_only() {
        use crate::dev::pcie_ecam::ETHERNET_DRIVER_IDS;
        for (vendor, device, path) in ETHERNET_DRIVER_IDS {
            let owner = path.trim_start_matches("/bin/");
            assert!(
                pci_id_allowed(owner, vendor, device),
                "{owner} {vendor:#x}:{device:#x}"
            );
            for (other, _) in PCI_DRIVERS.iter().filter(|(name, _)| *name != owner) {
                assert!(
                    !pci_id_allowed(other, vendor, device),
                    "{other} may claim {path}'s device"
                );
            }
        }
    }

    /// Only kernel install paths name a Cell; the SSH alias of the shell and
    /// sandbox copies of a signed program do not.
    #[test]
    fn launch_identity_needs_a_non_alias_install_path() {
        assert!(names_launch_identity("/bin/cellbox"));
        assert!(names_launch_identity("/bin/shutdown"));
        assert!(!names_launch_identity("/bin/cellbox-ssh"));
        assert!(!names_launch_identity("/home/x/shutdown"));
        assert!(!names_launch_identity("/apps/cellbox"));
        assert!(!names_launch_identity("/srv/sshd"));
    }

    #[test]
    fn class_drivers_cannot_claim_each_other() {
        assert!(pci_class_allowed("nvme", 0x01, 0x08, 0x02));
        assert!(!pci_class_allowed("ahci", 0x01, 0x08, 0x02));
        assert!(pci_class_allowed("ahci", 0x01, 0x06, 0x00));
        assert!(!pci_class_allowed("xhci", 0x01, 0x06, 0x01));
        assert!(!pci_class_allowed("", 0x01, 0x08, 0x02));
        assert!(!pci_id_allowed("input", VIRTIO, 0x1042));
    }
}
