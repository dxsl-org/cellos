use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use service_net_broker::local_ingress::ParsedLocalRequest;
use service_net_broker::restart_ipc_state::{
    arm_shutdown, finish_admission, RestartIpcState as State,
};
const RUNTIME_ROLE_COUNT: usize = 3;
// Elapsed-time shutdown bounds remain stable as runnable-task counts change.
const DRAIN_TIMEOUT_MS: u64 = service_net_broker::runtime_roles::RESTART_DRAIN_TIMEOUT_MS;
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static NETWORK_IPC_STATE: AtomicUsize = AtomicUsize::new(State::Idle.raw());
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
    // Shutdown atomically classifies an idle, admitting, or admitted exchange.
    // An in-progress admission publishes whether the post succeeded before the
    // shutdown flag is released.
    let shutdown_before_admission = loop {
        let state = NETWORK_IPC_STATE.load(Ordering::Acquire);
        let Some(state_value) = State::from_raw(state) else {
            exit_admission_timeout();
        };
        let Some((armed, before_admission)) = arm_shutdown(state_value) else {
            break false;
        };
        if NETWORK_IPC_STATE
            .compare_exchange(state, armed.raw(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break before_admission;
        }
    };

    let Some(admission_started_at_ms) = ostd::syscall::sys_get_time_ms() else {
        exit_admission_timeout();
    };
    while NETWORK_IPC_STATE.load(Ordering::Acquire) != State::Acked.raw() {
        let Some(now_ms) = ostd::syscall::sys_get_time_ms() else {
            exit_admission_timeout();
        };
        if now_ms.wrapping_sub(admission_started_at_ms) >= DRAIN_TIMEOUT_MS {
            exit_admission_timeout();
        }
        ostd::task::yield_now();
    }

    SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    if shutdown_before_admission {
        ostd::io::println("[net-broker] restart oracle shutdown-before-admission exercised");
    }
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

/// Claims the atomic state surrounding one nonblocking admission attempt.
/// Returns `false` when shutdown already owns the exchange.
pub fn network_ipc_admission_started() -> bool {
    NETWORK_IPC_STATE
        .compare_exchange(
            State::Idle.raw(),
            State::Admitting.raw(),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

/// Publishes whether the guarded admission attempt queued its request.
/// Returns `true` when shutdown raced the attempt and owns its result.
pub fn network_ipc_admission_finished(admitted: bool) -> bool {
    loop {
        let state = NETWORK_IPC_STATE.load(Ordering::Acquire);
        let Some(state_value) = State::from_raw(state) else {
            return true;
        };
        let Some((next, shutdown_owned)) = finish_admission(state_value, admitted) else {
            return true;
        };
        if NETWORK_IPC_STATE
            .compare_exchange(state, next.raw(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if !shutdown_owned {
                return false;
            }
            while !shutdown_requested() {
                ostd::task::yield_now();
            }
            if admitted {
                ostd::io::println("[net-broker] restart oracle shutdown-after-admission exercised");
            } else {
                ostd::io::println(
                    "[net-broker] restart oracle shutdown-before-admission exercised",
                );
            }
            return true;
        }
    }
}

/// Acknowledges that an armed restart found the currently admitted IPC waiting.
/// The acknowledgement remains published until shutdown releases the wait.
pub fn network_ipc_cancellation_checkpoint() {
    let _ = NETWORK_IPC_STATE.compare_exchange(
        State::ArmedActive.raw(),
        State::Acked.raw(),
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

/// Completes the active IPC at the restart linearization point.
/// Returns `true` when restart claimed the exchange and its response must be
/// discarded; returns `false` when normal completion won the race.
pub fn network_ipc_finished() -> bool {
    loop {
        let state = NETWORK_IPC_STATE.load(Ordering::Acquire);
        let Some(state_value) = State::from_raw(state) else {
            return true;
        };
        let idle = match state_value {
            State::Active => State::Idle,
            State::ArmedActive => State::Acked,
            State::Acked => {
                while !shutdown_requested() {
                    ostd::task::yield_now();
                }
                ostd::io::println("[net-broker] restart oracle shutdown-after-admission exercised");
                return true;
            }
            _ => return true,
        };
        if NETWORK_IPC_STATE
            .compare_exchange(state, idle.raw(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if idle == State::Acked {
                continue;
            }
            return false;
        }
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
