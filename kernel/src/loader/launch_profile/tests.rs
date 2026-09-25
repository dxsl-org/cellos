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
fn shell_reviewed_capability_free_edges_remain_empty() {
    for (route, target) in [
        (LaunchRoute::Path, "/bin/httpd"),
        (LaunchRoute::Elf, "/bin/httpd"),
        // Phase 02 ships this edge as part of the `cpp-freestanding` profile; a
        // silent drop here is what made its runner fail.
        (LaunchRoute::Path, "/bin/cpp-smoke"),
        (LaunchRoute::Elf, "/bin/cpp-smoke"),
        (LaunchRoute::Elf, "/bin/vfs-test"),
        (LaunchRoute::Path, "/bin/free"),
        (LaunchRoute::Elf, "/bin/free"),
        (LaunchRoute::Elf, "/bin/viui-demo"),
        (LaunchRoute::Path, "/bin/hotswap"),
        (LaunchRoute::Elf, "/bin/hotswap"),
    ] {
        assert_eq!(
            shell_edge(route, target).child_ceiling,
            CapSet::EMPTY,
            "{target} must remain capability-free on {route:?}"
        );
    }
    assert!(
        authorize(caller("shell", false, false), LaunchRoute::Elf, "/bin/vfs").is_none(),
        "shell must not launch privileged services by path forgery"
    );
}

#[test]
fn pipe_test_has_only_its_capability_free_peer_edge() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert_eq!(
            authorize(caller("pipe-test", false, false), route, "/bin/pipe-peer")
                .expect("pipe witness peer edge must exist")
                .child_ceiling,
            CapSet::EMPTY
        );
    }
    assert!(
        authorize(
            caller("pipe-test", false, false),
            LaunchRoute::Elf,
            "/bin/anything-else"
        )
        .is_none(),
        "the pipe witness must not gain an ambient launcher edge"
    );
}

#[test]
fn capacity_probe_path_grants_only_spawn_and_elf_is_denied() {
    assert_eq!(
        shell_edge(LaunchRoute::Path, "/bin/capacity-probe").child_ceiling,
        CapSet {
            spawn: true,
            ..CapSet::EMPTY
        },
        "the denial probe needs only its declared SpawnPinned authority"
    );
    assert!(
        authorize(
            caller("shell", false, false),
            LaunchRoute::Elf,
            "/bin/capacity-probe"
        )
        .is_none(),
        "caller-owned ELF bytes must not receive the denial probe's spawn authority"
    );
}

#[test]
fn c_pthread_witness_has_no_process_lifecycle_authority() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert_eq!(
            shell_edge(route, "/bin/c-pthread").child_ceiling,
            CapSet::EMPTY,
            "the pthread witness only creates in-cell tasks"
        );
    }
}

#[test]
fn c_spawn_adapter_has_only_its_capability_free_child_edge() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert_eq!(
            authorize(caller("c-spawn", false, false), route, "/bin/c-spawn-child")
                .expect("the C spawn witness child edge must exist")
                .child_ceiling,
            CapSet::EMPTY,
            "the launched child must hold no authority of its own on {route:?}"
        );
    }
    for target in [
        "/bin/pipe-peer",
        "/bin/init",
        "/bin/c-spawn",
        "/bin/c-spawn-child/../init",
    ] {
        assert!(
            authorize(caller("c-spawn", false, false), LaunchRoute::Path, target).is_none(),
            "the C spawn adapter must not gain an ambient launcher edge to {target}"
        );
    }
}

#[test]
fn c_spawn_child_cannot_relaunch_anything() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert!(
            authorize(
                caller("c-spawn-child", false, false),
                route,
                "/bin/c-spawn-child"
            )
            .is_none(),
            "the granted child must not hold a launch edge of its own on {route:?}"
        );
    }
}

#[test]
fn hardware_bus_demo_edges_are_class_scoped() {
    let sensor = shell_edge(LaunchRoute::Path, "/bin/sensor-demo");
    let spi = shell_edge(LaunchRoute::Path, "/bin/spi-demo");
    assert_eq!(
        sensor.child_ceiling.mmio_devices,
        crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_I2C
    );
    assert_eq!(
        spi.child_ceiling.mmio_devices,
        crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_SPI
    );
    assert_eq!(
        sensor.child_ceiling.mmio_devices & crate::resource_registry::DEV_SPI,
        0
    );
    assert_eq!(
        spi.child_ceiling.mmio_devices & crate::resource_registry::DEV_I2C,
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
            .child_ceiling,
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
        profile.child_ceiling,
        boot_ceiling::boot_ceiling("/bin/vfs")
    );
    let kms = authorize(caller("init", true, false), LaunchRoute::Path, "/bin/kms")
        .expect("init kms edge exists");
    assert_eq!(kms.child_ceiling, CapSet::EMPTY);
}

#[test]
fn init_can_launch_bcm_display_with_its_boot_ceiling() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        let profile = authorize(caller("init", true, false), route, "/bin/bcm-display")
            .expect("RPi3 init must be authorized to launch the BCM display driver");
        assert_eq!(
            profile.child_ceiling,
            boot_ceiling::boot_ceiling("/bin/bcm-display")
        );
        assert_eq!(
            profile.child_ceiling.mmio_devices,
            crate::resource_registry::DEV_DISPLAY
        );
        assert!(!profile.child_ceiling.pcie_driver);
    }
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

#[test]
fn backend_supervisor_edge_is_exact_and_lifecycle_gated() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        let profile = authorize(
            caller("backend-supervisor", true, false),
            route,
            "/bin/backend-worker",
        )
        .expect("the B0 supervisor must be able to launch its worker");
        assert_eq!(
            profile.child_ceiling,
            CapSet::EMPTY,
            "the worker is capability-free on {route:?}"
        );
        assert!(
            profile.requires_lifecycle_authority,
            "the edge exists so a SpawnCap holder can watch and restart the child"
        );
    }

    assert!(
        authorize(
            caller("backend-supervisor", false, false),
            LaunchRoute::Path,
            "/bin/backend-worker"
        )
        .is_none(),
        "a task named backend-supervisor without SpawnCap must not borrow the edge"
    );
    assert!(
        authorize(
            caller("backend-supervisor", true, false),
            LaunchRoute::Path,
            "/bin/vfs"
        )
        .is_none(),
        "the edge is exact: the worker path only"
    );
    assert!(
        authorize(
            caller("backend-supervisor", true, false),
            LaunchRoute::Mem,
            "/bin/backend-worker"
        )
        .is_none(),
        "no memory launch edge"
    );
}

#[test]
fn shell_reaches_the_backend_cells_with_the_intended_ceilings() {
    assert!(
        shell_edge(LaunchRoute::Path, "/bin/backend-supervisor")
            .child_ceiling
            .spawn,
        "the shell edge must not strip the supervisor's SpawnCap"
    );
    assert_eq!(
        shell_edge(LaunchRoute::Path, "/bin/backend-worker").child_ceiling,
        CapSet::EMPTY,
        "the worker stays capability-free when launched by hand"
    );
}
