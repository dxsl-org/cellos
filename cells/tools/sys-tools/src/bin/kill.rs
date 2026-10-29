#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use api::syscall::ProcessInfo;
use ostd::{io, syscall};

ostd::cell_main!(cell_main);

/// kill <tid> — send cooperative shutdown signal or force-exit a task.
///
/// Cooperative (state=Waiting): sends 0xFF via sys_send; the task's recv loop
/// checks for this sentinel and exits cleanly.
/// Force (state=Ready/Running): calls sys_force_exit; the kernel terminates the
/// task immediately.  System Cells with SpawnCap may reject force-exit.
fn cell_main() {
    let output = io::ProcessOutput::read_own().unwrap_or_else(|_| syscall::sys_exit(1));
    let argv = ostd::args();
    let tid_str = argv.first().map(|arg| arg.as_str()).unwrap_or("");

    if tid_str.is_empty() {
        emit(output.stderr(), &[b"usage: kill <tid>\n"]);
        syscall::sys_exit(1);
    }

    let tid: usize = match tid_str.parse() {
        Ok(n) => n,
        Err(_) => {
            emit(output.stderr(), &[b"kill: invalid tid: ", tid_str.as_bytes(), b"\n"]);
            syscall::sys_exit(1);
        }
    };
    let mut tid_buf = [0u8; 20];
    let tid_bytes = decimal(tid as u64, &mut tid_buf);

    // Read process table to determine the target task's state.
    let mut buf = [ProcessInfo::default(); 32];
    let count = syscall::sys_get_procs(&mut buf).unwrap_or(0);

    let state = (0..count).find(|&i| buf[i].id == tid).map(|i| buf[i].state);

    match state {
        Some(2) => {
            // Task is blocked in a recv call — send cooperative shutdown sentinel.
            let _ = syscall::sys_send(tid, &[0xFF_u8]);
            emit(output.stdout(), &[b"kill: sent shutdown signal to ", tid_bytes, b"\n"]);
        }
        Some(0) | Some(1) => {
            // Task is Ready or Running — force terminate.
            let _ = syscall::sys_force_exit(tid);
            emit(output.stdout(), &[b"kill: force-exited ", tid_bytes, b"\n"]);
        }
        Some(3) => {
            emit(output.stdout(), &[b"kill: task ", tid_bytes, b" is already terminated\n"]);
        }
        _ => {
            emit(output.stderr(), &[b"kill: task not found: ", tid_bytes, b"\n"]);
            syscall::sys_exit(1);
        }
    }

    syscall::sys_exit(0);
}

fn decimal(mut value: u64, buffer: &mut [u8; 20]) -> &[u8] {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    &buffer[cursor..]
}

fn emit(stream: &io::OutputStream, parts: &[&[u8]]) {
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => syscall::sys_exit(1),
    };
    for bytes in parts {
        if stream.write_all(bytes, deadline).is_err() {
            syscall::sys_exit(1);
        }
    }
}
