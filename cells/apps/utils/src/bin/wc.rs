#![no_std]
#![no_main]
extern crate ostd;
extern crate alloc;

use alloc::vec::Vec;
use ostd::syscall;

#[no_mangle]
pub fn main() {
    // Standalone wc counts lines/words/bytes of /dev/stdin (fd 0).
    // When invoked by the shell via exec, args are not yet available (Phase 17a).
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let bytes = data.len();
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    let words = data.split(|b| b == &b' ' || b == &b'\n' || b == &b'\t')
        .filter(|w| !w.is_empty()).count();
    ostd::io::print_usize(lines);
    ostd::io::print(" ");
    ostd::io::print_usize(words);
    ostd::io::print(" ");
    ostd::io::print_usize(bytes);
    ostd::io::println("");
    syscall::sys_exit(0);
}
