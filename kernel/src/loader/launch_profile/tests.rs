use crate::loader::boot_ceiling;

use super::{authorize, CallerLaunchState, LaunchProfile, LaunchRoute};
use crate::task::cap::CapSet;

fn caller(name: &'static str, has_spawn: bool, has_supervisor: bool) -> CallerLaunchState<'static> {
    CallerLaunchState {
        name,
        has_spawn,
        has_supervisor,
    }
}

fn shell_edge(route: LaunchRoute, target: &str) -> LaunchProfile {
    authorize(caller("shell", false, false), route, target).expect("shell launch edge must exist")
}

#[test]
fn shell_has_no_mem_launch_edge() {
    assert!(
        authorize(caller("shell", false, false), LaunchRoute::Mem, "/mem/demo").is_none(),
        "shell mem launches must fail closed"
    );
}

#[test]
fn shell_service_ipc_tool_edge_is_capability_free() {
    assert_eq!(
        shell_edge(LaunchRoute::Path, "/bin/httpd").parent_ceiling,
        CapSet::EMPTY
    );
    assert_eq!(
        shell_edge(LaunchRoute::Elf, "/bin/httpd").parent_ceiling,
        CapSet::EMPTY,
        "service-IPC tools must not carry ambient network authority"
    );
    assert_eq!(
        shell_edge(LaunchRoute::Elf, "/bin/vfs-test").parent_ceiling,
        CapSet::EMPTY,
        "capability-free ELF targets remain launchable"
    );
    assert_eq!(
        shell_edge(LaunchRoute::Path, "/bin/hotswap").parent_ceiling,
        CapSet::EMPTY,
        "the hotswap CLI must not inherit lifecycle or service authority"
    );
    assert_eq!(
        shell_edge(LaunchRoute::Elf, "/bin/hotswap").parent_ceiling,
        CapSet::EMPTY,
        "the VFS-loaded hotswap CLI must remain capability-free"
    );
    assert!(
        authorize(caller("shell", false, false), LaunchRoute::Elf, "/bin/vfs").is_none(),
        "shell must not launch privileged services by path forgery"
    );
}

#[test]
fn hardware_bus_demo_edges_are_class_scoped() {
    let sensor = shell_edge(LaunchRoute::Path, "/bin/sensor-demo");
    let spi = shell_edge(LaunchRoute::Path, "/bin/spi-demo");
    assert_eq!(
        sensor.parent_ceiling.mmio_devices,
        crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_I2C
    );
    assert_eq!(
        spi.parent_ceiling.mmio_devices,
        crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_SPI
    );
    assert_eq!(
        sensor.parent_ceiling.mmio_devices & crate::resource_registry::DEV_SPI,
        0
    );
    assert_eq!(
        spi.parent_ceiling.mmio_devices & crate::resource_registry::DEV_I2C,
        0
    );
}

#[test]
fn hotswap_cli_is_shell_only() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert!(
            authorize(caller("tool-spawn", true, false), route, "/bin/hotswap").is_none(),
            "tool-spawn must not reach the shell-only hotswap CLI on {route:?}"
        );
        assert!(
            authorize(caller("supervisor", true, true), route, "/bin/hotswap").is_none(),
            "supervisor must not relaunch the hotswap CLI on {route:?}"
        );
        assert!(
            authorize(caller("init", true, false), route, "/bin/hotswap").is_none(),
            "non-shell launchers must not inherit the hotswap CLI edge on {route:?}"
        );
    }
}

#[test]
fn capability_bearing_elf_route_requires_lifecycle_authority() {
    assert_eq!(
        authorize(caller("init", true, false), LaunchRoute::Elf, "/bin/vfs")
            .expect("init lifecycle authority keeps the boot-service edge")
            .parent_ceiling,
        boot_ceiling::boot_ceiling("/bin/vfs")
    );
    assert!(
        authorize(
            caller("tool-spawn", true, false),
            LaunchRoute::Elf,
            "/bin/tool-spawn"
        )
        .is_none(),
        "caller-owned ELF bytes must not inherit spawn authority"
    );
}

#[test]
fn init_edge_reuses_boot_ceiling_for_boot_services() {
    let profile = authorize(caller("init", true, false), LaunchRoute::Path, "/bin/vfs")
        .expect("init vfs edge exists");
    assert_eq!(
        profile.parent_ceiling,
        boot_ceiling::boot_ceiling("/bin/vfs")
    );
}

#[test]
fn hypha_fixed_edges_remain_exact() {
    let hypha = caller("hypha", true, false);
    assert!(authorize(hypha, LaunchRoute::Path, "/bin/llm-gateway").is_some());
    assert!(authorize(hypha, LaunchRoute::Path, "/bin/tool-fs").is_some());
    assert!(authorize(hypha, LaunchRoute::Path, "/bin/tool-sys").is_some());
    assert!(authorize(hypha, LaunchRoute::Path, "/bin/tool-spawn").is_some());
    assert!(authorize(hypha, LaunchRoute::Path, "/bin/vfs").is_none());
}

#[test]
fn supervisor_requires_supervisor_cap_for_hotswap_route() {
    assert!(
        authorize(
            caller("supervisor", true, false),
            LaunchRoute::Path,
            "/bin/net"
        )
        .is_none(),
        "task name alone must not unlock supervisor hotswap"
    );
    assert!(authorize(
        caller("supervisor", true, true),
        LaunchRoute::Path,
        "/bin/net"
    )
    .is_some());
}
