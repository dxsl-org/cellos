#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use ostd::{fs, io, syscall};

ostd::cell_main!(cell_main);

/// ls [path] — list kernel FS directory entries, one per line.
fn cell_main() {
    let output = io::ProcessOutput::read_own().unwrap_or_else(|_| syscall::sys_exit(1));
    let argv = ostd::args();
    let path = argv.first().map(|arg| arg.as_str()).unwrap_or("/");
    let path = if path.is_empty() { "/" } else { path };

    match fs::read_dir(path) {
        Ok(dir) => {
            for entry in dir {
                let name = core::str::from_utf8(&entry.name)
                    .unwrap_or("?")
                    .trim_matches('\0');
                if !name.is_empty() {
                    emit(output.stdout(), &[name.as_bytes(), b"\n"]);
                }
            }
        }
        Err(_) => {
            emit(output.stderr(), &[b"ls: ", path.as_bytes(), b": no such directory\n"]);
            syscall::sys_exit(1);
        }
    }
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
