#![no_std]
#![no_main]
extern crate ostd;

/// kill <pid> — terminate a task (stub; kernel task termination syscall deferred).
#[no_mangle]
pub fn main() {
    ostd::io::println("kill: task termination syscall not yet wired (Phase 20.5)");
    ostd::syscall::sys_exit(1);
}
