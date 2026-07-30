#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use ostd::{io, syscall};

ostd::cell_main!(cell_main);

/// echo [text...] — print arguments to stdout followed by a newline.
fn cell_main() {
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
