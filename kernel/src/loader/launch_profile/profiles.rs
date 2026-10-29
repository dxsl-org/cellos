use crate::resource_registry::{DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};
use crate::task::cap::CapSet;

use super::super::boot_ceiling;
use super::targets::{reviewed_user_target_ceiling, NET_CLIENT};
use super::{LaunchProfile, LaunchRoute};

const CONSOLE_MMIO: u8 = DEV_GPIO | DEV_UART;
const GPIO_ONLY_MMIO: u8 = DEV_GPIO;
const SENSOR_MMIO: u8 = DEV_GPIO | DEV_I2C;
const SPI_DEMO_MMIO: u8 = DEV_GPIO | DEV_SPI;
/// The pre-annex X/M/W rights every reviewed non-sandbox edge keeps (annex
/// S2-T/S2-W §3, D-TW1); "capability-free" below means nothing beyond them.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;

pub(super) fn init_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    #[cfg(feature = "phase04-driver-profile")]
    if target == "/bin/phase04-terminal-driver" {
        return phase04_driver_init_profile(route, target);
    }
    if matches!(route, LaunchRoute::Mem | LaunchRoute::Pinned) {
        return None;
    }
    match target {
        "/bin/ai" | "/bin/ahci" | "/bin/xhci" | "/bin/bcm-display" | "/bin/block" | "/bin/compositor" | "/bin/config"
        | "/bin/dwc2-usb" | "/bin/e1000" | "/bin/igb" | "/bin/fb-console" | "/bin/hypervisor" | "/bin/input"
        | "/bin/kms" | "/bin/net" | "/bin/net-broker" | "/bin/nvme" | "/bin/serial" | "/bin/cellbox"
        | "/bin/silo" | "/bin/ai-test" | "/bin/silo-test" | "/bin/srv-test" | "/bin/supervisor"
        | "/bin/vfs" | "/bin/vfs-test" | "/bin/virtio-gpu" | "/bin/virtio-net"
        | "/bin/std-smoke" | "/bin/desktop" | "/bin/sshd" | "/bin/chrome" | "/bin/vm-runner" | "/bin/files" | "/bin/terminal"
        // Ocel's demand-only Tier 2 JavaScript engines. Init activates the exact
        // fixed target on a document lease, not through the boot service plan.
        // The empty boot ceiling grants no engine authority; domain admission
        // independently decides whether the artifact may run in a private root.
        | "/bin/ocel-js"
        | "/bin/ocel-quickjs"
        // Isolated native PDF rasterizer; no additional capabilities.
        | "/bin/ocel-pdf"
        // Phase-02 Tier-2 entry fixtures. Both carry `PROTECTION_CLASS_UNTRUSTED`
        // manifests and `boot_ceiling` already gives them `CapSet::EMPTY`, so this
        // row grants init nothing beyond the ability to launch them: the
        // admission policy is still the only thing that decides whether they may
        // run as domains, and a production image (which has no such cells and no
        // qualified switch ordering) refuses them there.
        | "/bin/tier2-smoke" | "/bin/tier2-exploit" | "/bin/cpp-smoke"
        // Phase-02 slice B cross-tier exchange: the Tier-2 provider and the Tier-1
        // driver. Both ceilings are `CapSet::EMPTY` (neither spawns anything), so
        // this edge grants init nothing but the ability to launch them, and the
        // admission policy still decides whether the provider may run as a domain.
        | "/bin/tier2-rpc-provider" | "/bin/tier2-rpc-driver" => Some(LaunchProfile::new(
            boot_ceiling::boot_ceiling(target),
            "init-launch-edge",
            true,
        )),
        // Autoload may select applications already reviewed for interactive
        // launch. Reuse their exact ceiling, never init's union of authority;
        // unknown paths still fail closed and lifecycle authority is required.
        _ => reviewed_user_target_ceiling(target)
            .map(|ceiling| LaunchProfile::new(ceiling, "init-autoload-edge", true)),
    }
}

pub(super) fn cellbox_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    let ceiling = match route {
        LaunchRoute::Path | LaunchRoute::Elf if target == "/bin/hotswap" => LEGACY,
        // Retained power-off worker: the only caller of raw opcode 502 (see
        // `RAW_SHUTDOWN_CALLERS` in the syscall layer). Path route only; the
        // child holds no capability, the syscall gate is name-keyed.
        LaunchRoute::Path if target == "/bin/shutdown" => LEGACY,
        // Phase 09 D79-9: the cellbox ELF installed a second time for `cellbox ssh`.
        // Only the shell may start it, only by the kernel-resolved path, and only it
        // carries the outbound client rights; `/bin/cellbox` itself holds none. It is
        // not in the generic reviewed table, so init autoload and the desktop cannot
        // launch it, and its caller-owned ELF route stays denied below.
        LaunchRoute::Path if target == "/bin/cellbox-ssh" => NET_CLIENT,
        LaunchRoute::Path | LaunchRoute::Elf => reviewed_user_target_ceiling(target)?,
        LaunchRoute::Mem | LaunchRoute::Pinned => return None,
    };
    Some(LaunchProfile::new(ceiling, "cellbox-launch-edge", false))
}
pub(super) fn desktop_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    let ceiling = match route {
        LaunchRoute::Path | LaunchRoute::Elf => reviewed_user_target_ceiling(target)?,
        LaunchRoute::Mem | LaunchRoute::Pinned => return None,
    };
    Some(LaunchProfile::new(ceiling, "desktop-launch-edge", false))
}

pub(super) fn hypha_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) {
        return None;
    }
    let ceiling = match target {
        // The gateway's reviewed ceiling carries its network-client rights; the
        // edge reuses that row rather than restating it.
        "/bin/llm-gateway" => reviewed_user_target_ceiling(target)?,
        "/bin/tool-fs" | "/bin/tool-sys" => LEGACY,
        "/bin/tool-spawn" => CapSet {
            spawn: true,
            ..LEGACY
        },
        _ => return None,
    };
    Some(LaunchProfile::new(ceiling, "hypha-launch-edge", false))
}

pub(super) fn tool_spawn_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    let ceiling = match route {
        LaunchRoute::Path | LaunchRoute::Elf => reviewed_user_target_ceiling(target)?,
        LaunchRoute::Mem | LaunchRoute::Pinned => return None,
    };
    Some(LaunchProfile::new(ceiling, "tool-spawn-launch-edge", false))
}

/// The DWC2 transport cell may create only its capability-free LAN front-end.
///
/// The narrow launch edge prevents a compromised USB device from converting the
/// host cell's lifecycle authority into arbitrary process creation.
pub(super) fn dwc2_function_worker_profile(
    route: LaunchRoute,
    target: &str,
) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || target != "/bin/lan9514" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "dwc2-function-worker-edge",
        true,
    ))
}

/// The pipe witness needs one exact, capability-free child edge. Keeping it
/// separate from the general shell and tool-spawn profiles proves that a Tier 2
/// cell can grant a pipe endpoint across domains without acquiring ambient
/// lifecycle authority.
pub(super) fn pipe_test_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || target != "/bin/pipe-peer" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "pipe-test-peer-edge",
        false,
    ))
}

/// GUI Terminal (`terminal`) may run exactly one program: the multicall shell
/// `/bin/cellbox`, on the kernel-resolved path route. The child ceiling is
/// [`SHELL_EDGE`]: the pre-annex X/M/W rights (`LEGACY`) plus the Q Config-write
/// right (Phase 01 CP1, D-Q5), and no other capability. The shell it hosts is
/// the real `cellbox sh` over a session context.
pub(super) fn terminal_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if route != LaunchRoute::Path || target != "/bin/cellbox" {
        return None;
    }
    Some(LaunchProfile::new(SHELL_EDGE, "terminal-shell-edge", false))
}

/// The child ceiling of every reviewed edge that hosts the interactive shell
/// `/bin/cellbox`: `LEGACY` plus `config_write`, which service-config bounds to
/// the shell key namespace (Phase 01 CP1, D-Q5). Must equal the init boot row
/// `boot_ceiling("/bin/cellbox")`.
pub(super) const SHELL_EDGE: CapSet = CapSet {
    config_write: true,
    ..LEGACY
};

/// Private Phase04 fixture driver: shell hosting (the same ceiling as the GUI
/// Terminal's shell edge, so the lane proves the production Q grant) plus the
/// stock echo negative-output-grant probe on a capability-free edge. Neither
/// edge exists in ordinary images.
#[cfg(feature = "phase04-driver-profile")]
pub(super) fn phase04_driver_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if route != LaunchRoute::Path {
        return None;
    }
    let (ceiling, label) = match target {
        "/bin/cellbox" => (SHELL_EDGE, "phase04-driver-shell-edge"),
        "/bin/echo" => (LEGACY, "phase04-driver-output-probe-edge"),
        _ => return None,
    };
    Some(LaunchProfile::new(ceiling, label, false))
}

/// Init may start the private Phase04 driver from the generated autoload plan. Init
/// tries the caller-supplied-ELF route first and falls back to the path route, like
/// every other autoload entry; the child ceiling is empty either way and lifecycle
/// authority is required like every init autoload edge. Compiled only under
/// `phase04-driver-profile`.
#[cfg(feature = "phase04-driver-profile")]
pub(super) fn phase04_driver_init_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || target != "/bin/phase04-terminal-driver" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "phase04-driver-init-edge",
        true,
    ))
}

#[cfg(feature = "phase02-terminal-profile")]
pub(super) fn phase02_session_profile(
    caller: &str,
    route: LaunchRoute,
    target: &str,
) -> Option<LaunchProfile> {
    let approved = match caller {
        "cellbox" => target == "/bin/phase02-session-parent",
        "phase02-session-parent" | "phase02-session-child" => {
            target == "/bin/phase02-session-child"
        }
        _ => false,
    };
    if !approved || route != LaunchRoute::Path {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "phase02-session-fixture-edge",
        false,
    ))
}

/// Private Phase03 stdin-filter fixture: exactly one capability-free
/// `cellbox -> /bin/p03-filter` edge on the kernel-resolved path route. Compiled
/// only under `phase03-filter-profile`; no ordinary image carries this row.
#[cfg(feature = "phase03-filter-profile")]
pub(super) fn phase03_filter_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if route != LaunchRoute::Path || target != "/bin/p03-filter" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "phase03-filter-fixture-edge",
        false,
    ))
}

/// The C spawn adapter needs one exact, capability-free child edge. Both routes
/// are allowed because they resolve the same reviewed row: the kernel-resolved
/// path form is the one a board with a kernel block device can use, and the
/// caller-supplied-ELF form is the only one that reaches the VFS-served cell
/// store post-boot, where the kernel deliberately drives no block hardware. The
/// child holds no authority of its own, so a compromised child cannot escalate
/// through the edge it was started with.
pub(super) fn c_spawn_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || target != "/bin/c-spawn-child" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "c-spawn-child-edge",
        false,
    ))
}

/// The B0 supervisor library needs exactly one exact child edge: its worker.
///
/// Lifecycle authority is required because this edge exists so a supervisor can
/// spawn *and* watch/restart children (`NotifyOnExit`/`ForceExit` are `SpawnCap`
/// gated); the child itself is capability-free, so a compromised worker cannot
/// escalate through the edge it was started with. Keeping the row exact means a
/// supervisor cannot convert its authority into arbitrary process creation.
pub(super) fn backend_supervisor_profile(
    route: LaunchRoute,
    target: &str,
) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || target != "/bin/backend-worker" {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "backend-supervisor-worker-edge",
        true,
    ))
}

/// The renderer may launch only its capability-free trusted or domain worker.
/// Lifecycle authority permits synchronous teardown of either worker class.
pub(super) fn render_worker_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf)
        || !matches!(target, "/bin/c2c-render-worker" | "/bin/c2c-render-domain-worker")
    {
        return None;
    }
    Some(LaunchProfile::new(
        LEGACY,
        "c2c-render-worker-edge",
        true,
    ))
}

pub(super) fn supervisor_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) || !target.starts_with("/bin/") {
        return None;
    }
    let ceiling = boot_ceiling::lookup(target).or_else(|| reviewed_user_target_ceiling(target))?;
    Some(LaunchProfile::new(ceiling, "supervisor-hotswap-edge", true))
}

/// Compatibility edge: `periph-demo` uses `SpawnPinned("/bin/periph-demo", ..)`
/// as a self-relaunch path despite manifest `spawn = false`. Preserve only that
/// exact pinned edge, bounded to the reviewed console MMIO ceiling, so the demo
/// keeps working without reviving ambient lifecycle authority.
pub(super) fn pinned_profile(
    caller_name: &str,
    route: LaunchRoute,
    target: &str,
) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Pinned) {
        return None;
    }
    let ceiling = match (caller_name, target) {
        ("bench", "/bin/bench-probe") | ("capacity-probe", "/bin/bench-probe") => LEGACY,
        ("capacity-probe", "/bin/heavy-probe") => LEGACY,
        ("periph-demo", "/bin/periph-demo") => CapSet {
            mmio_devices: CONSOLE_MMIO,
            ..LEGACY
        },
        _ => return None,
    };
    Some(LaunchProfile::new(ceiling, "pinned-launch-edge", false))
}

pub(super) const fn console_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: CONSOLE_MMIO,
        ..LEGACY
    }
}

pub(super) const fn gpio_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: GPIO_ONLY_MMIO,
        ..LEGACY
    }
}

pub(super) const fn sensor_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: SENSOR_MMIO,
        ..LEGACY
    }
}

pub(super) const fn spi_demo_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: SPI_DEMO_MMIO,
        ..LEGACY
    }
}
