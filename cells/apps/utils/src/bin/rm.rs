#![no_std]
#![no_main]
extern crate ostd;

/// rm standalone — use the shell built-in `rm` (cmd_fs) until Phase 17a arg-passing lands.
#[no_mangle]
pub fn main() {
    ostd::io::println("rm: use the shell built-in rm command");
    ostd::syscall::sys_exit(1);
}
