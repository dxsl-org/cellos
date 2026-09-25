use crate::resource_registry::{DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};
use crate::task::cap::CapSet;

use super::super::boot_ceiling;
use super::targets::reviewed_user_target_ceiling;
use super::{LaunchProfile, LaunchRoute};

const CONSOLE_MMIO: u8 = DEV_GPIO | DEV_UART;
const GPIO_ONLY_MMIO: u8 = DEV_GPIO;
const SENSOR_MMIO: u8 = DEV_GPIO | DEV_I2C;
const SPI_DEMO_MMIO: u8 = DEV_GPIO | DEV_SPI;

pub(super) fn init_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if matches!(route, LaunchRoute::Mem | LaunchRoute::Pinned) {
        return None;
    }
    match target {
        "/bin/ai" | "/bin/bcm-display" | "/bin/block" | "/bin/compositor" | "/bin/config"
        | "/bin/dwc2-usb" | "/bin/e1000" | "/bin/fb-console" | "/bin/hypervisor" | "/bin/input"
        | "/bin/kms" | "/bin/net" | "/bin/net-broker" | "/bin/nvme" | "/bin/shell"
        | "/bin/silo" | "/bin/ai-test" | "/bin/silo-test" | "/bin/srv-test" | "/bin/supervisor"
        | "/bin/vfs" | "/bin/vfs-test" | "/bin/virtio-gpu" | "/bin/virtio-net"
        | "/bin/std-smoke" | "/bin/desktop" => Some(LaunchProfile::new(
            boot_ceiling::boot_ceiling(target),
            "init-launch-edge",
            true,
        )),
        _ => None,
    }
}

pub(super) fn shell_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    let ceiling = match route {
        LaunchRoute::Path | LaunchRoute::Elf if target == "/bin/hotswap" => CapSet::EMPTY,
        LaunchRoute::Path | LaunchRoute::Elf => reviewed_user_target_ceiling(target)?,
        LaunchRoute::Mem | LaunchRoute::Pinned => return None,
    };
    Some(LaunchProfile::new(ceiling, "shell-launch-edge", false))
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
        "/bin/llm-gateway" => CapSet::EMPTY,
        "/bin/tool-fs" | "/bin/tool-sys" => CapSet::EMPTY,
        "/bin/tool-spawn" => CapSet {
            spawn: true,
            ..CapSet::EMPTY
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
        CapSet::EMPTY,
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
        CapSet::EMPTY,
        "pipe-test-peer-edge",
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
        CapSet::EMPTY,
        "c-spawn-child-edge",
        false,
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
        ("bench", "/bin/bench-probe") | ("capacity-probe", "/bin/bench-probe") => CapSet::EMPTY,
        ("periph-demo", "/bin/periph-demo") => CapSet {
            mmio_devices: CONSOLE_MMIO,
            ..CapSet::EMPTY
        },
        _ => return None,
    };
    Some(LaunchProfile::new(ceiling, "pinned-launch-edge", false))
}

pub(super) const fn console_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: CONSOLE_MMIO,
        ..CapSet::EMPTY
    }
}

pub(super) const fn gpio_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: GPIO_ONLY_MMIO,
        ..CapSet::EMPTY
    }
}

pub(super) const fn sensor_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: SENSOR_MMIO,
        ..CapSet::EMPTY
    }
}

pub(super) const fn spi_demo_mmio_capset() -> CapSet {
    CapSet {
        mmio_devices: SPI_DEMO_MMIO,
        ..CapSet::EMPTY
    }
}
