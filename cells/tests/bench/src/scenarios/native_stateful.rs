//! Native stateful workload: 1,000 deterministic increments of the supervised
//! `hotswap-demo` service, a checkpoint every 100 operations in
//! `/srv/checkpoint.log`, a v1 -> v2 hotswap at operation 300 with a cached-TID
//! increment as operation 301, a VFS restart at operation 600, and a failed
//! hotswap that must leave the live provider intact.
//!
//! The shell drives it in steps, because only the shell-launched `hotswap` CLI
//! may ask the supervisor for a swap, and a shell-launched bench reaches `/srv`
//! only through its cwd handle (run it from `cd /srv`):
//!   `bench native-stateful prime`   ops 1-300, checkpoints 1-3, cached sender armed
//!   `hotswap hotswap-demo /bin/hotswap-demo-v2`
//!   `bench native-stateful restart` op 301 reconciled, ops 302-600, checkpoints
//!                                   4-6, then VFS killed through the supervisor
//!                                   (init restarts it; this launch ends with it)
//!   `bench native-stateful finish`  checkpoints 1-6 survived the restart, ops
//!                                   601-1000, checkpoints 7-10
//!   `hotswap hotswap-demo /bin/hotswap-demo-nonexistent` (must fail)
//!   `bench native-stateful live`    v2 still serves counter 1000

extern crate alloc;

use alloc::format;
use abi::dir_handles::ViDirHandle;
use wire::hostile_backend_recovery::{
    encode_kill_request, parse_kill_response, KILL_STATUS_OK,
};
use wire::proto::app_envelope::{HEADER_LEN, KIND_MESSAGE, MAGIC};
use abi::syscall::service;
use ostd::io::println;
use ostd::service::provider_tid;
use ostd::syscall::{
    sys_heartbeat, sys_recv, sys_send, sys_set_spawn_args, sys_set_timer, sys_spawn_pinned,
    sys_yield, SyscallResult,
};

use super::native_stateful_log::{self as log, CHECKPOINT_INTERVAL};

const TOTAL_OPS: u32 = 1000;
const WAIT_TICKS: usize = 500;
const CUTOVER_WINDOW_TICKS: usize = 1_600;
/// The supervisor answers a kill request once the Cell is gone.
const KILL_REPLY_TICKS: u64 = 200;

pub fn run(step: Option<&str>) -> ! {
    match step {
        Some("prime") => prime(),
        Some("restart") => restart(),
        Some("finish") => finish(),
        Some("live") => live(),
        _ => fail("usage: bench native-stateful prime|restart|finish|live"),
    }
}

fn prime() -> ! {
    let dir = srv_dir();
    let demo = demo_tid();
    expect_counter(demo, b"v1:", 0);
    log::clear(dir);
    run_ops(demo, dir, 1..=300);
    expect_counter(demo, b"v1:", 300);
    if !spawn_cached_sender_probe(demo) {
        fail("cannot spawn the cached-sender probe for operation 301");
    }
    done("primed: v1 counter=300, checkpoints 1-3, cached sender armed")
}

fn restart() -> ! {
    let dir = srv_dir();
    let demo = demo_tid();
    // Operation 301 is the cached sender's increment, drained into v2 at cutover.
    let mut counter = None;
    for _ in 0..100 {
        counter = read_counter(demo, b"v2:");
        if counter == Some(301) {
            break;
        }
        sys_yield();
    }
    if counter != Some(301) {
        fail(&format!(
            "op 301 not reconciled into v2: counter {counter:?}"
        ));
    }
    println("[native-stateful] op 301 reconciled via the cached-TID witness");
    run_ops(demo, dir, 302..=600);
    expect_log(dir, 6);

    let Some(supervisor) = provider_tid(service::SUPERVISOR) else {
        fail("SUPERVISOR service is not registered");
    };
    // This launch ends with the VFS: its cwd handle was prepared by that VFS, and
    // the kernel ends a shell-launched child whose launch the dead VFS took part
    // in. The lane checks the restart in init's log and the log in `finish`.
    println("[native-stateful] killing VFS through the supervisor");
    let (mut send, mut reply) = ([0u8; 8], [0u8; 8]);
    let status = ostd::ipc::service_call_bounded(
        supervisor,
        &encode_kill_request(service::VFS),
        &mut send,
        &mut reply,
        KILL_REPLY_TICKS,
    )
    .ok()
    .and_then(parse_kill_response);
    if status != Some(KILL_STATUS_OK) {
        fail(&format!("supervisor refused the VFS kill: {status:?}"));
    }
    ostd::syscall::sys_exit(0)
}

fn finish() -> ! {
    let dir = srv_dir();
    let demo = demo_tid();
    expect_log(dir, 6);
    println("[native-stateful] checkpoints 1-6 intact after the VFS restart");
    expect_counter(demo, b"v2:", 600);
    run_ops(demo, dir, 601..=TOTAL_OPS);
    expect_counter(demo, b"v2:", TOTAL_OPS);
    expect_log(dir, TOTAL_OPS / CHECKPOINT_INTERVAL);
    done("1000/1000 ops, 10 checkpoints verified")
}

fn live() -> ! {
    expect_counter(demo_tid(), b"v2:", TOTAL_OPS);
    println("[native-stateful] failed hotswap preserved the live provider and its counter");
    done("ALL CRITERIA PASSED")
}

fn run_ops(demo: usize, dir: ViDirHandle, ops: core::ops::RangeInclusive<u32>) {
    for op in ops {
        sys_heartbeat(0);
        if !inc_demo(demo) {
            fail(&format!("increment failed at op {op}"));
        }
        if op % CHECKPOINT_INTERVAL == 0 {
            let seq = op / CHECKPOINT_INTERVAL;
            if let Err(error) = log::commit(dir, seq) {
                fail(&format!("checkpoint {seq}: {error}"));
            }
            println(&format!(
                "[native-stateful] checkpoint {seq} committed: counter={op}"
            ));
        }
    }
}

fn expect_log(dir: ViDirHandle, count: u32) {
    match log::read(dir) {
        Some(bytes) if bytes == log::expected(count) => {}
        Some(bytes) => fail(&format!(
            "checkpoint log holds {} bytes, not records 1-{count}",
            bytes.len()
        )),
        None => fail("cannot read the checkpoint log"),
    }
}

fn expect_counter(demo: usize, prefix: &[u8; 3], expected: u32) {
    let counter = read_counter(demo, prefix);
    if counter != Some(expected) {
        fail(&format!(
            "demo counter expected {expected} from {}, got {counter:?}",
            core::str::from_utf8(&prefix[..2]).unwrap_or("?")
        ));
    }
}

fn srv_dir() -> ViDirHandle {
    log::srv_dir().unwrap_or_else(|error| fail(error))
}

fn demo_tid() -> usize {
    provider_tid(service::HOTSWAP_DEMO)
        .unwrap_or_else(|| fail("HOTSWAP_DEMO service is not registered"))
}

fn inc_demo(tid: usize) -> bool {
    let mut resp = [0u8; 2];
    let msg = [MAGIC, KIND_MESSAGE, b'i', b'n', b'c'];
    matches!(sys_send(tid, &msg), SyscallResult::Ok(_))
        && recv_from(tid, &mut resp)
        && &resp == b"ok"
}

fn read_counter(tid: usize, expected_prefix: &[u8; 3]) -> Option<u32> {
    let mut response = [0u8; 7];
    let msg = [MAGIC, KIND_MESSAGE, b'g', b'e', b't'];
    if !matches!(sys_send(tid, &msg), SyscallResult::Ok(_)) || !recv_from(tid, &mut response) {
        return None;
    }
    if &response[..3] != expected_prefix {
        return None;
    }
    Some(u32::from_le_bytes(response[3..7].try_into().ok()?))
}

fn spawn_cached_sender_probe(old_tid: usize) -> bool {
    let role = format!("native-stateful-cached-inc:{old_tid}");
    sys_set_spawn_args(&role);
    matches!(
        sys_spawn_pinned("/bin/bench-probe", abi::task::TaskPriority::Normal as u8, 0),
        SyscallResult::Ok(_)
    )
}

pub fn run_cached_sender_probe(role: &str) -> ! {
    sys_heartbeat(0);
    let Some(old_tid) = role
        .strip_prefix("native-stateful-cached-inc:")
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        fail("invalid cached tid in probe");
    };

    let mut inc = [0u8; 5];
    inc[..HEADER_LEN].copy_from_slice(&[MAGIC, KIND_MESSAGE]);
    inc[2..].copy_from_slice(b"inc");

    let mut sent = false;
    for _ in 0..CUTOVER_WINDOW_TICKS {
        sys_heartbeat(0);
        if provider_tid(service::HOTSWAP_DEMO).is_none()
            && matches!(sys_send(old_tid, &inc), SyscallResult::Ok(_))
        {
            sent = true;
            break;
        }
        let _ = sys_set_timer(1);
    }

    if !sent {
        for _ in 0..WAIT_TICKS {
            sys_heartbeat(0);
            if let Some(new_tid) = provider_tid(service::HOTSWAP_DEMO) {
                if new_tid != old_tid {
                    let _ = sys_send(new_tid, &inc);
                    break;
                }
            }
            sys_yield();
        }
    }
    ostd::syscall::sys_exit(0);
}

fn recv_from(expected_sender: usize, buf: &mut [u8]) -> bool {
    matches!(sys_recv(expected_sender, buf), SyscallResult::Ok(sender) if sender == expected_sender)
}

fn done(message: &str) -> ! {
    println(&format!("[native-stateful] {message}"));
    ostd::syscall::sys_exit(0)
}

fn fail(message: &str) -> ! {
    println(&format!("[native-stateful] FAIL: {message}"));
    ostd::syscall::sys_exit(1)
}
