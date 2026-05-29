#![no_std]
#![no_main]
extern crate ostd;
extern crate alloc;

use alloc::vec::Vec;
use ostd::syscall;

/// sort — read stdin lines, sort them lexicographically, write to stdout.
#[no_mangle]
pub fn main() {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let text = core::str::from_utf8(&data).unwrap_or("");
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    for line in lines {
        ostd::io::println(line);
    }
    syscall::sys_exit(0);
}
