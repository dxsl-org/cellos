use alloc::string::String;
use alloc::vec::Vec;
use api::syscall::service;
use cellos_boot_config::{CellSpec, Registration, RestartPolicy};
use ostd::syscall::{self, SyscallError, SyscallResult};

pub(crate) struct Service {
    pub(crate) spec: CellSpec,
    pub(crate) tid: Option<usize>,
    pub(crate) ready: bool,
    pub(crate) restart_pending: bool,
    pub(crate) restart_count: u32,
    pub(crate) window_start: u64,
}

impl Service {
    pub(crate) fn new(spec: CellSpec) -> Self {
        Self { spec, tid: None, ready: false, restart_pending: false, restart_count: 0, window_start: 0 }
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
        self.spec.enabled && self.ready && self.tid.is_some_and(|tid| {
            crate::activation::task_alive(tid) == Some(true)
                && self.spec.service_id.is_none_or(|id| syscall::sys_lookup_service(id) == Some(tid))
        })
    }
}

/// Check the live incarnation, not merely whether a dependency once launched.
pub(crate) fn dependencies_available(services: &[Service], index: usize) -> bool {
    services[index].spec.after.iter().all(|name| {
        services.iter().find(|candidate| candidate.spec.name == *name)
            .is_some_and(Service::available)
    })
}

/// Every attempt republishes the full structured argv. The general ostd path
/// helper may fall back after an ELF rejection, which consumes the staged argv;
/// keeping the attempts here lets init republish it before the raw-path attempt.
fn spawn_path(spec: &CellSpec) -> Option<usize> {
    if let Some(vfs) = syscall::sys_lookup_service(service::VFS) {
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
    let registered = match (service.spec.registration, service.spec.service_id) {
        (Registration::Init, Some(id)) => {
            matches!(syscall::sys_register_service(id, tid), SyscallResult::Ok(0))
                && syscall::sys_lookup_service(id) == Some(tid)
        }
        (Registration::SelfReady, Some(id)) => {
            wait_for_exact_registration(id, tid, service.spec.ready_timeout_ticks)
        }
        (Registration::Init, None) => true,
        (Registration::SelfReady, None) => false,
    };
    service.ready = watched && registered && crate::activation::task_alive(tid) == Some(true)
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
        match syscall::sys_lookup_service(id) {
            Some(tid) => return tid == expected_tid,
            None if now_ticks().wrapping_sub(started) >= timeout
                || crate::activation::task_alive(expected_tid) == Some(false) => return false,
            None => ostd::task::yield_now(),
        }
    }
    false
}
