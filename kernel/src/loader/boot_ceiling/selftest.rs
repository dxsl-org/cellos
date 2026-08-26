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
    name: "shell",
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
        ..CapSet::EMPTY
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
            "/bin/shell holds network",
            !boot_ceiling("/bin/shell").network,
        ),
        ("/bin/shell holds spawn", !boot_ceiling("/bin/shell").spawn),
        (
            "/bin/shell holds block_regions",
            boot_ceiling("/bin/shell").block_regions == 0,
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
    let privileged: [PrivCapCase; 8] = [
        ("/bin/platform", |c| c.platform),
        ("/bin/supervisor", |c| c.supervisor),
        ("/bin/block", |c| c.pcie_driver),
        ("/bin/nvme", |c| c.pcie_driver),
        ("/bin/e1000", |c| c.pcie_driver),
        ("/bin/virtio-net", |c| c.pcie_driver),
        ("/bin/virtio-gpu", |c| c.pcie_driver),
        ("/bin/input", |c| c.pcie_driver),
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

    // Negative direction: the USB driver cells hold NO authority until policy v3
    // adds a signed USB host byte. `with_path_caps` mints nothing for them and
    // their ceiling rows are EMPTY; if either side starts granting `pcie_driver`
    // or `DEV_DISPLAY`, this fails the power-on self-test.
    for path in ["/bin/dwc2-usb", "/bin/lan9514"] {
        let requested = CapSet::EMPTY.with_path_caps(path);
        let granted = requested.intersect(boot_ceiling(path));
        if granted != CapSet::EMPTY {
            ok = false;
            log::error!(
                "[selftest] boot-ceiling: {} granted {:?} without USB policy v3 — must stay EMPTY",
                path,
                granted
            );
        }
    }

    if authorize(SHELL, LaunchRoute::Mem, "/mem/demo").is_some() {
        ok = false;
        log::error!("[selftest] launch-profile: shell mem launch must fail closed");
    }
    if authorize(SHELL, LaunchRoute::Path, "/bin/httpd").is_none() {
        ok = false;
        log::error!("[selftest] launch-profile: shell lost exact /bin/httpd launch edge");
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/httpd").is_none() {
        ok = false;
        log::error!("[selftest] launch-profile: shell lost exact /bin/httpd ELF edge");
    }
    if authorize(SHELL, LaunchRoute::Elf, "/bin/vfs-test").is_none() {
        ok = false;
        log::error!("[selftest] launch-profile: shell lost capability-free ELF edge");
    }
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        match authorize(SHELL, route, "/bin/hotswap") {
            Some(profile) if profile.child_ceiling == CapSet::EMPTY => {}
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
