#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// env — print known environment key=value pairs from the Config Cell.
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
    if output
        .stdout()
        .write_all(b"PATH=/bin\nSHELL=/bin/cellbox\nOS=Cellos\nVERSION=0.2.1\n", deadline)
        .is_err()
    {
        ostd::syscall::sys_exit(1);
    }
    ostd::syscall::sys_exit(0);
}
