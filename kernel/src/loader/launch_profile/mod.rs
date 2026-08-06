//! Exact launch-edge profiles for kernel-authorized spawns.
//!
//! Authorization is by the exact `(caller identity, route, target path)` edge.
//! This is deliberately narrower than ambient `SpawnCap`: callers keep their
//! own lifecycle authority model, while child ceilings come from reviewed rows.

mod profiles;
mod targets;

#[cfg(test)]
mod tests;

use crate::task::cap::CapSet;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LaunchRoute {
    Path,
    Elf,
    Mem,
    Pinned,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CallerLaunchState<'a> {
    pub name: &'a str,
    pub has_spawn: bool,
    pub has_supervisor: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LaunchProfile {
    pub parent_ceiling: CapSet,
    pub denial_label: &'static str,
    pub requires_lifecycle_authority: bool,
}

impl LaunchProfile {
    pub(super) const fn new(
        parent_ceiling: CapSet,
        denial_label: &'static str,
        requires_lifecycle_authority: bool,
    ) -> Self {
        Self {
            parent_ceiling,
            denial_label,
            requires_lifecycle_authority,
        }
    }
}

pub fn authorize(
    caller: CallerLaunchState<'_>,
    route: LaunchRoute,
    target: &str,
) -> Option<LaunchProfile> {
    let profile = match caller.name {
        "init" if caller.has_spawn => profiles::init_profile(route, target),
        "shell" => profiles::shell_profile(route, target),
        "hypha" if caller.has_spawn => profiles::hypha_profile(route, target),
        "tool-spawn" if caller.has_spawn => profiles::tool_spawn_profile(route, target),
        "supervisor" if caller.has_spawn && caller.has_supervisor => {
            profiles::supervisor_profile(route, target)
        }
        "bench" | "capacity-probe" if caller.has_spawn => {
            profiles::pinned_profile(caller.name, route, target)
        }
        "periph-demo" => profiles::pinned_profile(caller.name, route, target),
        _ => None,
    }?;

    // SpawnFromElf carries caller-owned bytes and only an advisory path. Until
    // VFS can attest grant provenance, that route must never mint authority from
    // the path: arbitrary bytes could otherwise borrow a reviewed privileged
    // target name. Capability-bearing ELF routes therefore remain available
    // only to lifecycle-authority profiles (init/supervisor); capability-free
    // tools preserve the current shell/VFS launch path.
    if matches!(route, LaunchRoute::Elf)
        && profile.parent_ceiling != CapSet::EMPTY
        && !profile.requires_lifecycle_authority
    {
        return None;
    }

    Some(profile)
}
