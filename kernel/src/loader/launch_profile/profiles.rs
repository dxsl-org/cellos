use crate::resource_registry::{DEV_GPIO, DEV_UART};
use crate::task::cap::CapSet;

use super::super::boot_ceiling;
use super::targets::reviewed_user_target_ceiling;
use super::{LaunchProfile, LaunchRoute};

const CONSOLE_MMIO: u8 = DEV_GPIO | DEV_UART;
const GPIO_ONLY_MMIO: u8 = DEV_GPIO;

pub(super) fn init_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if matches!(route, LaunchRoute::Mem | LaunchRoute::Pinned) {
        return None;
    }
    match target {
        "/bin/block" | "/bin/compositor" | "/bin/config" | "/bin/e1000" | "/bin/fb-console"
        | "/bin/hypervisor" | "/bin/input" | "/bin/net" | "/bin/net-broker" | "/bin/nvme"
        | "/bin/shell" | "/bin/silo" | "/bin/silo-test" | "/bin/srv-test" | "/bin/supervisor"
        | "/bin/vfs" | "/bin/vfs-test" | "/bin/virtio-gpu" | "/bin/virtio-net" => Some(
            LaunchProfile::new(boot_ceiling::boot_ceiling(target), "init-launch-edge", true),
        ),
        _ => None,
    }
}

pub(super) fn shell_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    let ceiling = match route {
        LaunchRoute::Path | LaunchRoute::Elf => reviewed_user_target_ceiling(target)?,
        LaunchRoute::Mem | LaunchRoute::Pinned => return None,
    };
    Some(LaunchProfile::new(ceiling, "shell-launch-edge", false))
}

pub(super) fn hypha_profile(route: LaunchRoute, target: &str) -> Option<LaunchProfile> {
    if !matches!(route, LaunchRoute::Path | LaunchRoute::Elf) {
        return None;
    }
    let ceiling = match target {
        "/bin/llm-gateway" => CapSet {
            network: true,
            ..CapSet::EMPTY
        },
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
