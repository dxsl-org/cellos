//! Runtime witness for state-preserving Supervisor Cell replacement.

use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{
    sys_lookup_service, sys_recv, sys_send, sys_set_spawn_args, sys_set_timer, sys_spawn_pinned,
    SyscallResult,
};

const OP_HOTSWAP: u8 = 0x01;
const OP_STATUS: u8 = 0x03;
const APP_MESSAGE_PREFIX: [u8; 2] = [0xAC, 0x00];
const REQUEST_LEN: usize = 1 + 64 + 128;
const WAIT_TICKS: usize = 500;
const CUTOVER_WINDOW_TICKS: usize = 1_600;
const UNAUTHORIZED_DENIED_STATUS: [u8; 3] = [OP_STATUS, 0, 0xFD];

pub fn run() {
    println("[hotswap-supervisor-runtime] START");
    let Some(old_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        fail("demo-v1 is not registered");
    };
    for _ in 0..5 {
        if !expect_reply(old_tid, b"inc", b"ok") {
            fail("demo-v1 increment failed");
        }
    }
    println("[hotswap-supervisor-runtime] counter primed to 5");
    if !spawn_cached_sender_probe(old_tid) {
        fail("cannot spawn cached-sender probe");
    }

    if sys_lookup_service(service::SUPERVISOR).is_none() {
        fail("supervisor is not registered");
    }
    println("[hotswap-supervisor-runtime] ready for CLI trigger (v1 counter=5)");

    let Some(new_tid) = wait_for_hotswap_demo_replacement_ticks(old_tid, WAIT_TICKS) else {
        fail("replacement is not registered");
    };
    if read_counter(new_tid, b"v2:") != Some(6) {
        fail("replacement did not apply the drained increment");
    }
    println("[hotswap-supervisor-runtime] PASS (v1 counter=5 -> v2 counter=6)");
    ostd::syscall::sys_exit(0);
}

pub fn run_unauthorized_probe() {
    println("[hotswap-unauthorized-runtime] START");
    let Some(old_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        unauthorized_fail("demo-v1 is not registered");
    };
    let Some(supervisor_tid) = sys_lookup_service(service::SUPERVISOR) else {
        unauthorized_fail("supervisor is not registered");
    };
    let request = hotswap_request();
    if !send_app_message(supervisor_tid, &request) {
        unauthorized_fail("cannot send canonical request to supervisor");
    }
    let mut status = [0u8; 3];
    if !recv_from(supervisor_tid, &mut status) || status != UNAUTHORIZED_DENIED_STATUS {
        unauthorized_fail("supervisor did not reject the direct bench sender");
    }
    if sys_lookup_service(service::HOTSWAP_DEMO) != Some(old_tid) {
        unauthorized_fail("unauthorized request changed the target service tid");
    }
    println("[hotswap-unauthorized-runtime] PASS (direct bench sender denied; tid unchanged)");
    ostd::syscall::sys_exit(0);
}

pub fn run_cached_sender_probe(role: &str) -> ! {
    let Some(old_tid) = role
        .strip_prefix("hotswap-cached-inc:")
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        probe_fail("invalid cached tid");
    };
    let mut inc = [0u8; 5];
    inc[..2].copy_from_slice(&[0xAC, 0x00]);
    inc[2..].copy_from_slice(b"inc");
    let mut get = [0u8; 5];
    get[..2].copy_from_slice(&[0xAC, 0x00]);
    get[2..].copy_from_slice(b"get");

    if !queue_during_cutover(old_tid, &inc, &get, CUTOVER_WINDOW_TICKS) {
        probe_fail("frozen old FIFO window not observed");
    }

    let Some(new_tid) = wait_for_hotswap_demo_replacement_ticks(old_tid, WAIT_TICKS) else {
        probe_fail("replacement did not publish a new HOTSWAP_DEMO tid");
    };
    if !matches!(sys_send(old_tid, &inc), SyscallResult::Err(_)) {
        probe_fail("post-cutover old tid accepted ingress");
    }

    let mut first = [0u8; 8];
    if !matches!(sys_recv(new_tid, &mut first), SyscallResult::Ok(sender) if sender == new_tid)
        || &first[..2] != b"ok"
    {
        probe_fail("drained inc reply missing or out of order");
    }
    let mut second = [0u8; 7];
    if !matches!(sys_recv(new_tid, &mut second), SyscallResult::Ok(sender) if sender == new_tid)
        || &second[..3] != b"v2:"
        || u32::from_le_bytes(second[3..7].try_into().unwrap_or([0; 4])) != 6
    {
        probe_fail("drained get reply missing or counter was not 6");
    }
    println(
        "[hotswap-cached-sender] PASS: frozen FIFO drained in order; post-cutover old tid rejected",
    );
    ostd::syscall::sys_exit(0)
}

pub(super) fn expect_reply(tid: usize, request: &[u8], expected: &[u8]) -> bool {
    let mut response = [0u8; 8];
    send_app_message(tid, request)
        && recv_from(tid, &mut response)
        && &response[..expected.len()] == expected
}

pub(super) fn send_app_message(tid: usize, payload: &[u8]) -> bool {
    let mut envelope = [0u8; REQUEST_LEN + 2];
    if payload.len() + 2 > envelope.len() {
        return false;
    }
    envelope[..APP_MESSAGE_PREFIX.len()].copy_from_slice(&APP_MESSAGE_PREFIX);
    envelope[2..2 + payload.len()].copy_from_slice(payload);
    matches!(
        sys_send(tid, &envelope[..2 + payload.len()]),
        SyscallResult::Ok(_)
    )
}

fn hotswap_request() -> [u8; REQUEST_LEN] {
    let mut request = [0u8; REQUEST_LEN];
    request[0] = OP_HOTSWAP;
    let service_name = b"hotswap-demo";
    request[1..1 + service_name.len()].copy_from_slice(service_name);
    let path = b"/bin/hotswap-demo-v2";
    request[65..65 + path.len()].copy_from_slice(path);
    request
}

pub(super) fn wait_for_hotswap_demo_replacement_ticks(
    old_tid: usize,
    max_ticks: usize,
) -> Option<usize> {
    for _ in 0..max_ticks {
        if let Some(tid) = sys_lookup_service(service::HOTSWAP_DEMO) {
            if tid != old_tid {
                return Some(tid);
            }
        }
        if !matches!(sys_set_timer(1), SyscallResult::Ok(_)) {
            return None;
        }
    }
    None
}

fn queue_during_cutover(old_tid: usize, inc: &[u8; 5], get: &[u8; 5], max_ticks: usize) -> bool {
    for _ in 0..max_ticks {
        if sys_lookup_service(service::HOTSWAP_DEMO).is_none()
            && matches!(sys_send(old_tid, inc), SyscallResult::Ok(_))
        {
            if !matches!(sys_send(old_tid, get), SyscallResult::Ok(_)) {
                probe_fail("cutover split the two-message frozen FIFO witness");
            }
            return true;
        }
        if !matches!(sys_set_timer(1), SyscallResult::Ok(_)) {
            return false;
        }
    }
    false
}

pub(super) fn read_counter(tid: usize, expected_prefix: &[u8; 3]) -> Option<u32> {
    let mut response = [0u8; 7];
    if !send_app_message(tid, b"get") {
        return None;
    }
    if !recv_from(tid, &mut response) {
        return None;
    }
    if &response[..3] != expected_prefix {
        return None;
    }
    Some(u32::from_le_bytes(
        response[3..7].try_into().unwrap_or([0; 4]),
    ))
}

fn spawn_cached_sender_probe(old_tid: usize) -> bool {
    let role = alloc::format!("hotswap-cached-inc:{old_tid}");
    sys_set_spawn_args(&role);
    matches!(
        sys_spawn_pinned("/bin/bench-probe", api::task::TaskPriority::Normal as u8, 0),
        SyscallResult::Ok(_)
    )
}

fn recv_from(expected_sender: usize, buf: &mut [u8]) -> bool {
    matches!(sys_recv(expected_sender, buf), SyscallResult::Ok(sender) if sender == expected_sender)
}

fn probe_fail(message: &str) -> ! {
    println(&alloc::format!("[hotswap-cached-sender] FAIL ({message})"));
    ostd::syscall::sys_exit(1)
}

fn unauthorized_fail(message: &str) -> ! {
    println(&alloc::format!(
        "[hotswap-unauthorized-runtime] FAIL ({message})"
    ));
    ostd::syscall::sys_exit(1)
}

fn fail(message: &str) -> ! {
    println(&alloc::format!(
        "[hotswap-supervisor-runtime] FAIL ({message})"
    ));
    ostd::syscall::sys_exit(1)
}
