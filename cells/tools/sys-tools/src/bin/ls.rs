#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use ostd::{fs, io, syscall};

ostd::cell_main!(cell_main);

/// ls [path] — list kernel FS directory entries, one per line.
fn cell_main() {
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
                    io::println(name);
                }
            }
        }
        Err(_) => {
            io::print("ls: ");
            io::print(path);
            io::println(": no such directory");
            syscall::sys_exit(1);
        }
    }
    syscall::sys_exit(0);
}
