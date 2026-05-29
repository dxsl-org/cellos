#![no_std]
#![no_main]
extern crate ostd;
extern crate alloc;

use alloc::vec::Vec;
use ostd::syscall;

#[no_mangle]
pub fn main() {
    let n: usize = 10;
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
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    for line in &lines[start..] {
        ostd::io::println(line);
    }
    syscall::sys_exit(0);
}
