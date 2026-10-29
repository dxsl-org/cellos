#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use ostd::{io, syscall};

ostd::cell_main!(cell_main);

/// echo [text...] — print arguments to stdout followed by a newline.
fn cell_main() {
    let output = io::ProcessOutput::read_own().unwrap_or_else(|_| syscall::sys_exit(1));
    let argv = ostd::args();
    for (index, arg) in argv.iter().enumerate() {
        if index > 0 {
            emit(output.stdout(), b" ");
        }
        emit(output.stdout(), arg.as_bytes());
    }
    emit(output.stdout(), b"\n");
    syscall::sys_exit(0);
}

fn emit(stream: &io::OutputStream, bytes: &[u8]) {
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => syscall::sys_exit(1),
    };
    if stream.write_all(bytes, deadline).is_err() {
        syscall::sys_exit(1);
    }
}
