#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// uname [-a] — print system identification.
fn cell_main() {
    ostd::io::println("ViCell vicell-kernel 0.2.1 riscv64 ViCell");
    ostd::syscall::sys_exit(0);
}
