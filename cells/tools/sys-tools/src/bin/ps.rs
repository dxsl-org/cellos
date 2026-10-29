#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;
use ostd::syscall;

ostd::cell_main!(cell_main);

/// List running tasks from the kernel task table.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own()
        .unwrap_or_else(|_| syscall::sys_exit(1));
    let mut buf = [api::syscall::ProcessInfo::default(); 32];
    match syscall::sys_get_procs(&mut buf) {
        Ok(count) => {
            emit(output.stdout(), &[b"PID   STATE     NAME\n------------------------\n"]);
            for info in buf.iter().take(count) {
                let name = core::str::from_utf8(&info.name)
                    .unwrap_or("?")
                    .trim_matches('\0');
                let state = match info.state {
                    0 => "Ready  ",
                    1 => "Running",
                    2 => "Waiting",
                    3 => "Dead   ",
                    _ => "?      ",
                };
                let mut id_buf = [0u8; 20];
                let id = decimal(info.id as u64, &mut id_buf);
                emit(output.stdout(), &[id, b"  ", state.as_bytes(), b"  ", name.as_bytes(), b"\n"]);
            }
        }
        Err(_) => emit(output.stderr(), &[b"ps: cannot read process table\n"]),
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

fn emit(stream: &ostd::io::OutputStream, parts: &[&[u8]]) {
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
