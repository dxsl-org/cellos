#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// uname [-a] — print system identification.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own()
        .unwrap_or_else(|_| ostd::syscall::sys_exit(1));
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => ostd::syscall::sys_exit(1),
    };
    for bytes in [
        ostd::system_info::OS_NAME.as_bytes(),
        b" ",
        ostd::system_info::KERNEL_NAME.as_bytes(),
        b" ",
        ostd::system_info::KERNEL_VERSION.as_bytes(),
        b" ",
        ostd::system_info::ARCH.as_bytes(),
        b"\n",
    ] {
        if output.stdout().write_all(bytes, deadline).is_err() {
            ostd::syscall::sys_exit(1);
        }
    }
    ostd::syscall::sys_exit(0);
}
