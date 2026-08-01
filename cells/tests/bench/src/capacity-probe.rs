#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use api::task::TaskPriority;
use ostd::syscall::{SyscallError, SyscallResult};

api::declare_syscalls![Log, SpawnPinned, StateStash, StateRestore, Exit, Yield];
api::declare_manifest!(block_io = false, network = false, spawn = true);

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
        ostd::syscall::sys_set_spawn_args("resp-echo");
        match ostd::syscall::sys_spawn_pinned("/bin/bench-probe", TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(SyscallError::OutOfMemory) => {
                ostd::io::println(&alloc::format!("[a2a3-probe] OOM_TYPED count={count}"));
                ostd::syscall::sys_exit(0);
            }
            SyscallResult::Err(_) => {
                ostd::io::println("[a2a3-probe] SPAWN_GENERIC_ERROR");
                ostd::syscall::sys_exit(3);
            }
        }
    }

    ostd::io::println("[a2a3-probe] OOM_NOT_REACHED");
    ostd::syscall::sys_exit(4)
}
