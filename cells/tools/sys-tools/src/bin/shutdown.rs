#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// shutdown [-r] — power the system off, or with `-r` (x86_64) reboot it.
///
/// - x86_64: asks the Supervisor Cell (`OP_POWER`), which freezes the other
///   Cells, waits a bounded settle window and calls `SystemPower` (436). The
///   worker returns only when the supervisor refused or did not act.
/// - RISC-V / ARM64: raw syscall 502 (SBI SRST / PSCI SYSTEM_OFF); arguments
///   are ignored there.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own().unwrap_or_else(|_| ostd::syscall::sys_exit(1));
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => ostd::syscall::sys_exit(1),
    };
    #[cfg(target_arch = "x86_64")]
    let action = match ostd::args().as_slice() {
        [] => ostd::syscall::SystemPowerAction::PowerOff,
        [flag] if flag == "-r" => ostd::syscall::SystemPowerAction::Reboot,
        _ => exit_with(&output, b"usage: shutdown [-r]\n", deadline),
    };
    #[cfg(target_arch = "x86_64")]
    let banner: &[u8] = match action {
        ostd::syscall::SystemPowerAction::PowerOff => b"System shutting down...\n",
        ostd::syscall::SystemPowerAction::Reboot => b"System rebooting...\n",
    };
    #[cfg(not(target_arch = "x86_64"))]
    let banner: &[u8] = b"System shutting down...\n";
    if output.stdout().write_all(banner, deadline).is_err() {
        ostd::syscall::sys_exit(1);
    }
    // `write_all` returns once the bytes are in the session's stdout ring; the
    // shell relays that ring to the console only when it next runs. Powering off
    // at once would drop the message, so give the relay a bounded window first.
    if let Ok(now) = ostd::session::monotonic_ns() {
        let until = now.saturating_add(RELAY_WINDOW_NS);
        while ostd::session::monotonic_ns().is_ok_and(|now| now < until) {
            ostd::task::yield_now();
        }
    }
    #[cfg(target_arch = "x86_64")]
    request_from_supervisor(action, &output, deadline);
    #[cfg(not(target_arch = "x86_64"))]
    ostd::syscall::sys_shutdown();
}

/// How long the shutdown message gets to reach the console before power-off.
const RELAY_WINDOW_NS: u64 = 300_000_000;

/// Supervisor-private `PowerRequest` (`cells/services/supervisor/src/protocol.rs`)
/// in the App SDK envelope; the action byte is the 436 a0 value.
#[cfg(target_arch = "x86_64")]
const APP_MESSAGE_PREFIX: [u8; 2] = [0xAC, 0x00];
#[cfg(target_arch = "x86_64")]
const OP_POWER: u8 = 0x04;
#[cfg(target_arch = "x86_64")]
const OP_STATUS: u8 = 0x03;
/// Covers the supervisor's freeze sweep and settle window with a wide margin.
#[cfg(target_arch = "x86_64")]
const REPLY_TIMEOUT_TICKS: u64 = 3_000;

/// Send the request and wait. A successful request freezes this worker and
/// resets or powers off the machine, so any return here is a failure.
#[cfg(target_arch = "x86_64")]
fn request_from_supervisor(
    action: ostd::syscall::SystemPowerAction,
    output: &ostd::io::ProcessOutput,
    deadline: u64,
) -> ! {
    use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_send, SyscallResult};
    let Some(supervisor) = sys_lookup_service(api::syscall::service::SUPERVISOR) else {
        exit_with(output, b"shutdown: supervisor unavailable\n", deadline);
    };
    let action = match action {
        ostd::syscall::SystemPowerAction::Reboot => api::syscall::system_power::ACTION_REBOOT,
        ostd::syscall::SystemPowerAction::PowerOff => api::syscall::system_power::ACTION_POWEROFF,
    };
    let request = [APP_MESSAGE_PREFIX[0], APP_MESSAGE_PREFIX[1], OP_POWER, action as u8];
    if !matches!(sys_send(supervisor, &request), SyscallResult::Ok(_)) {
        exit_with(output, b"shutdown: cannot send request to supervisor\n", deadline);
    }
    let mut status = [0u8; 3];
    match sys_recv_timeout(supervisor, &mut status, REPLY_TIMEOUT_TICKS) {
        SyscallResult::Ok(sender) if sender == supervisor && status[0] == OP_STATUS => exit_with(
            output,
            b"shutdown: refused; the supervisor resumed the system\n",
            deadline,
        ),
        _ => exit_with(output, b"shutdown: the machine did not power off or reset\n", deadline),
    }
}

#[cfg(target_arch = "x86_64")]
fn exit_with(output: &ostd::io::ProcessOutput, message: &[u8], deadline: u64) -> ! {
    let _ = output.stderr().write_all(message, deadline);
    ostd::syscall::sys_exit(1)
}
