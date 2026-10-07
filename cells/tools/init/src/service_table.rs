use api::syscall::service;
use ostd::syscall::{sys_register_service, sys_spawn_from_path, SyscallResult};

#[derive(Clone, Copy)]
pub(crate) enum RestartPolicy {
    Permanent,
    /// Restarted only while it has not exited cleanly; both profiles' shells use
    /// it, so it is not gated on the profile any more.
    Transient,
    #[allow(dead_code)]
    Temporary,
}

#[derive(Clone, Copy)]
pub(crate) enum Registration {
    /// The shell starts unregistered in every profile; the server and embedded
    /// tables register every other service they launch.
    #[allow(dead_code)]
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

/// Services every image carries: storage, the network service and a shell on a
/// serial console. This is the embedded-first floor -- Cellos boots to a prompt
/// on a board with nothing but a UART, and everything else is an option.
pub(crate) const BASE_SERVICES: usize = 3;

/// One term per option, so adding an option means adding one `#[cfg]` entry to
/// the table below and one term here. A missing term is a compile error, not a
/// silently truncated table: `configured()` must produce exactly this many.
pub(crate) const SERVICE_COUNT: usize = BASE_SERVICES
    + cfg!(feature = "input") as usize
    + cfg!(feature = "ai") as usize * 2
    + cfg!(feature = "ui") as usize
    + (cfg!(feature = "ui") && !cfg!(feature = "board-rpi3")) as usize
    + cfg!(any(
        feature = "supervisor",
        feature = "hostile-backend-recovery"
    )) as usize
    + cfg!(feature = "c2c-broker") as usize
    + cfg!(feature = "development-silo-provider") as usize;

pub(crate) fn configured() -> [Service; SERVICE_COUNT] {
    [
        Service::new(
            "/bin/vfs",
            Registration::Init(service::VFS),
            RestartPolicy::Permanent,
        ),
        // Spec 24 unified inference service. Fail-soft: when the image has no
        // /bin/ai (or no model), init skips it with a log line and the service,
        // if present without a model, refuses inference truthfully. The config
        // service ships with it -- the inference front end reads its settings
        // there, and nothing else in the base needs it.
        #[cfg(feature = "ai")]
        Service::new(
            "/bin/config",
            Registration::Init(service::CONFIG),
            RestartPolicy::Permanent,
        ),
        #[cfg(feature = "ai")]
        Service::new(
            "/bin/ai",
            Registration::Init(service::AI),
            RestartPolicy::Permanent,
        ),
        // Keyboard and mouse event routing. Needed wherever a HID source exists
        // (the USB host cell on a Pi, VirtIO input on a desktop image) and
        // harmless where there is none.
        #[cfg(feature = "input")]
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
        // UI bundle: a compositor surface, plus the KMS service on boards whose
        // display is a real driver rather than VirtIO.
        #[cfg(feature = "ui")]
        Service::new(
            "/bin/compositor",
            Registration::Init(service::COMPOSITOR),
            RestartPolicy::Permanent,
        ),
        #[cfg(all(feature = "ui", not(feature = "board-rpi3")))]
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
        // Hotswap supervision. `hostile-backend-recovery` turns this same cell
        // into the recovery handler, so it implies it.
        #[cfg(any(feature = "supervisor", feature = "hostile-backend-recovery"))]
        Service::new(
            "/bin/supervisor",
            Registration::Init(service::SUPERVISOR),
            RestartPolicy::Permanent,
        ),
        #[cfg(feature = "development-silo-provider")]
        Service::new(
            "/bin/silo",
            Registration::SelfReady(service::SILO),
            RestartPolicy::Permanent,
        ),
        // Last: init asserts it, and by then everything the shell may launch is
        // already up.
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
/// Scheduler ticks (10 ms slices) — the clock the kernel itself uses for `RecvTimeout`
/// deadlines, and the one every `*_TICKS` constant in this crate assumes.
///
/// `sys_get_time()` is `GetTime` **op 0**, the raw architected counter (10 MHz `mtime` on
/// RV64), so a window compared against it is a thousand times too small: the restart
/// budget rolled on every exit and `init` never gave up on a crash-looping service.
pub(crate) fn now_ticks() -> u64 {
    ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0)
}

#[cfg(feature = "development-silo-provider")]
pub(crate) fn wait_for_exact_registration(service_id: u16, expected_tid: usize) -> bool {
    const READY_TIMEOUT_TICKS: u64 = 5_000;
    let started = now_ticks();
    loop {
        match ostd::syscall::sys_lookup_service(service_id) {
            Some(tid) => return tid == expected_tid,
            None if now_ticks().wrapping_sub(started) >= READY_TIMEOUT_TICKS => {
                return false;
            }
            None => ostd::task::yield_now(),
        }
    }
}
