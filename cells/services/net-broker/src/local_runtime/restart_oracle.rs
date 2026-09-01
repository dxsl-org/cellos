use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use service_net_broker::local_ingress::ParsedLocalRequest;

const RUNTIME_ROLE_COUNT: usize = 3;
const NETWORK_IPC_IDLE: usize = 0;
const NETWORK_IPC_ACTIVE: usize = 1;
const NETWORK_IPC_ARMED: usize = 2;
// Bound shutdown by elapsed time, not scheduler turns: runnable-task count changes
// how many yields a role needs before it can observe the shutdown flag.
const DRAIN_TIMEOUT_MS: u64 = service_net_broker::runtime_roles::RESTART_DRAIN_TIMEOUT_MS;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static NETWORK_IPC_STATE: AtomicUsize = AtomicUsize::new(NETWORK_IPC_IDLE);
static EXITED_ROLES: AtomicUsize = AtomicUsize::new(0);

pub fn request_matches(
    sender: usize,
    identity: Option<&api::caller_identity::CallerIdentity>,
    request: Option<&ParsedLocalRequest>,
) -> bool {
    identity
        .is_some_and(|identity| identity.sender_tid as usize == sender && identity.generation != 0)
        && request.is_some_and(|request| {
            request.payload_len == 2
                && request.payload[0] == service_net_broker::bench_oracle::OP_ECHO
                && request.payload[1] == service_net_broker::bench_oracle::OP_RESTART
        })
}

pub fn shutdown() -> ! {
    // Claim an exchange atomically. If completion wins the race, wait for the
    // next admission rather than treating a stale observation as proof.
    let Some(admission_started_at_ms) = ostd::syscall::sys_get_time_ms() else {
        exit_admission_timeout();
    };
    loop {
        if NETWORK_IPC_STATE
            .compare_exchange(
                NETWORK_IPC_ACTIVE,
                NETWORK_IPC_ARMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            break;
        }
        let Some(now_ms) = ostd::syscall::sys_get_time_ms() else {
            exit_admission_timeout();
        };
        if now_ms.wrapping_sub(admission_started_at_ms) >= DRAIN_TIMEOUT_MS {
            exit_admission_timeout();
        }
        ostd::task::yield_now();
    }

    SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    let Some(started_at_ms) = ostd::syscall::sys_get_time_ms() else {
        exit_drain_timeout();
    };
    loop {
        if EXITED_ROLES.load(Ordering::Acquire) == RUNTIME_ROLE_COUNT {
            ostd::io::println("[net-broker] restart oracle drained runtime roles");
            ostd::syscall::sys_exit(0xC2C0_0300);
        }
        let Some(now_ms) = ostd::syscall::sys_get_time_ms() else {
            exit_drain_timeout();
        };
        if now_ms.wrapping_sub(started_at_ms) >= DRAIN_TIMEOUT_MS {
            exit_drain_timeout();
        }
        ostd::task::yield_now();
    }
}

/// Marks one post-admission IPC active.
///
/// Returns `false` if another exchange is active or restart already claimed it.
pub fn network_ipc_started() -> bool {
    NETWORK_IPC_STATE
        .compare_exchange(
            NETWORK_IPC_IDLE,
            NETWORK_IPC_ACTIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

/// Completes the active IPC at the restart linearization point.
///
/// Returns `true` when restart claimed the exchange and its response must be
/// discarded; returns `false` when normal completion won the race.
pub fn network_ipc_finished() -> bool {
    match NETWORK_IPC_STATE.compare_exchange(
        NETWORK_IPC_ACTIVE,
        NETWORK_IPC_IDLE,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => false,
        Err(NETWORK_IPC_ARMED) => {
            while !shutdown_requested() {
                ostd::task::yield_now();
            }
            ostd::io::println("[net-broker] restart oracle shutdown-after-admission exercised");
            true
        }
        Err(_) => true,
    }
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Acquire)
}

fn exit_admission_timeout() -> ! {
    ostd::io::println("[net-broker] restart oracle IPC admission timed out");
    ostd::syscall::sys_exit(0xC2C0_03FE)
}

fn exit_drain_timeout() -> ! {
    ostd::io::println("[net-broker] restart oracle role drain timed out");
    ostd::syscall::sys_exit(0xC2C0_03FF)
}

pub fn exit_role_if_requested() {
    if shutdown_requested() {
        EXITED_ROLES.fetch_add(1, Ordering::AcqRel);
        ostd::syscall::sys_exit(0);
    }
}
