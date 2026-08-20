use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use service_net_broker::local_ingress::ParsedLocalRequest;

const RUNTIME_ROLE_COUNT: usize = 3;
const DRAIN_TURNS: usize = 4_096;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
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
    SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    for _ in 0..DRAIN_TURNS {
        if EXITED_ROLES.load(Ordering::Acquire) == RUNTIME_ROLE_COUNT {
            ostd::io::println("[net-broker] restart oracle drained runtime roles");
            ostd::syscall::sys_exit(0xC2C0_0300);
        }
        ostd::task::yield_now();
    }
    ostd::io::println("[net-broker] restart oracle role drain timed out");
    ostd::syscall::sys_exit(0xC2C0_03FF)
}

pub fn exit_role_if_requested() {
    if SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
        EXITED_ROLES.fetch_add(1, Ordering::AcqRel);
        ostd::syscall::sys_exit(0);
    }
}
