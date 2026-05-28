#![no_std]
#![no_main]

extern crate ostd;

#[no_mangle]
pub fn main() {
    ostd::io::println("Cat (External App): Not fully implemented args yet.");
    ostd::syscall::sys_exit(0);
}
