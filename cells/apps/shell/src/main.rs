#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod shell;
mod commands;

use shell::ViShell;

#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DEBUG: Shell Started\n");

    let shell = ViShell::new();
    shell.run();
}
