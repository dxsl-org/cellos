#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod shell;
mod commands;
mod async_utils;
mod config_client;

use shell::ViShell;
use ostd::executor;

#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DEBUG: Shell Started (Async Mode)\n");

    let shell = ViShell::new();

    // Execute the async shell using the block_on executor
    executor::block_on(async {
        shell.run().await;
    });
}
