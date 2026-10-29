//! Boot self-test for the per-path boot-ceiling table.
//!
//! Checks the two properties the table exists for: it is **per-path** (no row is
//! the union of the others, so the ceiling can actually bind), and no row is
//! *over*-tightened out of a cap its cell legitimately needs.
//!
//! Deliberately returns a bool and logs, rather than using `assert!`: it runs on
//! every boot, so a wrong expectation here must print the offending row, not
//! panic the kernel before anything has started.

use super::{boot_ceiling, lookup, VFS_REGIONS};
use crate::loader::launch_profile::{authorize, CallerLaunchState, LaunchRoute};
use crate::task::cap::CapSet;

/// One (path, cap-accessor) case: the cap that path must still hold.
type PrivCapCase = (&'static str, fn(&CapSet) -> bool);

const SHELL: CallerLaunchState<'static> = CallerLaunchState {
    name: "cellbox",
    has_spawn: false,
    has_supervisor: false,
};

/// Returns `true` when every table property holds; logs each violation.
pub fn run() -> bool {
    let mut ok = true;

    // An unknown path must yield no authority at all, and must be reported as
    // unknown (not as a row that happens to grant nothing).
    if boot_ceiling("/bin/no-such-cell") != CapSet::EMPTY || lookup("/bin/no-such-cell").is_some() {
        ok = false;
        log::error!("[selftest] boot-ceiling: unknown path did not fail closed");
    }

    // Not a union: for every cap, some row must LACK it. A union-shaped table
    // fails all of these, which is the whole point of pinning them.
    let platform_only = CapSet {
        platform: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    let per_path = [
        (
            "/bin/vfs holds pcie_driver",
            !boot_ceiling("/bin/vfs").pcie_driver,
        ),
        (
            "/bin/nvme holds block_io",
            !boot_ceiling("/bin/nvme").block_io,
        ),
        ("/bin/net holds spawn", !boot_ceiling("/bin/net").spawn),
        (
            "/bin/cellbox holds network",
            !boot_ceiling("/bin/cellbox").network,
        ),
        ("/bin/cellbox holds spawn", !boot_ceiling("/bin/cellbox").spawn),
        (
            "/bin/cellbox holds block_regions",
            boot_ceiling("/bin/cellbox").block_regions == 0,
        ),
        (
            "/bin/supervisor holds mmio",
            boot_ceiling("/bin/supervisor").mmio_devices == 0,
        ),
        (
            "/bin/platform is wider than platform",
            boot_ceiling("/bin/platform") == platform_only,
        ),
        (
            "/bin/init holds platform",
            !boot_ceiling("/bin/init").platform,
        ),
    ];
    for (violation, holds) in per_path {
        if !holds {
            ok = false;
            log::error!("[selftest] boot-ceiling: union collapse — {}", violation);
        }
    }

    // The `/bin/vfs` cell-store region bit must survive the ceiling, which runs
    // BEFORE policy: a `0b111` row silently zeroes it whatever the policy says.
    if boot_ceiling("/bin/vfs").block_regions != VFS_REGIONS
        || boot_ceiling("/bin/init").block_regions != VFS_REGIONS
    {
        ok = false;
        log::error!("[selftest] boot-ceiling: /bin/vfs cell-store region would be zeroed");
    }

    // Positive direction: each privileged boot cell must still receive the cap
    // its install path requests once the ceiling is intersected.
    let privileged: [PrivCapCase; 10] = [
        ("/bin/platform", |c| c.platform),
        ("/bin/supervisor", |c| c.supervisor),
        ("/bin/block", |c| c.pcie_driver),
        ("/bin/nvme", |c| c.pcie_driver),
        ("/bin/e1000", |c| c.pcie_driver),
        ("/bin/igb", |c| c.pcie_driver),
        ("/bin/virtio-net", |c| c.pcie_driver),
        ("/bin/virtio-gpu", |c| c.pcie_driver),
        ("/bin/input", |c| c.pcie_driver),
        ("/bin/dwc2-usb", |c| c.usb_driver),
    ];
    for (path, held) in privileged {
        let requested = CapSet::EMPTY.with_path_caps(path);
        if !held(&requested.intersect(boot_ceiling(path))) {
            ok = false;
            log::error!(
                "[selftest] boot-ceiling: {} would LOSE its privileged cap — row over-tightened",
                path
            );
        }
    }

    let bcm = CapSet::EMPTY
        .with_path_caps("/bin/bcm-display")
        .intersect(boot_ceiling("/bin/bcm-display"));
    if bcm.mmio_devices != crate::resource_registry::DEV_DISPLAY || bcm.pcie_driver {
        ok = false;
        log::error!(
            "[selftest] boot-ceiling: /bin/bcm-display must hold display-only MMIO authority"
        );
    }

    // Scope check: /bin/dwc2-usb holds ONLY usb_driver under policy v3, never pcie_driver or DEV_DISPLAY.
    let dwc2 = CapSet::EMPTY
        .with_path_caps("/bin/dwc2-usb")
        .intersect(boot_ceiling("/bin/dwc2-usb"));
    if !dwc2.usb_driver || dwc2.pcie_driver || dwc2.mmio_devices != 0 {
        ok = false;
        log::error!(
            "[selftest] boot-ceiling: /bin/dwc2-usb must hold usb_driver only (no pcie or display)"
        );
    }
    // Scope check: /bin/serial holds ONLY serial_port — port-I/O authority over
    // the probed 16550 ports, never PCIe/MMIO/DMA.
    let serial = CapSet::EMPTY
        .with_path_caps("/bin/serial")
        .intersect(boot_ceiling("/bin/serial"));
    if !serial.serial_port
        || serial.pcie_driver
        || serial.platform
        || serial.usb_driver
        || serial.mmio_devices != 0
        || serial.block_io
        || serial.network
    {
        ok = false;
        log::error!(
            "[selftest] boot-ceiling: /bin/serial must hold serial_port only (no pcie/mmio/dma)"
        );
    }
    // /bin/lan9514 holds NO direct hardware authority (pure IPC client to dwc2-usb);
    // only the pre-annex S2-T/S2-W rights every reviewed row keeps.
    let lan = CapSet::EMPTY
        .with_path_caps("/bin/lan9514")
        .intersect(boot_ceiling("/bin/lan9514"));
    if lan != CapSet::LEGACY_OBSERVE_MUTATE {
        ok = false;
        log::error!(
            "[selftest] boot-ceiling: /bin/lan9514 granted {:?} — must hold only X/M/W",
            lan
        );
    }

    // S2-T/S2-W (D-TW1): every boot row keeps task/memory observation and VFS
    // mutation after request ∩ ceiling; the user-app sandbox gets none.
    for path in [
        "/bin/init",
        "/bin/platform",
        "/bin/vfs",
        "/bin/net",
        "/bin/cellbox",
        "/bin/supervisor",
        "/bin/nvme",
        "/bin/lan9514",
        "/bin/compositor",
        "/bin/sshd",
    ] {
        let g = CapSet::EMPTY
            .with_path_caps(path)
            .intersect(boot_ceiling(path));
        if !(g.observe_tasks && g.observe_memory && g.vfs_mutate) {
            ok = false;
            log::error!("[selftest] boot-ceiling: {} lost a pre-annex X/M/W right", path);
        }
    }
    for path in ["/apps/demo", "/data/bin/tool", "/home/user/app"] {
        let g = CapSet::EMPTY
            .with_path_caps(path)
            .intersect(boot_ceiling(path));
        if g != CapSet::EMPTY {
            ok = false;
            log::error!("[selftest] boot-ceiling: sandbox {} granted {:?}", path, g);
        }
    }

    // Network rights (annex S2-N §5): each boot-started sender keeps exactly its
    // grant after request ∩ ceiling, and the shell, the net service itself and
    // silo receive none.
    let net_cases: [(&str, (bool, bool, bool, bool)); 6] = [
        ("/bin/sshd", (false, false, true, false)),
        ("/bin/net-broker", (true, true, true, false)),
        ("/bin/hypervisor", (false, false, false, true)),
        ("/bin/cellbox", (false, false, false, false)),
        ("/bin/net", (false, false, false, false)),
        ("/bin/silo", (false, false, false, false)),
    ];
    for (path, want) in net_cases {
        let g = CapSet::EMPTY
            .with_path_caps(path)
            .intersect(boot_ceiling(path));
        if (g.net_resolve, g.net_connect, g.net_listen, g.net_l2) != want {
            ok = false;
            log::error!(
                "[selftest] boot-ceiling: {} network rights {:?}, expected {:?}",
                path,
                (g.net_resolve, g.net_connect, g.net_listen, g.net_l2),
                want
            );
        }
    }

    // Q (Phase 01 CP1, D-Q5): the boot shell keeps `config_write` after
    // request ∩ ceiling; no other boot-started Cell receives it.
    for (path, want) in [
        ("/bin/cellbox", true),
        ("/bin/config", false),
        ("/bin/init", false),
        ("/bin/terminal", false),
        ("/bin/desktop", false),
        ("/bin/net", false),
        ("/bin/sshd", false),
    ] {
        let g = CapSet::EMPTY
            .with_path_caps(path)
            .intersect(boot_ceiling(path));
        if g.config_write != want {
            ok = false;
            log::error!(
                "[selftest] boot-ceiling: {} config_write {}, expected {}",
                path,
                g.config_write,
                want
            );
        }
    }

    if authorize(SHELL, LaunchRoute::Mem, "/mem/demo").is_some() {
        ok = false;
        log::error!("[selftest] launch-profile: shell mem launch must fail closed");
    }
    // /bin/httpd's reviewed edge carries `net_listen`, so the kernel-resolved path
    // route keeps it and the caller-owned ELF route must refuse it: arbitrary
    // bytes labelled /bin/httpd must not borrow a network right.
    match authorize(SHELL, LaunchRoute::Path, "/bin/httpd") {
        Some(profile)
            if profile.child_ceiling
                == (CapSet {
                    net_listen: true,
                    ..CapSet::LEGACY_OBSERVE_MUTATE
                }) => {}
        _ => {
            ok = false;
            log::error!("[selftest] launch-profile: shell lost exact listen-only /bin/httpd edge");
        }
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/httpd").is_some() {
        ok = false;
        log::error!(
            "[selftest] launch-profile: caller-owned ELF borrowed /bin/httpd network right"
        );
    }
    // Phase 09 D79-9: the shell's second install path of its own ELF carries exactly
    // the outbound client pair on the kernel-resolved route and nothing on the
    // caller-owned ELF route, while `/bin/cellbox` itself keeps none (above).
    match authorize(SHELL, LaunchRoute::Path, "/bin/cellbox-ssh") {
        Some(profile)
            if profile.child_ceiling
                == (CapSet {
                    net_resolve: true,
                    net_connect: true,
                    ..CapSet::LEGACY_OBSERVE_MUTATE
                }) => {}
        _ => {
            ok = false;
            log::error!("[selftest] launch-profile: shell lost exact client-only /bin/cellbox-ssh edge");
        }
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/cellbox-ssh").is_some() {
        ok = false;
        log::error!("[selftest] launch-profile: caller-owned ELF borrowed /bin/cellbox-ssh network right");
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/vfs-test").is_none() {
        ok = false;
        log::error!("[selftest] launch-profile: shell lost capability-free ELF edge");
    }
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        match authorize(SHELL, route, "/bin/hotswap") {
            Some(profile) if profile.child_ceiling == CapSet::LEGACY_OBSERVE_MUTATE => {}
            Some(_) => {
                ok = false;
                log::error!(
                    "[selftest] launch-profile: /bin/hotswap gained ambient authority on {:?}",
                    route
                );
            }
            None => {
                ok = false;
                log::error!(
                    "[selftest] launch-profile: shell lost exact /bin/hotswap edge on {:?}",
                    route
                );
            }
        }
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/vfs").is_some() {
        ok = false;
        log::error!("[selftest] launch-profile: shell gained privileged /bin/vfs launch edge");
    }

    if ok {
        log::info!("[selftest] boot-ceiling: PASS (per-path table, no union collapse)");
    } else {
        log::error!("[selftest] boot-ceiling: FAIL");
    }
    ok
}
