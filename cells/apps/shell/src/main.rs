#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod async_utils;
mod commands;
mod config_client;
mod shell;

use ostd::executor;
use shell::ViShell;

#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DEBUG: Shell Started (Async Mode)\n");
    let mut shell = ViShell::new();
    executor::block_on(shell.run());
}
