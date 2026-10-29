#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use ostd::{io, syscall};

ostd::cell_main!(cell_main);

/// cat <path> — print file content from the kernel FS to stdout.
fn cell_main() {
    let output = io::ProcessOutput::read_own().unwrap_or_else(|_| syscall::sys_exit(1));
    let argv = ostd::args();
    let path = argv.first().map(|arg| arg.as_str()).unwrap_or("");

    if path.is_empty() {
        emit(output.stderr(), &[b"usage: cat <path>\n"]);
        syscall::sys_exit(1);
    }

    let fd = match syscall::sys_open(path) {
        Ok(f) => f,
        Err(_) => {
            emit(output.stderr(), &[b"cat: ", path.as_bytes(), b": file not found\n"]);
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => emit(output.stdout(), &[&buf[..n]]),
            Err(_) => break,
        }
    }
    syscall::sys_close(fd);
    syscall::sys_exit(0);
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
