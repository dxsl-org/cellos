//! Hostile-image-only backend fault control.

use api::hostile_backend_recovery::{encode_kill_request, parse_kill_response, KILL_STATUS_OK};
use api::syscall::service;

const CONTROL_TIMEOUT_TICKS: u64 = 20;

/// Ask the hostile-image supervisor to terminate the active backend service.
///
/// Returns only after an authenticated supervisor response arrives within the
/// bounded timeout. The production hypervisor never compiles this control path.
pub fn disconnect(service_id: u16) -> bool {
    if !matches!(service_id, service::VFS | service::NET) {
        return false;
    }
    let Some(supervisor_tid) = ostd::syscall::sys_lookup_service(service::SUPERVISOR) else {
        return false;
    };
    let request = encode_kill_request(service_id);
    let mut send_buffer = [0u8; 8];
    let mut response_buffer = [0u8; 8];
    let Ok(response) = ostd::ipc::service_call_bounded(
        supervisor_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        CONTROL_TIMEOUT_TICKS,
    ) else {
        return false;
    };
    parse_kill_response(response) == Some(KILL_STATUS_OK)
}
