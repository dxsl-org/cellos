#![no_std]
#![no_main]
extern crate ostd;

use ostd::{io, syscall};

/// echo [text...] — print arguments to stdout followed by a newline.
#[no_mangle]
pub fn main() {
    let argv = ostd::args();
    for (index, arg) in argv.iter().enumerate() {
        if index > 0 {
            io::print(" ");
        }
        io::print(arg);
    }
    io::println("");
    syscall::sys_exit(0);
}
