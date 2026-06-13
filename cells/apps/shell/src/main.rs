#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

// Declares spawn capability; the kernel grants SpawnCap at spawn.
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

#[cfg(feature = "shell_test")]
mod shell_test;

use shell::ViShell;

#[no_mangle]
pub fn main() {
    #[cfg(feature = "shell_test")]
    shell_test::run();

    #[cfg(not(feature = "shell_test"))]
    {
        let _ = ostd::syscall::sys_log("DEBUG: Shell Started (Async Mode)\n");
        let mut shell = ViShell::new();
        ostd::executor::block_on(shell.run());
    }
}
