//! Finite private P01 fixture. Lifecycle success is not resource-settlement evidence.
use alloc::format;
use api::task::TaskPriority;
use ostd::io::println;
use ostd::ipc::{self, IpcTerminal, PendingCall};
use ostd::syscall::{self, SyscallError, SyscallResult};

const BOUND: u64 = 500;
const TAG: u8 = 0x91;
const READY: u8 = 1;
const RELEASE: u8 = 2;

fn fail(reason: &str) -> ! {
    println(&format!("P01-WORKLOAD {{\"status\":\"FAILED\",\"reason\":\"{reason}\"}}"));
    println("P01:closing");
    syscall::sys_exit(1)
}

fn ticks() -> u64 {
    syscall::sys_get_scheduler_ticks().unwrap_or_else(|| fail("scheduler-clock-unavailable"))
}

fn exchange(tid: usize, phase: u8, index: u8, deadline: u64) -> bool {
    let Ok(call) = PendingCall::submit(tid, &[TAG, phase, index]) else { return false; };
    let mut bytes = [0u8; 8];
    while ticks() < deadline {
        match call.try_take(&mut bytes) {
            Ok(Some(done)) => return done.terminal == IpcTerminal::Reply
                && done.len == 3 && bytes[..3] == [TAG, phase, index],
            Ok(None) => { ipc::wait(1); }
            Err(_) => return false,
        }
    }
    // Cancellation is not success; drain our retained slot when possible.
    let _ = ipc::cancel(call.operation());
    let _ = call.try_take(&mut bytes);
    false
}

fn all_live(tids: &[usize]) -> bool {
    let mut rows = [api::syscall::ProcessInfo::default(); 64];
    let Ok(count) = syscall::sys_get_procs(&mut rows) else { return false; };
    count < rows.len() && tids.iter().all(|tid| rows[..count].iter()
        .any(|row| row.id == *tid && row.state != 3))
}

/// Exactly two bounded requests; release acknowledgment precedes normal exit.
#[allow(dead_code)]
pub fn run_probe() -> ! {
    let deadline = ticks().checked_add(BOUND).unwrap_or_else(|| fail("clock-overflow"));
    let mut bytes = [0u8; 8];
    let mut parent = 0;
    let mut index = 0;
    for phase in [READY, RELEASE] {
        let remaining = deadline.saturating_sub(ticks());
        if remaining == 0 { fail("probe-timeout"); }
        let sender = match syscall::sys_recv_timeout(0, &mut bytes, remaining) {
            SyscallResult::Ok(sender) if sender != 0 => sender,
            _ => fail("probe-receive"),
        };
        if phase == READY { parent = sender; index = bytes[2]; }
        if sender != parent || bytes[..3] != [TAG, phase, index] || index >= 8 {
            fail("probe-protocol");
        }
        let Some(operation) = ipc::current() else { fail("probe-operation-missing"); };
        if ipc::reply(operation, &[TAG, phase, index]).is_err() { fail("probe-reply"); }
    }
    syscall::sys_exit(0)
}

pub fn run(role: &str) -> ! {
    let denied = role == "parked-footprint:denied";
    let requested = if denied { 1 } else {
        match role.strip_prefix("parked-footprint:") {
            Some("0") => 0, Some("1") => 1, Some("4") => 4, Some("8") => 8,
            _ => fail("count-invalid"),
        }
    };
    println(&format!("P01:pre count={requested}"));
    let deadline = ticks().checked_add(BOUND).unwrap_or_else(|| fail("clock-overflow"));
    if denied {
        if !syscall::sys_set_spawn_args("parked-footprint-probe") { fail("denied-argv-not-run"); }
        let result = syscall::sys_spawn_pinned("/bin/vfs", TaskPriority::Normal as u8, 0);
        // SpawnPinned preserves the legacy -1 -> Unknown public mapping.
        // This observation alone is not denial proof: the host must join it to
        // the exact raw kernel PermissionDenied outcome and absence of a child.
        let public_result_preserved = matches!(result, SyscallResult::Err(SyscallError::Unknown));
        let admitted = usize::from(matches!(result, SyscallResult::Ok(_)));
        println("P01:cleanup");
        println(&format!("P01-WORKLOAD {{\"status\":\"{}\",\"requested\":1,\"admitted\":{admitted},\"public_result\":\"{result:?}\"}}",
            if public_result_preserved { "DENIED_CONTROL" } else { "DENIED_CONTROL_PUBLIC_MISMATCH" }));
        println("P01:closing");
        syscall::sys_exit(if public_result_preserved { 0 } else { 1 })
    }
    let mut tids = [0usize; 8];
    let mut admitted = 0;
    let mut valid = true;
    for index in 0..requested {
        if !syscall::sys_set_spawn_args("parked-footprint-probe") {
            println("P01-NOT-RUN child-argv"); valid = false; break;
        }
        match syscall::sys_spawn_pinned("/bin/bench-probe", TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(tid) if tid != 0 && !tids[..admitted].contains(&tid) => {
                tids[index] = tid; admitted += 1;
                if !matches!(syscall::sys_notify_on_exit(tid), SyscallResult::Ok(0))
                    || !exchange(tid, READY, index as u8, deadline) {
                    valid = false; break;
                }
            }
            result => { println(&format!("P01-REFUSAL index={index} result={result:?}")); valid = false; break; }
        }
    }
    valid &= admitted == requested && all_live(&tids[..admitted]);
    if valid { println("P01:live"); }
    println("P01:cleanup");
    // Attempt release for every admitted exact child even after another failed.
    for (index, &tid) in tids[..admitted].iter().enumerate() {
        valid &= exchange(tid, RELEASE, index as u8, deadline);
    }
    let mut exited = [false; 8];
    let mut exit_count = 0;
    while exit_count < admitted && ticks() < deadline {
        let mut bytes = [0u8; 8];
        match syscall::sys_recv_timeout(0, &mut bytes, 1) {
            SyscallResult::Ok(0) => {}
            SyscallResult::Ok(sender) => {
                match tids[..admitted].iter().position(|tid| *tid == sender) {
                    Some(index) if !exited[index] && u64::from_ne_bytes(bytes) == 0 => {
                        exited[index] = true; exit_count += 1;
                    }
                    _ => { valid = false; break; }
                }
            }
            _ => { valid = false; break; }
        }
    }
    valid &= exit_count == admitted;
    if valid {
        for _ in 0..3 {
            if !matches!(syscall::sys_set_timer(1), SyscallResult::Ok(_)) { fail("settle-timer"); }
            println("P01:settle");
        }
    }
    println(&format!("P01-WORKLOAD {{\"status\":\"{}\",\"requested\":{requested},\"admitted\":{admitted},\"exited\":{exit_count}}}",
        if valid { "LIFECYCLE_COMPLETE" } else { "COUNT_OR_HANDSHAKE_FAILED" }));
    println("P01:closing");
    syscall::sys_exit(if valid { 0 } else { 1 })
}
