#![no_std]
#![no_main]
extern crate ostd;

/// uname [-a] — print system identification.
#[no_mangle]
pub fn main() {
    ostd::io::println("ViOS vios-kernel 0.2.1 riscv64 ViOS");
    ostd::syscall::sys_exit(0);
}
