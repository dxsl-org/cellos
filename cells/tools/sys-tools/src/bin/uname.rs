#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;
extern crate ostd;

use alloc::format;

ostd::cell_main!(cell_main);

/// uname [-a] — print system identification.
fn cell_main() {
    let info = format!(
        "{} {} {} {}",
        ostd::system_info::OS_NAME,
        ostd::system_info::KERNEL_NAME,
        ostd::system_info::KERNEL_VERSION,
        ostd::system_info::ARCH,
    );
    ostd::io::println(&info);
    ostd::syscall::sys_exit(0);
}
