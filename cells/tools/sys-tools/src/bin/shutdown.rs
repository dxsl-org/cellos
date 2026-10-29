#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// shutdown — halt the system via the kernel's architecture-independent shutdown syscall.
///
/// Invokes syscall 502 (Shutdown) through ostd, which the kernel routes to:
/// - RISC-V: SBI SRST (System Reset Extension) ecall
/// - ARM64: PSCI SYSTEM_OFF
/// - x86_64: QEMU isa-debug-exit or ACPI S5
///
/// Never returns if the host accepts the shutdown request.
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
        .write_all(b"System shutting down...\n", deadline)
        .is_err()
    {
        ostd::syscall::sys_exit(1);
    }
    ostd::syscall::sys_shutdown();
}
