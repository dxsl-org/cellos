use alloc::string::String;
use alloc::vec::Vec;
use api::block_route::{ViBlockRouteV1, MAX_BLOCK_ROUTES};
use api::syscall::service;
use cellos_boot_config::{CellSpec, Registration, RestartPolicy};
use ostd::syscall::{self, SyscallError, SyscallResult};

/// How init decides that a spawned incarnation is ready. Init-private: never
/// part of the configured `CellSpec`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// `spec.registration` and `spec.service_id` decide.
    Spec,
    /// A block Driver Cell: ready once a block route (syscall 432) names the
    /// spawned tid as its live provider.
    BlockRoute,
}

pub(crate) struct Service {
    pub(crate) spec: CellSpec,
    pub(crate) readiness: Readiness,
    pub(crate) tid: Option<usize>,
    pub(crate) ready: bool,
    pub(crate) restart_pending: bool,
    pub(crate) restart_count: u32,
    pub(crate) window_start: u64,
}

impl Service {
    pub(crate) fn new(spec: CellSpec) -> Self {
        Self {
            spec,
            readiness: Readiness::Spec,
            tid: None,
            ready: false,
            restart_pending: false,
            restart_count: 0,
            window_start: 0,
        }
    }

    /// VFS is the fixed bootstrap prerequisite, never a configured entry.
    pub(crate) fn bootstrap_vfs() -> Self {
        Self::new(CellSpec {
            name: String::from("vfs"),
            path: String::from("/bin/vfs"),
            args: Vec::new(),
            enabled: true,
            required: true,
            restart: RestartPolicy::Always,
            service_id: Some(service::VFS),
            registration: Registration::Init,
            after: Vec::new(),
            ready_timeout_ticks: 500,
        })
    }

    pub(crate) fn available(&self) -> bool {
        self.spec.enabled
            && self.ready
            && self.tid.is_some_and(|tid| {
                crate::activation::task_alive(tid) == Some(true)
                    && match self.readiness {
                        Readiness::Spec => self
                            .spec
                            .service_id
                            .is_none_or(|id| ostd::service::provider_tid(id) == Some(tid)),
                        Readiness::BlockRoute => routes_name(&block_routes(), tid),
                    }
            })
    }
}

/// Check the live incarnation, not merely whether a dependency once launched.
pub(crate) fn dependencies_available(services: &[Service], index: usize) -> bool {
    services[index].spec.after.iter().all(|name| {
        services
            .iter()
            .find(|candidate| candidate.spec.name == *name)
            .is_some_and(Service::available)
    })
}

/// Every attempt republishes the full structured argv. The general ostd path
/// helper may fall back after an ELF rejection, which consumes the staged argv;
/// keeping the attempts here lets init republish it before the raw-path attempt.
fn spawn_path(spec: &CellSpec) -> Option<usize> {
    if let Some(vfs) = ostd::service::provider_tid(service::VFS) {
        if let Ok((grant, len)) = ostd::fs::read_full_via_grant(&spec.path, vfs) {
            if !ostd::args::set_spawn_argv(&spec.args) {
                syscall::sys_grant_free(grant);
                return None;
            }
            let result = syscall::sys_spawn_from_elf(grant, len, &spec.path);
            syscall::sys_grant_free(grant);
            match result {
                SyscallResult::Ok(tid) if tid != 0 => return Some(tid),
                SyscallResult::Err(SyscallError::OutOfMemory) => return None,
                _ => {}
            }
        }
    }
    if !ostd::args::set_spawn_argv(&spec.args) {
        return None;
    }
    match syscall::sys_spawn_from_path_raw(&spec.path) {
        SyscallResult::Ok(tid) if tid != 0 => Some(tid),
        _ => None,
    }
}

pub(crate) fn spawn(service: &mut Service) -> Option<usize> {
    service.ready = false;
    service.tid = None;
    let tid = spawn_path(&service.spec)?;
    service.tid = Some(tid);
    // Observe death from the first generation, including one that fails readiness.
    let watched = matches!(syscall::sys_notify_on_exit(tid), SyscallResult::Ok(0));
    let registered = match (
        service.readiness,
        service.spec.registration,
        service.spec.service_id,
    ) {
        (Readiness::BlockRoute, _, _) => {
            wait_for_block_route(tid, service.spec.ready_timeout_ticks)
        }
        (Readiness::Spec, Registration::Init, Some(id)) => {
            matches!(
                syscall::sys_register_service(id, tid, ostd::service::namespace_flags(id)),
                SyscallResult::Ok(0)
            ) && ostd::service::provider_tid(id) == Some(tid)
        }
        (Readiness::Spec, Registration::SelfReady, Some(id)) => {
            wait_for_exact_registration(id, tid, service.spec.ready_timeout_ticks)
        }
        (Readiness::Spec, Registration::Init, None) => true,
        (Readiness::Spec, Registration::SelfReady, None) => false,
    };
    service.ready = watched
        && registered
        && crate::activation::task_alive(tid) == Some(true)
        && (service.spec.path != "/bin/vfs"
            || crate::configuration::wait_for_vfs(tid, service.spec.ready_timeout_ticks));
    // A one-shot application may complete before this check. Successful admission
    // and an armed exit watch are a launch, not a failed service registration.
    // Required/dependency gates still use available(), which demands a live task.
    if service.ready || (watched && registered && service.spec.service_id.is_none()) {
        Some(tid)
    } else {
        None
    }
}

/// Scheduler ticks (10 ms slices), not the raw architected GetTime counter.
pub(crate) fn now_ticks() -> u64 {
    syscall::sys_get_scheduler_ticks().unwrap_or(0)
}

pub(crate) fn wait_for_exact_registration(id: u16, expected_tid: usize, timeout: u64) -> bool {
    const POLL_LIMIT: usize = 5_000_000;
    let started = now_ticks();
    for _ in 0..POLL_LIMIT {
        match ostd::service::provider_tid(id) {
            Some(tid) => return tid == expected_tid,
            None if now_ticks().wrapping_sub(started) >= timeout
                || crate::activation::task_alive(expected_tid) == Some(false) =>
            {
                return false
            }
            None => ostd::task::yield_now(),
        }
    }
    false
}

/// Every bound block route, in slot order. Slots fill in bind order and are
/// never removed within a boot, so enumeration stops at the first empty slot.
pub(crate) fn block_routes() -> Vec<ViBlockRouteV1> {
    (0..MAX_BLOCK_ROUTES as u32)
        .map_while(|index| syscall::sys_lookup_block_route(index).ok().flatten())
        .collect()
}

/// Does a route name `tid` as its live provider?
pub(crate) fn routes_name(routes: &[ViBlockRouteV1], tid: usize) -> bool {
    routes.iter().any(|route| {
        route
            .provider()
            .is_some_and(|(provider, _, _)| provider == tid as u64)
    })
}

/// Bounded wait (scheduler ticks, yield between polls) for a block route that
/// names `tid` as its live provider. Fails early once `tid` is gone.
fn wait_for_block_route(tid: usize, timeout: u64) -> bool {
    const POLL_LIMIT: usize = 5_000_000;
    let started = now_ticks();
    for _ in 0..POLL_LIMIT {
        if routes_name(&block_routes(), tid) {
            return true;
        }
        if now_ticks().wrapping_sub(started) >= timeout
            || crate::activation::task_alive(tid) == Some(false)
        {
            return false;
        }
        ostd::task::yield_now();
    }
    false
}

#[cfg(test)]
mod route_readiness_tests {
    use super::routes_name;
    use api::block_route::{
        ViBlockRouteV1, BLOCK_DEVICE_PCI, BLOCK_ROUTE_VERSION, FLAG_PROVIDER_LIVE,
    };

    fn route(id: u64, provider: Option<u64>) -> ViBlockRouteV1 {
        ViBlockRouteV1 {
            version: BLOCK_ROUTE_VERSION,
            kind: BLOCK_DEVICE_PCI,
            flags: if provider.is_some() {
                FLAG_PROVIDER_LIVE
            } else {
                0
            },
            id,
            provider_tid: provider.unwrap_or(0),
            cell_id: provider.map_or(0, |_| 7),
            generation: provider.map_or(0, |_| 1),
        }
    }

    #[test]
    fn a_live_route_naming_the_tid_is_ready() {
        let routes = [route(0x08, Some(41)), route(0xF8, Some(42))];
        assert!(routes_name(&routes, 41));
        assert!(routes_name(&routes, 42));
    }

    #[test]
    fn another_provider_or_no_route_is_not_ready() {
        assert!(!routes_name(&[], 41));
        assert!(!routes_name(&[route(0x08, Some(40))], 41));
    }

    #[test]
    fn a_slot_without_a_live_provider_names_nobody() {
        // Zero provider fields are what an unbound slot carries; tid 0 is never ready.
        let routes = [route(0x08, None)];
        assert!(!routes_name(&routes, 0));
        assert!(!routes_name(&routes, 41));
    }
}
