#![no_std]
#![no_main]

extern crate ostd;

use ostd::io::println;

#[no_mangle]
pub extern "C" fn main() {
    println("Init: Starting...");
    
    // Check math (sanity)
    if 2 + 2 == 4 {
        println("Init: Math ok.");
    }

    // Spawn Shell
    println("Init: Spawning /SHELL...");
    match ostd::syscall::sys_exec("/SHELL") {
        ostd::syscall::SyscallResult::Ok(tid) => {
            println("Init: Shell spawned successfully.");
        },
        ostd::syscall::SyscallResult::Err(e) => {
            println("Init: Failed to spawn shell.");
        }
    }

    // Keep init alive
    loop {
        ostd::task::yield_now();
    }
}
