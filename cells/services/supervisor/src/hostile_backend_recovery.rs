use alloc::format;
use api::hostile_backend_recovery::{
    encode_kill_response, parse_kill_request, DISCONNECT_LOG_MARKER, KILL_STATUS_INVALID_REQUEST,
    KILL_STATUS_KILL_FAILED, KILL_STATUS_OK, KILL_STATUS_REJECTED_CALLER,
    KILL_STATUS_SERVICE_NOT_ALLOWED, KILL_STATUS_SERVICE_NOT_FOUND,
};
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_kill_cell, sys_lookup_service, sys_send};

const HYPERVISOR_TASK_NAME: &str = "hypervisor";
const HOSTILE_BACKEND_KILL_REASON: u32 = 0x4842_4B52;

pub(crate) fn handle(sender_tid: usize, data: &[u8]) {
    if !super::sender_has_exact_name(sender_tid, HYPERVISOR_TASK_NAME) {
        reply(sender_tid, KILL_STATUS_REJECTED_CALLER);
        return;
    }

    let Some(service_id) = parse_kill_request(data) else {
        reply(sender_tid, KILL_STATUS_INVALID_REQUEST);
        return;
    };
    let Some(service_name) = allowed_service_name(service_id) else {
        reply(sender_tid, KILL_STATUS_SERVICE_NOT_ALLOWED);
        return;
    };
    let Some(old_tid) = sys_lookup_service(service_id) else {
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

fn reply(sender_tid: usize, status: u8) {
    let _ = sys_send(sender_tid, &encode_kill_response(status));
}
