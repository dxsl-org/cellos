use alloc::format;
use api::caller_identity::CallerIdentity;
use api::hostile_backend_recovery::{
    encode_kill_response, parse_kill_request, DISCONNECT_LOG_MARKER, KILL_STATUS_INVALID_REQUEST,
    KILL_STATUS_KILL_FAILED, KILL_STATUS_OK, KILL_STATUS_REJECTED_CALLER,
    KILL_STATUS_SERVICE_NOT_ALLOWED, KILL_STATUS_SERVICE_NOT_FOUND,
};
use api::syscall::service;
use ostd::io::println;
use ostd::ipc::Incoming;
use ostd::service::provider_tid;
use ostd::syscall::sys_kill_cell;

/// Signed programs reviewed to request a kill (see `sender_is_program`).
const HYPERVISOR_PROGRAM: &str = "hypervisor";
const BENCH_PROGRAM: &str = "bench";
const HOSTILE_BACKEND_KILL_REASON: u32 = 0x4842_4B52;

pub(crate) fn handle(caller: Option<CallerIdentity>, sender_tid: usize, data: &[u8]) {
    let is_hypervisor = super::sender_is_program(caller, sender_tid, HYPERVISOR_PROGRAM);
    let is_bench = super::sender_is_program(caller, sender_tid, BENCH_PROGRAM);

    if !is_hypervisor && !is_bench {
        reply(sender_tid, KILL_STATUS_REJECTED_CALLER);
        return;
    }
    if data.len() < 3 {
        reply(sender_tid, KILL_STATUS_INVALID_REQUEST);
        return;
    }
    let Some(service_id) = parse_kill_request(&data[..3]) else {
        reply(sender_tid, KILL_STATUS_INVALID_REQUEST);
        return;
    };
    if is_bench && service_id != service::VFS {
        reply(sender_tid, KILL_STATUS_REJECTED_CALLER);
        return;
    }
    let Some(service_name) = allowed_service_name(service_id) else {
        reply(sender_tid, KILL_STATUS_SERVICE_NOT_ALLOWED);
        return;
    };
    let Some(old_tid) = provider_tid(service_id) else {
        reply(sender_tid, KILL_STATUS_SERVICE_NOT_FOUND);
        return;
    };
    if sys_kill_cell(old_tid, HOSTILE_BACKEND_KILL_REASON).is_err() {
        reply(sender_tid, KILL_STATUS_KILL_FAILED);
        return;
    }

    println(&format!(
        "{DISCONNECT_LOG_MARKER} service={service_name} old_tid={old_tid}"
    ));
    reply(sender_tid, KILL_STATUS_OK);
}

fn allowed_service_name(service_id: u16) -> Option<&'static str> {
    match service_id {
        service::VFS => Some("vfs"),
        service::NET => Some("net"),
        _ => None,
    }
}

/// Nothing in `handle` receives, so the latest request is still this one.
fn reply(sender_tid: usize, status: u8) {
    let _ = Incoming::current(sender_tid).reply(&encode_kill_response(status));
}
