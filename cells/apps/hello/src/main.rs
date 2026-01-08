#![no_std]
#![no_main]

extern crate ostd;

use ostd::prelude::*;

#[no_mangle]
pub fn main() {
    ostd::io::println("Hello form separate ELF!");
    ostd::syscall::sys_exit(0);
}
