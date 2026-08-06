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
fn shell_net_tool_edge_is_exact() {
    assert_eq!(
        shell_edge(LaunchRoute::Elf, "/bin/httpd").parent_ceiling,
        CapSet {
            network: true,
            ..CapSet::EMPTY
        }
    );
    assert!(
        authorize(caller("shell", false, false), LaunchRoute::Elf, "/bin/vfs").is_none(),
        "shell must not launch privileged services by path forgery"
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
