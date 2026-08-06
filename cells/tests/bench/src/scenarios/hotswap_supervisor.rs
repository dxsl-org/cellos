//! Runtime witness for state-preserving Supervisor Cell replacement.

use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{
    sys_lookup_service, sys_recv, sys_send, sys_set_spawn_args, sys_spawn_pinned, sys_yield,
    SyscallResult,
};

const OP_HOTSWAP: u8 = 0x01;
const OP_STATUS: u8 = 0x03;
const REQUEST_LEN: usize = 1 + 64 + 128;

pub fn run() {
    println("[hotswap-supervisor-runtime] START");
    let Some(old_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        fail("demo-v1 is not registered");
    };
    for _ in 0..5 {
        if !send_and_receive(old_tid, b"inc", b"ok") {
            fail("demo-v1 increment failed");
        }
    }
    println("[hotswap-supervisor-runtime] counter primed to 5");
    if !spawn_cached_sender_probe(old_tid) {
        fail("cannot spawn cached-sender probe");
    }

    let Some(supervisor_tid) = sys_lookup_service(service::SUPERVISOR) else {
        fail("supervisor is not registered");
    };
    let request = hotswap_request();
    if !send_app_message(supervisor_tid, &request) {
        fail("cannot send Supervisor hotswap request");
    }
    println("[hotswap-supervisor-runtime] request accepted");
    let mut status = [0u8; 3];
    if !matches!(
        sys_recv(supervisor_tid, &mut status),
        SyscallResult::Ok(sender) if sender == supervisor_tid
    ) || status != [OP_STATUS, 6, 0]
    {
        fail("Supervisor hotswap failed");
    }

    let Some(new_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        fail("replacement is not registered");
    };
    let mut response = [0u8; 7];
    if !send_app_message(new_tid, b"get")
        || !matches!(
            sys_recv(new_tid, &mut response),
            SyscallResult::Ok(sender) if sender == new_tid
        )
        || &response[..3] != b"v2:"
        || u32::from_le_bytes(response[3..7].try_into().unwrap_or([0; 4])) != 5
    {
        fail("replacement did not preserve counter=5");
    }
    println("[hotswap-supervisor-runtime] PASS (v1 counter=5 -> v2 counter=5)");
    ostd::syscall::sys_exit(0);
}

pub fn run_cached_sender_probe(role: &str) -> ! {
    let Some(old_tid) = role
        .strip_prefix("hotswap-cached-inc:")
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        probe_fail("invalid cached tid");
    };
    for _ in 0..5_000 {
        if sys_lookup_service(service::HOTSWAP_DEMO).is_none() {
            let mut envelope = [0u8; 5];
            envelope[..2].copy_from_slice(&[0xAC, 0x00]);
            envelope[2..].copy_from_slice(b"inc");
            match sys_send(old_tid, &envelope) {
                SyscallResult::Err(_) => {
                    println("[hotswap-cached-sender] PASS: paused old tid rejected");
                    ostd::syscall::sys_exit(0)
                }
                SyscallResult::Ok(_) => probe_fail("paused old tid accepted mutation"),
            }
        }
        sys_yield();
    }
    probe_fail("pause window not observed")
}

fn send_and_receive(tid: usize, request: &[u8], expected: &[u8]) -> bool {
    let mut response = [0u8; 8];
    send_app_message(tid, request)
        && matches!(sys_recv(tid, &mut response), SyscallResult::Ok(sender) if sender == tid)
        && &response[..expected.len()] == expected
}

fn send_app_message(tid: usize, payload: &[u8]) -> bool {
    let mut envelope = [0u8; REQUEST_LEN + 2];
    if payload.len() + 2 > envelope.len() {
        return false;
    }
    envelope[0] = 0xAC;
    envelope[1] = 0x00;
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

fn spawn_cached_sender_probe(old_tid: usize) -> bool {
    let role = alloc::format!("hotswap-cached-inc:{old_tid}");
    sys_set_spawn_args(&role);
    matches!(
        sys_spawn_pinned("/bin/bench-probe", api::task::TaskPriority::Normal as u8, 0),
        SyscallResult::Ok(_)
    )
}

fn probe_fail(message: &str) -> ! {
    println(&alloc::format!("[hotswap-cached-sender] FAIL ({message})"));
    ostd::syscall::sys_exit(1)
}

fn fail(message: &str) -> ! {
    println(&alloc::format!(
        "[hotswap-supervisor-runtime] FAIL ({message})"
    ));
    ostd::syscall::sys_exit(1)
}
