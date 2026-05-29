#![no_std]
#![no_main]
extern crate ostd;

/// mkdir standalone — use the shell built-in `mkdir` until Phase 17a arg-passing lands.
#[no_mangle]
pub fn main() {
    ostd::io::println("mkdir: use the shell built-in mkdir command");
    ostd::syscall::sys_exit(1);
}
