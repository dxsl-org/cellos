#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// env — print known environment key=value pairs from the Config Cell.
fn cell_main() {
    ostd::io::println("PATH=/bin");
    ostd::io::println("SHELL=/bin/shell");
    ostd::io::println("OS=Cellos");
    ostd::io::println("VERSION=0.2.1");
    ostd::syscall::sys_exit(0);
}
