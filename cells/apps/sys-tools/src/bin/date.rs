#![no_std]
#![no_main]
extern crate ostd;

/// date — print current uptime as a proxy for system time (RTC not yet wired).
#[no_mangle]
pub fn main() {
    let ticks = ostd::syscall::sys_get_time();
    let secs  = ticks / 10_000_000; // 10 MHz mtime
    let mins  = secs / 60;
    let hrs   = mins / 60;
    ostd::io::print("Uptime: ");
    ostd::io::print_usize(hrs as usize);
    ostd::io::print("h ");
    ostd::io::print_usize((mins % 60) as usize);
    ostd::io::print("m ");
    ostd::io::print_usize((secs % 60) as usize);
    ostd::io::println("s  (RTC not yet wired — shows uptime only)");
    ostd::syscall::sys_exit(0);
}
