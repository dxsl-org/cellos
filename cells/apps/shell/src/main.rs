#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

// Phase 30: declare spawn capability so the kernel grants SpawnCap at spawn.
api::declare_manifest!(block_io = false, network = false, spawn = true);

mod aliases;
mod async_utils;
mod state_transfer;
mod cmd_fs;
mod cmd_sys;
mod commands;
mod config_client;
mod executor;
mod history;
mod jobs;
mod parser;
mod shell;

use shell::ViShell;

#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DEBUG: Shell Started (Async Mode)\n");
    let mut shell = ViShell::new();
    ostd::executor::block_on(shell.run());
}
