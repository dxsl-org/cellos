#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

use ostd::syscall::{SyscallError, SyscallResult};

api::declare_syscalls![Log, SpawnFromPath, Exit];

ostd::cell_main!(cell_main);

fn cell_main() {
    match ostd::syscall::sys_mem_info() {
        Err(_) => ostd::io::println("[a2a3-probe] MEMINFO_DENIED"),
        Ok(_) => {
            ostd::io::println("[a2a3-probe] MEMINFO_UNEXPECTEDLY_ALLOWED");
            ostd::syscall::sys_exit(2);
        }
    }

    for count in 0..128 {
        match ostd::syscall::sys_spawn_from_path("/bin/robot-dashboard") {
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(SyscallError::OutOfMemory) => {
                ostd::io::print("[a2a3-probe] OOM_TYPED count=");
                ostd::io::print_usize(count);
                ostd::io::println("");
                ostd::syscall::sys_exit(0);
            }
            SyscallResult::Err(_) => {
                ostd::io::println("[a2a3-probe] SPAWN_GENERIC_ERROR");
                ostd::syscall::sys_exit(3);
            }
        }
    }

    ostd::io::println("[a2a3-probe] OOM_NOT_REACHED");
    ostd::syscall::sys_exit(4);
}
