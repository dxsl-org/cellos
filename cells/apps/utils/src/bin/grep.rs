#![no_std]
#![no_main]
extern crate ostd;
extern crate alloc;

use alloc::vec::Vec;
use ostd::syscall;

/// Minimal grep: reads stdin, prints lines containing the pattern.
/// Pattern is sent as the first 64 bytes of a setup IPC message (Phase 17a).
/// Until then, prints all lines (effectively cat) as a safe no-crash stub.
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
    // TODO (Phase 17a): receive pattern via arg IPC before reading stdin.
    let text = core::str::from_utf8(&data).unwrap_or("");
    for line in text.lines() {
        ostd::io::println(line);
    }
    syscall::sys_exit(0);
}
