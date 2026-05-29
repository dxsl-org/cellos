#![no_std]
#![no_main]
extern crate ostd;

/// nc (netcat) <host> <port> — TCP/UDP relay (stub; Phase 15 data-path).
#[no_mangle]
pub fn main() {
    ostd::io::println("nc: TCP socket data path not yet wired (Phase 15 data-path milestone)");
    ostd::syscall::sys_exit(1);
}
