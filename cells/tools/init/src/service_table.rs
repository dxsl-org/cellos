use api::syscall::service;
use ostd::syscall::{sys_register_service, sys_spawn_from_path, SyscallResult};

#[derive(Clone, Copy)]
pub(crate) enum RestartPolicy {
    Permanent,
    #[cfg(not(feature = "hypervisor-min"))]
    Transient,
    #[allow(dead_code)]
    Temporary,
}

#[derive(Clone, Copy)]
pub(crate) enum Registration {
    None,
    Init(u16),
    #[cfg(feature = "development-silo-provider")]
    SelfReady(u16),
}

pub(crate) struct Service {
    pub(crate) path: &'static str,
    pub(crate) registration: Registration,
    pub(crate) policy: RestartPolicy,
    pub(crate) tid: Option<usize>,
    pub(crate) restart_count: u32,
    pub(crate) window_start: u64,
}

impl Service {
    const fn new(path: &'static str, registration: Registration, policy: RestartPolicy) -> Self {
        Self {
            path,
            registration,
            policy,
            tid: None,
            restart_count: 0,
            window_start: 0,
        }
    }

    pub(crate) const fn service_id(&self) -> Option<u16> {
        match self.registration {
            Registration::None => None,
            Registration::Init(id) => Some(id),
            #[cfg(feature = "development-silo-provider")]
            Registration::SelfReady(id) => Some(id),
        }
    }
}

#[cfg(feature = "hypervisor-min")]
pub(crate) const SERVICE_COUNT: usize = 2 + cfg!(feature = "hostile-backend-recovery") as usize;
#[cfg(not(feature = "hypervisor-min"))]
pub(crate) const SERVICE_COUNT: usize = 8
    + cfg!(feature = "development-silo-provider") as usize
    + cfg!(feature = "c2c-broker") as usize;

pub(crate) fn configured() -> [Service; SERVICE_COUNT] {
    #[cfg(feature = "hypervisor-min")]
    return [
        Service::new(
            "/bin/vfs",
            Registration::Init(service::VFS),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/net",
            Registration::Init(service::NET),
            RestartPolicy::Permanent,
        ),
        #[cfg(feature = "hostile-backend-recovery")]
        Service::new(
            "/bin/supervisor",
            Registration::Init(service::SUPERVISOR),
            RestartPolicy::Permanent,
        ),
    ];

    #[cfg(not(feature = "hypervisor-min"))]
    [
        Service::new(
            "/bin/vfs",
            Registration::Init(service::VFS),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/config",
            Registration::Init(service::CONFIG),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/input",
            Registration::Init(service::INPUT),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/net",
            Registration::Init(service::NET),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/compositor",
            Registration::Init(service::COMPOSITOR),
            RestartPolicy::Permanent,
        ),
        #[cfg(feature = "development-silo-provider")]
        Service::new(
            "/bin/silo",
            Registration::SelfReady(service::SILO),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/kms",
            Registration::Init(service::KMS),
            RestartPolicy::Permanent,
        ),
        #[cfg(feature = "c2c-broker")]
        Service::new(
            "/bin/net-broker",
            Registration::Init(service::NET_BROKER),
            RestartPolicy::Permanent,
        ),
        Service::new(
            "/bin/supervisor",
            Registration::Init(service::SUPERVISOR),
            RestartPolicy::Permanent,
        ),
        Service::new("/bin/shell", Registration::None, RestartPolicy::Transient),
    ]
}

pub(crate) fn spawn(service: &mut Service) -> Option<usize> {
    let tid = match sys_spawn_from_path(service.path) {
        SyscallResult::Ok(tid) => tid,
        _ => {
            service.tid = None;
            return None;
        }
    };
    service.tid = Some(tid);
    if let Registration::Init(service_id) = service.registration {
        let _ = sys_register_service(service_id, tid);
    }
    Some(tid)
}
#[cfg(feature = "development-silo-provider")]
pub(crate) fn wait_for_exact_registration(service_id: u16, expected_tid: usize) -> bool {
    const READY_TIMEOUT_TICKS: u64 = 5_000;
    let started = ostd::syscall::sys_get_time();
    loop {
        match ostd::syscall::sys_lookup_service(service_id) {
            Some(tid) => return tid == expected_tid,
            None if ostd::syscall::sys_get_time().wrapping_sub(started) >= READY_TIMEOUT_TICKS => {
                return false;
            }
            None => ostd::task::yield_now(),
        }
    }
}
