use crate::loader::boot_ceiling;

use super::{authorize, CallerLaunchState, LaunchProfile, LaunchRoute};
use crate::task::cap::CapSet;

/// Annex S2-T/S2-W D-TW1: every reviewed non-sandbox edge keeps X/M/W, so a
/// "capability-free" edge is exactly this; only the sandbox arm is EMPTY.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;

fn caller(name: &'static str, has_spawn: bool, has_supervisor: bool) -> CallerLaunchState<'static> {
    CallerLaunchState {
        name,
        has_spawn,
        has_supervisor,
    }
}

fn shell_edge(route: LaunchRoute, target: &str) -> LaunchProfile {
    authorize(caller("cellbox", false, false), route, target).expect("cellbox launch edge must exist")
}

#[test]
fn pdf_engine_edge_cannot_grant_authority_or_boot_without_spawn() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        let profile = authorize(caller("init", true, false), route, "/bin/ocel-pdf")
            .expect("init may launch the isolated PDF engine");
        assert_eq!(profile.child_ceiling, LEGACY);
        assert!(profile.requires_lifecycle_authority);
        assert!(authorize(caller("init", false, false), route, "/bin/ocel-pdf").is_none());
        assert!(authorize(caller("ocel", false, false), route, "/bin/ocel-pdf").is_none());
        assert!(authorize(caller("ocel-pdf", true, false), route, "/bin/vfs").is_none());
    }
    for route in [LaunchRoute::Mem, LaunchRoute::Pinned] {
        assert!(authorize(caller("init", true, false), route, "/bin/ocel-pdf").is_none());
    }
}

#[test]
fn shell_has_no_mem_launch_edge() {
    assert!(
        authorize(caller("cellbox", false, false), LaunchRoute::Mem, "/mem/demo").is_none(),
        "shell mem launches must fail closed"
    );
}

/// Tier 3 is started on demand, so the shell carries the launch edge for the
/// hypervisor cell — with exactly the authority the init edge gives it, and
/// nothing else. A wider ceiling here would hand a user-typed command more than
/// the cell itself is allowed to hold.
#[test]
fn shell_can_launch_the_hypervisor_cell_with_only_its_own_authority() {
    let edge = shell_edge(LaunchRoute::Path, "/bin/hypervisor");
    assert_eq!(
        edge.child_ceiling,
        CapSet {
            hypervisor: true,
            net_l2: true,
            ..LEGACY
        },
        "the shell's hypervisor edge must grant hypervisor + raw frames and nothing else"
    );
    assert_eq!(edge.child_ceiling, boot_ceiling::boot_ceiling("/bin/hypervisor"));
}

fn net_rights(c: CapSet) -> (bool, bool, bool, bool) {
    (c.net_resolve, c.net_connect, c.net_listen, c.net_l2)
}

/// Annex S2-N §5: each reviewed network worker edge carries exactly its grant,
/// the kernel-resolved path route keeps it, and the caller-owned ELF route
/// refuses it so arbitrary bytes cannot borrow a network right by path label.
#[test]
fn network_worker_edges_carry_only_their_grant() {
    let client = (true, true, false, false);
    for (target, want) in [
        ("/bin/curl", client),
        ("/bin/wget", client),
        ("/bin/mqtt", client),
        ("/bin/ocel", client),
        ("/bin/llm-gateway", client),
        ("/bin/https-demo", client),
        ("/bin/robot-demo", client),
        ("/bin/nc", (true, true, true, false)),
        ("/bin/net-broker", (true, true, true, false)),
        ("/bin/httpd", (false, false, true, false)),
    ] {
        let edge = shell_edge(LaunchRoute::Path, target);
        assert_eq!(net_rights(edge.child_ceiling), want, "{target}");
        let path_request = CapSet::EMPTY.with_path_caps(target);
        assert_eq!(
            net_rights(path_request.intersect(edge.child_ceiling)),
            want,
            "{target}: request ∩ ceiling must keep the grant"
        );
        assert!(
            authorize(caller("cellbox", false, false), LaunchRoute::Elf, target).is_none(),
            "{target}: caller-owned ELF must not borrow a network right"
        );
    }
    // The httpd edge holds nothing else.
    assert_eq!(
        shell_edge(LaunchRoute::Path, "/bin/httpd").child_ceiling,
        CapSet {
            net_listen: true,
            ..LEGACY
        }
    );
    // D-N2/D-N3: scripting runtimes and the user sandbox get no network right;
    // the sandbox (D-TW1) gets nothing at all.
    for target in ["/bin/lua", "/bin/python"] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf] {
            assert_eq!(
                shell_edge(route, target).child_ceiling,
                LEGACY,
                "{target} must hold only the legacy rights on {route:?}"
            );
        }
    }
    for target in ["/apps/demo", "/data/bin/tool", "/home/user/app"] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf] {
            assert_eq!(
                shell_edge(route, target).child_ceiling,
                CapSet::EMPTY,
                "{target} must stay capability-free on {route:?}"
            );
        }
    }
    // Init autoload reuses the same reviewed rows.
    let autoload = authorize(caller("init", true, false), LaunchRoute::Path, "/bin/curl")
        .expect("reviewed network client may be autoloaded");
    assert_eq!(net_rights(autoload.child_ceiling), client);
    // Boot-started network services keep their grant on the init edge.
    let sshd = authorize(caller("init", true, false), LaunchRoute::Path, "/bin/sshd")
        .expect("init sshd edge exists");
    assert_eq!(
        sshd.child_ceiling,
        CapSet {
            net_listen: true,
            ..LEGACY
        }
    );
}

/// Phase 09 D79-9 + KSSH K3: the second install path of the cellbox ELF is the
/// only shell edge with outbound client rights and the KMS SSH sign right. The
/// shell's own path holds none, the ELF route and every other caller are refused,
/// and nothing beyond the client pair and the sign right is granted.
#[test]
fn cellbox_ssh_edge_is_shell_only_path_only_client() {
    let edge = shell_edge(LaunchRoute::Path, "/bin/cellbox-ssh");
    assert_eq!(
        edge.child_ceiling,
        CapSet {
            net_resolve: true,
            net_connect: true,
            kms_ssh_sign: true,
            ..LEGACY
        }
    );
    assert_eq!(edge.child_ceiling, super::profiles::SSH_CLIENT_EDGE);
    let request = CapSet::EMPTY.with_path_caps("/bin/cellbox-ssh");
    assert_eq!(net_rights(request.intersect(edge.child_ceiling)), (true, true, false, false));
    assert_eq!(net_rights(CapSet::EMPTY.with_path_caps("/bin/cellbox")), (false, false, false, false));
    assert!(authorize(caller("cellbox", false, false), LaunchRoute::Elf, "/bin/cellbox-ssh").is_none());
    for name in ["init", "desktop", "cellbox-ssh", "terminal"] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf] {
            assert!(
                authorize(caller(name, true, false), route, "/bin/cellbox-ssh").is_none(),
                "{name} must not launch /bin/cellbox-ssh on {route:?}"
            );
        }
    }
}

/// KSSH K7: the admin cell's edge. Only the shell, only on the kernel-resolved path
/// route and only for `/bin/kms-ssh-admin`; the ceiling is the pre-annex X/M/W rights
/// plus the admin right (no network, no sign right, no Q), and no other caller or
/// route reaches the path.
#[test]
fn kms_ssh_admin_edge_is_shell_only_path_only_admin() {
    use super::profiles::KMS_SSH_ADMIN_EDGE;
    use api::caller_identity::CALLER_FLAG_KMS_SSH_ADMIN;

    let edge = shell_edge(LaunchRoute::Path, "/bin/kms-ssh-admin");
    assert_eq!(
        edge.child_ceiling,
        CapSet {
            kms_ssh_admin: true,
            ..LEGACY
        }
    );
    assert_eq!(edge.child_ceiling, KMS_SSH_ADMIN_EDGE);
    assert_eq!(edge.denial_label, "cellbox-launch-edge");
    assert!(!edge.requires_lifecycle_authority);
    assert_eq!(net_rights(edge.child_ceiling), (false, false, false, false));
    assert!(!edge.child_ceiling.kms_ssh_sign && !edge.child_ceiling.config_write);
    assert!(!CapSet::path_mints_ptrust("/bin/kms-ssh-admin"));
    // The grant: request ∩ ceiling is the admin right and X/M/W, and flags are admin only.
    let request = CapSet::EMPTY.with_path_caps("/bin/kms-ssh-admin");
    let granted = request.intersect(edge.child_ceiling);
    assert_eq!(granted, KMS_SSH_ADMIN_EDGE);
    assert_eq!(granted.kms_caller_flags(), CALLER_FLAG_KMS_SSH_ADMIN);
    // The two KMS rights are never on one ceiling: the ssh edge has no admin, this edge no sign.
    let ssh = shell_edge(LaunchRoute::Path, "/bin/cellbox-ssh").child_ceiling;
    assert!(!ssh.kms_ssh_admin);
    assert!(!(edge.child_ceiling.kms_ssh_sign && edge.child_ceiling.kms_ssh_admin));
    assert_eq!(
        CapSet::EMPTY.with_path_caps("/bin/cellbox-ssh").intersect(edge.child_ceiling).kms_caller_flags(),
        0,
        "the ssh request meets nothing on the admin ceiling"
    );
    // The caller-owned ELF route and the memory/pinned routes are refused; so is every
    // caller other than the shell, on any route.
    for route in [LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
        assert!(authorize(caller("cellbox", true, true), route, "/bin/kms-ssh-admin").is_none(), "{route:?}");
    }
    for name in ["init", "desktop", "terminal", "kms", "cellbox-ssh", "kms-ssh-admin", "hypha", "tool-spawn"] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
            for (spawn, supervisor) in [(false, false), (true, false), (true, true)] {
                assert!(
                    authorize(caller(name, spawn, supervisor), route, "/bin/kms-ssh-admin").is_none(),
                    "{name} must not launch /bin/kms-ssh-admin on {route:?}"
                );
            }
        }
    }
    // Near-miss and sandbox spellings of the path get no edge from the shell either.
    for path in [
        "/bin/kms-ssh-admin2",
        "/bin/kms-ssh-admi",
        "/bin/KMS-SSH-ADMIN",
        "bin/kms-ssh-admin",
        "/bin/kms-ssh-admin/",
        "/apps/kms-ssh-admin",
        "/data/bin/kms-ssh-admin",
        "/home/user/kms-ssh-admin",
    ] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf] {
            if let Some(profile) = authorize(caller("cellbox", false, false), route, path) {
                assert!(!profile.child_ceiling.kms_ssh_admin, "{path} {route:?}");
            }
        }
    }
}

#[test]
fn shell_reviewed_capability_free_edges_remain_empty() {
    for (route, target) in [
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
            LEGACY,
            "{target} must remain capability-free on {route:?}"
        );
    }
    assert!(
        authorize(caller("cellbox", false, false), LaunchRoute::Elf, "/bin/vfs").is_none(),
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
            LEGACY
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
            ..LEGACY
        },
        "the denial probe needs only its declared SpawnPinned authority"
    );
    assert!(
        authorize(
            caller("cellbox", false, false),
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
            LEGACY,
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
            LEGACY,
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

/// S2c-min: the retained power-off worker is launchable from the shell by
/// path only, capability-free, and from no other caller.
#[test]
fn shutdown_worker_edge_is_shell_path_only_and_capability_free() {
    let edge = shell_edge(LaunchRoute::Path, "/bin/shutdown");
    assert_eq!(edge.child_ceiling, LEGACY);
    assert!(!edge.requires_lifecycle_authority);
    for route in [LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
        assert!(
            authorize(caller("cellbox", false, false), route, "/bin/shutdown").is_none(),
            "shell must not reach /bin/shutdown on {route:?}"
        );
    }
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        for name in ["tool-spawn", "supervisor", "init", "desktop", "shutdown"] {
            assert!(
                authorize(caller(name, true, name == "supervisor"), route, "/bin/shutdown").is_none(),
                "{name} must not inherit the /bin/shutdown edge on {route:?}"
            );
        }
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
    assert_eq!(kms.child_ceiling, LEGACY);
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
    let gateway = authorize(hypha, LaunchRoute::Path, "/bin/llm-gateway")
        .expect("hypha llm-gateway edge exists");
    assert_eq!(net_rights(gateway.child_ceiling), (true, true, false, false));
    assert!(
        authorize(hypha, LaunchRoute::Elf, "/bin/llm-gateway").is_none(),
        "caller-owned ELF must not borrow the gateway's network right"
    );
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
            LEGACY,
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
        LEGACY,
        "the worker stays capability-free when launched by hand"
    );
}

#[test]
fn init_autoload_keeps_reviewed_application_ceiling_and_authority_gate() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        let echo = authorize(caller("init", true, false), route, "/bin/echo")
            .expect("reviewed applications may be autoloaded");
        assert_eq!(echo.child_ceiling, LEGACY);
        assert!(echo.requires_lifecycle_authority);
        let bench = authorize(caller("init", true, false), route, "/bin/bench")
            .expect("reviewed spawning application may be autoloaded");
        assert!(bench.child_ceiling.spawn);
        assert!(!bench.child_ceiling.block_io);
        assert!(!bench.child_ceiling.network);
        assert!(!bench.child_ceiling.hypervisor);
        assert!(authorize(caller("init", false, false), route, "/bin/echo").is_none());
        assert!(authorize(caller("init", true, false), route, "/bin/unreviewed-app").is_none());
    }
    for route in [LaunchRoute::Mem, LaunchRoute::Pinned] {
        assert!(authorize(caller("init", true, false), route, "/bin/echo").is_none());
    }
}

#[test]
fn renderer_cannot_launch_privileged_or_unreviewed_children() {
    let renderer = caller("c2c-render", true, false);
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        for target in ["/bin/c2c-render-worker", "/bin/c2c-render-domain-worker"] {
            let worker = authorize(renderer, route, target).unwrap();
            assert_eq!(worker.child_ceiling, LEGACY);
            assert!(worker.requires_lifecycle_authority);
            assert!(authorize(caller("c2c-render", false, false), route, target).is_none());
        }
        for target in ["/bin/vfs", "/bin/net", "/bin/c2c-render", "/bin/unreviewed"] {
            assert!(authorize(renderer, route, target).is_none());
        }
        for worker in ["c2c-render-worker", "c2c-render-domain-worker"] {
            for target in ["/bin/c2c-render-worker", "/bin/c2c-render-domain-worker"] {
                assert!(authorize(caller(worker, true, false), route, target).is_none());
            }
        }
    }
    for route in [LaunchRoute::Mem, LaunchRoute::Pinned] {
        for target in ["/bin/c2c-render-worker", "/bin/c2c-render-domain-worker"] {
            assert!(authorize(renderer, route, target).is_none());
        }
    }
    assert!(shell_edge(LaunchRoute::Path, "/bin/c2c-render").child_ceiling.spawn);
    assert!(authorize(caller("cellbox", false, false), LaunchRoute::Elf, "/bin/c2c-render").is_none());
}

#[test]
fn terminal_owns_exactly_one_capability_free_cellbox_path_edge() {
    let terminal = caller("terminal", false, false);
    let edge = authorize(terminal, LaunchRoute::Path, "/bin/cellbox")
        .expect("terminal -> /bin/cellbox edge must exist");
    // X/M/W plus the Q Config-write right (Phase 01 CP1, D-Q5); nothing else.
    assert_eq!(edge.child_ceiling, SHELL_EDGE_Q);
    assert!(!edge.requires_lifecycle_authority);
    // The edge does not depend on caller lifecycle capabilities.
    assert_eq!(
        authorize(caller("terminal", true, true), LaunchRoute::Path, "/bin/cellbox"),
        Some(edge)
    );
    for route in [LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
        assert!(authorize(terminal, route, "/bin/cellbox").is_none(), "{route:?}");
    }
    for target in [
        "/bin/ls",
        "/bin/cat",
        "/bin/echo",
        "/bin/ps",
        "/bin/vfs",
        "/bin/net",
        "/bin/shutdown",
        "/bin/kill",
        "/bin/terminal",
        "/bin/desktop",
        "/bin/unreviewed",
        "/bin/cellbox2",
        "/apps/cellbox",
    ] {
        for route in [LaunchRoute::Path, LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
            assert!(
                authorize(terminal, route, target).is_none(),
                "terminal must not launch {target} on {route:?}"
            );
        }
    }
}

#[test]
fn phase04_driver_rows_are_feature_gated_and_exact() {
    let driver = caller("phase04-terminal-driver", false, false);
    let init = caller("init", true, false);
    #[cfg(feature = "phase04-driver-profile")]
    {
        let edge = authorize(driver, LaunchRoute::Path, "/bin/cellbox")
            .expect("driver -> /bin/cellbox edge must exist under the fixture feature");
        assert_eq!(edge.child_ceiling, SHELL_EDGE_Q);
        assert!(!edge.requires_lifecycle_authority);
        let output_probe = authorize(driver, LaunchRoute::Path, "/bin/echo")
            .expect("private driver may probe stock echo output grants");
        assert_eq!(output_probe.child_ceiling, LEGACY);
        assert!(!output_probe.requires_lifecycle_authority);
        for route in [LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
            assert!(authorize(driver, route, "/bin/echo").is_none(), "{route:?}");
        }
        for route in [LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
            assert!(authorize(driver, route, "/bin/cellbox").is_none(), "{route:?}");
        }
        for target in ["/bin/ls", "/bin/cat", "/bin/vfs", "/bin/terminal", "/bin/shutdown"] {
            assert!(authorize(driver, LaunchRoute::Path, target).is_none(), "{target}");
        }
        // Init may start the driver on the path route and (as for every autoload entry)
        // the ELF route, never Mem/Pinned; the ceiling is empty either way and
        // lifecycle authority is required.
        for route in [LaunchRoute::Path, LaunchRoute::Elf] {
            let boot = authorize(init, route, "/bin/phase04-terminal-driver")
                .expect("init -> driver edge must exist under the fixture feature");
            assert_eq!(boot.child_ceiling, LEGACY);
            assert!(boot.requires_lifecycle_authority);
            assert!(authorize(caller("init", false, false), route, "/bin/phase04-terminal-driver").is_none());
        }
        for route in [LaunchRoute::Mem, LaunchRoute::Pinned] {
            assert!(authorize(init, route, "/bin/phase04-terminal-driver").is_none(), "{route:?}");
        }
        // The driver is no one else's launch target.
        for name in ["cellbox", "terminal", "desktop"] {
            assert!(
                authorize(caller(name, true, true), LaunchRoute::Path, "/bin/phase04-terminal-driver")
                    .is_none(),
                "{name}"
            );
        }
    }
    #[cfg(not(feature = "phase04-driver-profile"))]
    {
        assert!(authorize(driver, LaunchRoute::Path, "/bin/cellbox").is_none());
        assert!(authorize(driver, LaunchRoute::Path, "/bin/echo").is_none());
        for route in [LaunchRoute::Path, LaunchRoute::Elf, LaunchRoute::Mem, LaunchRoute::Pinned] {
            assert!(
                authorize(init, route, "/bin/phase04-terminal-driver").is_none(),
                "{route:?}"
            );
        }
    }
}

/// `pre` with the three S2-T/S2-W rights ORed in.
fn with_legacy(pre: CapSet) -> CapSet {
    CapSet {
        observe_tasks: true,
        observe_memory: true,
        vfs_mutate: true,
        ..pre
    }
}

/// Annex S2-T/S2-W §7 census check: every reviewed target row that existed
/// before the annex keeps its exact pre-change ceiling plus X/M/W, row by row,
/// and the sandbox default arm stays EMPTY. The `pre` values are the table as
/// it stood before the annex; a row missing the constant fails here by name.
#[test]
fn every_pre_annex_target_row_keeps_observe_mutate() {
    use crate::resource_registry::{DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};
    use super::targets::reviewed_user_target_ceiling;

    let none = CapSet::EMPTY;
    let client = CapSet {
        net_resolve: true,
        net_connect: true,
        ..CapSet::EMPTY
    };
    let peer = CapSet {
        net_listen: true,
        ..client
    };
    let spawn = CapSet {
        spawn: true,
        ..CapSet::EMPTY
    };
    let mmio = |mmio_devices| CapSet {
        mmio_devices,
        ..CapSet::EMPTY
    };
    let mut rows: alloc::vec::Vec<(&str, CapSet)> = [
        "/bin/adc-demo",
        "/bin/ai-test",
        "/bin/audio-demo",
        "/bin/bench-probe",
        "/bin/c2c-render-worker",
        "/bin/c2c-render-domain-worker",
        "/bin/can-demo",
        "/bin/cat",
        "/bin/cfi-test",
        "/bin/c-pthread",
        "/bin/cpp-smoke",
        "/bin/tls-test",
        "/bin/futex-test",
        "/bin/pipe-test",
        "/bin/pipe-peer",
        "/bin/c-spawn",
        "/bin/backend-worker",
        "/bin/doom",
        "/bin/echo",
        "/bin/free",
        "/bin/gpio-test-rv",
        "/bin/http-smoke",
        "/bin/input-test",
        "/bin/window-policy-probe",
        "/bin/ls",
        "/bin/posix-shim-test",
        "/bin/ps",
        "/bin/robot-dashboard",
        "/bin/viui-demo",
        "/bin/tetris",
        "/bin/tetris-c",
        "/bin/tetris-lua",
        "/bin/vfs-test",
        "/bin/wx-test",
        "/bin/tier2-exploit",
        "/bin/tier2-smoke",
        "/bin/tier2-rpc-provider",
        "/bin/tier2-rpc-driver",
        "/bin/std-smoke",
        "/bin/desktop",
        "/bin/ocel-js",
        "/bin/ocel-quickjs",
        "/bin/ocel-pdf",
        "/bin/chrome",
        "/bin/vm-runner",
        "/bin/files",
        "/bin/terminal",
        "/bin/python",
        "/bin/lua",
        "/bin/tool-fs",
        "/bin/tool-sys",
    ]
    .into_iter()
    .map(|path| (path, none))
    .collect();
    for path in [
        "/bin/curl",
        "/bin/wget",
        "/bin/mqtt",
        "/bin/ocel",
        "/bin/llm-gateway",
        "/bin/https-demo",
    ] {
        rows.push((path, client));
    }
    for path in [
        "/bin/bench",
        "/bin/c2c-render",
        "/bin/capacity-probe",
        "/bin/hypha",
        "/bin/tool-spawn",
        "/bin/backend-supervisor",
        "/bin/hotswap-demo-v1",
        "/bin/hotswap-demo-v2",
    ] {
        rows.push((path, spawn));
    }
    rows.extend([
        ("/bin/nc", peer),
        (
            "/bin/httpd",
            CapSet {
                net_listen: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/net-broker",
            CapSet {
                network: true,
                ..peer
            },
        ),
        (
            "/bin/dwc2-usb",
            CapSet {
                usb_driver: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/hypervisor",
            CapSet {
                hypervisor: true,
                net_l2: true,
                ..CapSet::EMPTY
            },
        ),
        ("/bin/periph-demo", mmio(DEV_GPIO | DEV_UART)),
        ("/bin/periph-test", mmio(DEV_GPIO | DEV_UART)),
        ("/bin/pwm-demo", mmio(DEV_GPIO)),
        ("/bin/sensor-demo", mmio(DEV_GPIO | DEV_I2C)),
        ("/bin/spi-demo", mmio(DEV_GPIO | DEV_SPI)),
        (
            "/bin/robot-demo",
            CapSet {
                network: true,
                mmio_devices: DEV_GPIO,
                ..client
            },
        ),
    ]);
    for (path, pre) in rows {
        assert_eq!(
            reviewed_user_target_ceiling(path),
            Some(with_legacy(pre)),
            "{path}: the pre-annex row must keep its ceiling plus X/M/W"
        );
    }
    for path in ["/apps/demo", "/data/bin/tool", "/data/bin/s2tw-probe", "/home/user/app"] {
        assert_eq!(reviewed_user_target_ceiling(path), Some(CapSet::EMPTY), "{path}");
    }
    assert_eq!(reviewed_user_target_ceiling("/bin/unreviewed"), None);
}

/// The same census for the boot-ceiling table: each pre-annex row keeps its
/// exact authority plus X/M/W; the sandbox arm stays EMPTY and unknown paths
/// stay unlisted. The boot self-test runs on the host too.
#[test]
fn every_pre_annex_boot_row_keeps_observe_mutate() {
    use crate::resource_registry::{DEV_DISPLAY, DEV_GPIO, DEV_I2C, DEV_SPI, DEV_UART};

    let pcie = CapSet {
        pcie_driver: true,
        mmio_devices: DEV_DISPLAY,
        ..CapSet::EMPTY
    };
    let mut rows: alloc::vec::Vec<(&str, CapSet)> = alloc::vec![
        (
            "/bin/init",
            CapSet {
                block_io: true,
                network: true,
                spawn: true,
                hypervisor: true,
                mmio_devices: DEV_GPIO | DEV_UART | DEV_I2C | DEV_SPI | DEV_DISPLAY,
                block_regions: 0b1111,
                pcie_driver: true,
                supervisor: true,
                usb_driver: true,
                serial_port: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/platform",
            CapSet {
                platform: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/vfs",
            CapSet {
                block_io: true,
                block_regions: 0b1111,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/net",
            CapSet {
                network: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/net-broker",
            CapSet {
                network: true,
                net_resolve: true,
                net_connect: true,
                net_listen: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/supervisor",
            CapSet {
                spawn: true,
                supervisor: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/silo",
            CapSet {
                hypervisor: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/hypervisor",
            CapSet {
                hypervisor: true,
                net_l2: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/xhci",
            CapSet {
                usb_driver: true,
                ..pcie
            },
        ),
        (
            "/bin/serial",
            CapSet {
                serial_port: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/bcm-display",
            CapSet {
                mmio_devices: DEV_DISPLAY,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/dwc2-usb",
            CapSet {
                spawn: true,
                usb_driver: true,
                ..CapSet::EMPTY
            },
        ),
        (
            "/bin/sshd",
            CapSet {
                net_listen: true,
                ..CapSet::EMPTY
            },
        ),
        // Phase 01 CP1 (D-Q5): the boot shell row additionally carries Q.
        (
            "/bin/cellbox",
            CapSet {
                config_write: true,
                ..CapSet::EMPTY
            },
        ),
    ];
    for path in [
        "/bin/block",
        "/bin/nvme",
        "/bin/e1000",
        "/bin/igb",
        "/bin/virtio-net",
        "/bin/virtio-gpu",
        "/bin/input",
        "/bin/ahci",
    ] {
        rows.push((path, pcie));
    }
    for path in [
        "/bin/lan9514",
        "/bin/ocel",
        "/bin/ocel-js",
        "/bin/ocel-quickjs",
        "/bin/ocel-pdf",
        "/bin/chrome",
        "/bin/vm-runner",
        "/bin/files",
        "/bin/terminal",
        "/bin/config",
        "/bin/compositor",
        "/bin/fb-console",
        "/bin/kms",
        "/bin/silo-test",
        "/bin/vfs-test",
        "/bin/srv-test",
        "/bin/std-smoke",
        "/bin/desktop",
        "/bin/tier2-exploit",
        "/bin/tier2-smoke",
        "/bin/cpp-smoke",
        "/bin/tier2-rpc-provider",
        "/bin/tier2-rpc-driver",
    ] {
        rows.push((path, CapSet::EMPTY));
    }
    for (path, pre) in rows {
        assert_eq!(
            boot_ceiling::lookup(path),
            Some(with_legacy(pre)),
            "{path}: the pre-annex boot row must keep its ceiling plus X/M/W"
        );
    }
    for path in ["/apps/demo", "/data/bin/tool", "/home/user/app"] {
        assert_eq!(boot_ceiling::lookup(path), Some(CapSet::EMPTY), "{path}");
    }
    assert_eq!(boot_ceiling::lookup("/bin/no-such-cell"), None);
    assert!(boot_ceiling::self_test(), "boot-ceiling self-test must hold on the host");
}

/// Annex S2-T/S2-W §6/E1: the private `/bin/s2tw-probe` positive-control row
/// holds exactly X/M/W under the fixture feature and does not exist otherwise,
/// while the same binary at the sandbox path gets nothing on either route.
#[test]
fn s2tw_probe_fixture_row_is_feature_gated() {
    for route in [LaunchRoute::Path, LaunchRoute::Elf] {
        assert_eq!(
            shell_edge(route, "/data/bin/s2tw-probe").child_ceiling,
            CapSet::EMPTY,
            "{route:?}"
        );
        let control = authorize(caller("cellbox", false, false), route, "/bin/s2tw-probe");
        #[cfg(feature = "phase04-driver-profile")]
        assert_eq!(control.map(|p| p.child_ceiling), Some(LEGACY), "{route:?}");
        #[cfg(not(feature = "phase04-driver-profile"))]
        assert!(control.is_none(), "{route:?}");
    }
}

/// The caller-owned-ELF gate counts the S2-T/S2-W rights as no authority (they
/// were ambient before the annex), but still refuses every other right.
#[test]
fn elf_route_ignores_only_the_legacy_rights() {
    for target in ["/bin/echo", "/bin/free", "/bin/ps", "/bin/vfs-test", "/bin/hotswap"] {
        assert_eq!(
            authorize(caller("cellbox", false, false), LaunchRoute::Elf, target)
                .map(|p| p.child_ceiling),
            Some(LEGACY),
            "{target}"
        );
    }
    for target in ["/bin/curl", "/bin/httpd", "/bin/capacity-probe", "/bin/periph-demo"] {
        assert!(
            authorize(caller("cellbox", false, false), LaunchRoute::Elf, target).is_none(),
            "{target}"
        );
    }
}

/// The shell-hosting ceiling of Phase 01 CP1 (D-Q5): X/M/W plus Q.
const SHELL_EDGE_Q: CapSet = CapSet {
    config_write: true,
    ..LEGACY
};

/// Phase 01 CP1 (Q) grant table: only the three shell-hosting edges carry
/// `config_write` — the init boot row, the GUI terminal edge and the private
/// driver edge — and only `/bin/cellbox` requests it, so request ∩ ceiling
/// yields Q there and nowhere else.
#[test]
fn config_write_is_held_only_by_the_shell_edges() {
    use super::targets::reviewed_user_target_ceiling;

    let request = CapSet::EMPTY.with_path_caps("/bin/cellbox");
    assert!(request.config_write, "/bin/cellbox requests Q");
    // Boot row and terminal edge are the same ceiling.
    assert_eq!(boot_ceiling::lookup("/bin/cellbox"), Some(SHELL_EDGE_Q));
    let terminal = authorize(caller("terminal", false, false), LaunchRoute::Path, "/bin/cellbox")
        .expect("terminal -> /bin/cellbox edge");
    assert_eq!(terminal.child_ceiling, SHELL_EDGE_Q);
    assert!(request.intersect(terminal.child_ceiling).config_write);
    let init = authorize(caller("init", true, false), LaunchRoute::Path, "/bin/cellbox")
        .expect("init -> /bin/cellbox edge");
    assert_eq!(init.child_ceiling, SHELL_EDGE_Q);

    // No other path requests Q, the ssh install of the same ELF included.
    for path in [
        "/bin/cellbox-ssh",
        "/bin/config",
        "/bin/init",
        "/bin/lua",
        "/bin/python",
        "/bin/p03-filter",
        "/bin/terminal",
        "/bin/cellbox2",
        "/apps/cellbox",
        "/data/bin/cellbox",
        "/home/user/cellbox",
    ] {
        assert!(!CapSet::EMPTY.with_path_caps(path).config_write, "{path}");
    }
    // The shell's own child edges never carry Q.
    let ssh = shell_edge(LaunchRoute::Path, "/bin/cellbox-ssh");
    assert!(!ssh.child_ceiling.config_write);
    for target in ["/bin/echo", "/bin/curl", "/bin/hotswap", "/bin/shutdown", "/bin/lua"] {
        if let Some(profile) = authorize(caller("cellbox", false, false), LaunchRoute::Path, target) {
            assert!(!profile.child_ceiling.config_write, "{target}");
        }
    }
    for target in ["/bin/curl", "/bin/echo", "/bin/terminal", "/bin/python", "/apps/demo"] {
        if let Some(ceiling) = reviewed_user_target_ceiling(target) {
            assert!(!ceiling.config_write, "{target}");
        }
    }
    // Q is not P-TRUST and is not one of the ELF-gate-exempt legacy rights.
    assert!(!CapSet::path_mints_ptrust("/bin/cellbox"));
    assert_eq!(SHELL_EDGE_Q.without_ptrust(), SHELL_EDGE_Q);
    assert_eq!(
        SHELL_EDGE_Q.without_observe_mutate(),
        CapSet {
            config_write: true,
            ..CapSet::EMPTY
        }
    );
}

/// KSSH K3 grant table (`k3-cp1.md` §4.5, §8) extended by KSSH K7: `kms_ssh_sign` can
/// be non-false only when the path is exactly `/bin/cellbox-ssh` (request) AND the
/// spawn edge is the shell's reviewed `SSH_CLIENT_EDGE` (ceiling); `kms_ssh_admin`
/// only when the path is exactly `/bin/kms-ssh-admin` AND the edge is the shell's
/// reviewed `KMS_SSH_ADMIN_EDGE`. The shared `NET_CLIENT` ceiling of curl, wget, mqtt,
/// ocel, llm-gateway, https-demo and robot-demo does not carry either, no boot row
/// carries either, and no edge, boot row or request carries both rights.
#[test]
fn kms_ssh_sign_is_held_only_by_the_ssh_edge() {
    use super::profiles::SSH_CLIENT_EDGE;
    use super::targets::{reviewed_user_target_ceiling, NET_CLIENT};
    use api::caller_identity::CALLER_FLAG_KMS_SSH_SIGN;

    // The constant is the shared client ceiling plus the one right, and the shared
    // ceiling itself is unchanged.
    assert_eq!(
        SSH_CLIENT_EDGE,
        CapSet {
            net_resolve: true,
            net_connect: true,
            kms_ssh_sign: true,
            ..LEGACY
        }
    );
    assert_eq!(
        NET_CLIENT,
        CapSet {
            net_resolve: true,
            net_connect: true,
            ..LEGACY
        }
    );
    assert!(!NET_CLIENT.kms_ssh_sign && !NET_CLIENT.kms_ssh_admin);
    assert_eq!(SSH_CLIENT_EDGE.without_ptrust(), SSH_CLIENT_EDGE);
    assert_eq!(
        SSH_CLIENT_EDGE.without_observe_mutate(),
        CapSet {
            net_resolve: true,
            net_connect: true,
            kms_ssh_sign: true,
            ..CapSet::EMPTY
        }
    );
    assert!(!CapSet::path_mints_ptrust("/bin/cellbox-ssh"));

    // The one grant: request ∩ edge ceiling holds the sign right, the client pair
    // and X/M/W, and nothing else (no Q, no admin).
    let ssh_request = CapSet::EMPTY.with_path_caps("/bin/cellbox-ssh");
    let granted = ssh_request.intersect(shell_edge(LaunchRoute::Path, "/bin/cellbox-ssh").child_ceiling);
    assert_eq!(
        granted,
        CapSet {
            net_resolve: true,
            net_connect: true,
            kms_ssh_sign: true,
            ..LEGACY
        }
    );
    assert_eq!(granted.kms_caller_flags(), CALLER_FLAG_KMS_SSH_SIGN);

    // The `NET_CLIENT` targets keep their client pair and gain no KMS right.
    for target in [
        "/bin/curl",
        "/bin/wget",
        "/bin/mqtt",
        "/bin/ocel",
        "/bin/llm-gateway",
        "/bin/https-demo",
        "/bin/robot-demo",
    ] {
        let ceiling = shell_edge(LaunchRoute::Path, target).child_ceiling;
        assert_eq!(net_rights(ceiling), (true, true, false, false), "{target}");
        assert_eq!(ceiling.kms_caller_flags(), 0, "{target}");
        let kept = CapSet::EMPTY.with_path_caps(target).intersect(ceiling);
        assert_eq!(kept.kms_caller_flags(), 0, "{target}");
    }

    // Sweep every path literal of the reviewed launch and boot tables (plus the
    // near-miss and sandbox spellings of the two KMS paths) against every caller
    // name, caller capability combination and route.
    fn quoted_paths(src: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = src;
        while let Some(start) = rest.find("\"/") {
            let tail = &rest[start + 1..];
            let Some(end) = tail.find('"') else { break };
            let literal = &tail[..end];
            if literal.len() > 1 && !literal.contains(char::is_whitespace) {
                found.push(literal);
            }
            rest = &tail[end + 1..];
        }
        found
    }
    let mut paths: Vec<&str> = Vec::new();
    for source in [
        include_str!("targets.rs"),
        include_str!("profiles.rs"),
        include_str!("../boot_ceiling.rs"),
    ] {
        paths.extend(quoted_paths(source));
    }
    paths.extend([
        "/bin/cellbox-ssh",
        "/bin/cellbox-ssh2",
        "/bin/cellbox-sshd",
        "/bin/kms-ssh-admin",
        "/bin/kms-ssh-admin2",
        "/bin/kms",
        "/mem/cellbox-ssh",
        "/apps/cellbox-ssh",
        "/data/bin/cellbox-ssh",
        "/home/user/cellbox-ssh",
        "/apps/kms-ssh-admin",
        "/data/bin/kms-ssh-admin",
        "/home/user/kms-ssh-admin",
    ]);
    assert!(paths.len() > 100, "the sweep must cover the reviewed tables");
    assert!(paths.contains(&"/bin/cellbox-ssh") && paths.contains(&"/bin/curl"));

    let callers = [
        "init",
        "cellbox",
        "terminal",
        "desktop",
        "pipe-test",
        "c-spawn",
        "hypha",
        "tool-spawn",
        "dwc2-usb",
        "backend-supervisor",
        "c2c-render",
        "supervisor",
        "bench",
        "capacity-probe",
        "periph-demo",
        "phase02-session-parent",
        "phase02-session-child",
        "phase04-terminal-driver",
        "cellbox-ssh",
        "kms",
        "no-such-caller",
    ];
    let mut ssh_edges = 0;
    let mut admin_edges = 0;
    for path in paths {
        // Request side: exactly the two spellings, nothing nearby.
        let request = CapSet::EMPTY.with_path_caps(path);
        assert_eq!(request.kms_ssh_sign, path == "/bin/cellbox-ssh", "{path}");
        assert_eq!(request.kms_ssh_admin, path == "/bin/kms-ssh-admin", "{path}");
        // No boot row and no reviewed generic target carries either right.
        let boot = boot_ceiling::boot_ceiling(path);
        assert_eq!(boot.kms_caller_flags(), 0, "boot row {path}");
        assert!(!request.intersect(boot).kms_ssh_sign && !request.intersect(boot).kms_ssh_admin);
        if let Some(ceiling) = reviewed_user_target_ceiling(path) {
            assert_eq!(ceiling.kms_caller_flags(), 0, "reviewed target {path}");
        }
        // Edge side: only (shell, kernel-resolved path, /bin/cellbox-ssh).
        for name in callers {
            for has_spawn in [false, true] {
                for has_supervisor in [false, true] {
                    for route in [
                        LaunchRoute::Path,
                        LaunchRoute::Elf,
                        LaunchRoute::Mem,
                        LaunchRoute::Pinned,
                    ] {
                        let Some(profile) =
                            authorize(caller(name, has_spawn, has_supervisor), route, path)
                        else {
                            continue;
                        };
                        let ceiling = profile.child_ceiling;
                        let ssh_edge = name == "cellbox"
                            && route == LaunchRoute::Path
                            && path == "/bin/cellbox-ssh";
                        assert_eq!(
                            ceiling.kms_ssh_sign, ssh_edge,
                            "{name} {route:?} {path}: sign right on the wrong edge"
                        );
                        let admin_edge = name == "cellbox"
                            && route == LaunchRoute::Path
                            && path == "/bin/kms-ssh-admin";
                        assert_eq!(
                            ceiling.kms_ssh_admin, admin_edge,
                            "{name} {route:?} {path}: admin right on the wrong edge"
                        );
                        assert!(
                            !(ceiling.kms_ssh_sign && ceiling.kms_ssh_admin),
                            "{name} {route:?} {path}: both KMS rights on one ceiling"
                        );
                        ssh_edges += ssh_edge as usize;
                        admin_edges += admin_edge as usize;
                        // The sign right is only ever granted to the ssh install path.
                        assert_eq!(
                            request.intersect(ceiling).kms_ssh_sign,
                            ssh_edge && request.kms_ssh_sign,
                            "{name} {route:?} {path}"
                        );
                        assert_eq!(
                            request.intersect(ceiling).kms_ssh_admin,
                            admin_edge && request.kms_ssh_admin,
                            "{name} {route:?} {path}"
                        );
                        assert!(
                            !(request.intersect(ceiling).kms_ssh_sign
                                && request.intersect(ceiling).kms_ssh_admin)
                        );
                    }
                }
            }
        }
    }
    // The shell reaches the edge in each of its four (has_spawn, has_supervisor)
    // states for every occurrence of the path in the sweep, so this is a floor.
    assert!(ssh_edges >= 4, "the sweep never saw the ssh edge");
    assert!(admin_edges >= 4, "the sweep never saw the admin edge");
}
