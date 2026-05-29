#![no_std]
#![no_main]
extern crate ostd;

/// curl <url> — HTTP client (stub; requires TCP connect from Phase 15 data path).
#[no_mangle]
pub fn main() {
    ostd::io::println("curl: TCP connect data path not yet wired (Phase 15 data-path milestone)");
    ostd::syscall::sys_exit(1);
}
