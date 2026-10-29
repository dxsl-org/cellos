#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use api::syscall::service;
use ostd::syscall::{self, sys_lookup_service, sys_recv_timeout, sys_send, SyscallResult};

ostd::cell_main!(cell_main);

const APP_MESSAGE_PREFIX: [u8; 2] = [0xAC, 0x00];
const OP_HOTSWAP: u8 = 0x01;
const OP_STATUS: u8 = 0x03;
const SVC_NAME_LEN: usize = 64;
const ELF_PATH_LEN: usize = 128;
const REQUEST_LEN: usize = 1 + SVC_NAME_LEN + ELF_PATH_LEN;
const STATUS_LEN: usize = 3;
const SUCCESS_PHASE: u8 = 6;
const SUCCESS_CODE: u8 = 0;
const REPLY_TIMEOUT_TICKS: u64 = 1_600;
const USAGE: &str = "hotswap: usage: hotswap <service-name> <new-elf-path>";
const EXACT_BIN_TARGET_ERROR: &str = "hotswap: ELF path must be an exact /bin/<name> target";
const MALFORMED_REPLY: &str = "hotswap: protocol error: malformed supervisor reply";

/// hotswap <service-name> <new-elf-path> - ask the Supervisor Cell to replace a
/// registered service.
///
/// Phase 02 keeps this CLI service-only on purpose: the supervisor routes
/// hotswap requests by registered `service::*` id, not arbitrary tids.
/// The reply timeout spans the supervisor's bounded snapshot/freeze/spawn/
/// restore/publish sequence rather than a single IPC hop.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own()
        .unwrap_or_else(|_| syscall::sys_exit(1));
    let argv = ostd::args();
    let [service_name, new_path] = argv.as_slice() else {
        exit_with_message(output.stderr(), USAGE);
    };
    let service_name = service_name.as_str();
    let new_path = new_path.as_str();

    let request = match encode_hotswap_request(service_name, new_path) {
        Ok(request) => request,
        Err(message) => exit_with_message(output.stderr(), message),
    };

    let Some(supervisor_tid) = sys_lookup_service(service::SUPERVISOR) else {
        exit_with_message(output.stderr(), "hotswap: supervisor unavailable");
    };

    if !send_request(supervisor_tid, &request) {
        exit_with_message(output.stderr(), "hotswap: cannot send request to supervisor");
    }

    match recv_status(supervisor_tid) {
        Ok(()) => {
            emit(
                output.stdout(),
                &[
                    b"hotswap: success: supervisor swapped service '",
                    service_name.as_bytes(),
                    b"' to ",
                    new_path.as_bytes(),
                    b"\n",
                ],
            );
            syscall::sys_exit(0);
        }
        Err(StatusError::Message(message)) => exit_with_message(output.stderr(), message),
        Err(StatusError::UnexpectedStatus { phase, code }) => {
            let mut phase_buf = [0u8; 3];
            let mut code_buf = [0u8; 3];
            emit(
                output.stderr(),
                &[
                    b"hotswap: supervisor returned unexpected status phase=",
                    decimal(phase, &mut phase_buf),
                    b" result=",
                    decimal(code, &mut code_buf),
                    b"\n",
                ],
            );
            syscall::sys_exit(1);
        }
    }
}

fn encode_hotswap_request(
    service_name: &str,
    new_path: &str,
) -> Result<[u8; REQUEST_LEN], &'static str> {
    if service_name.is_empty() || new_path.is_empty() {
        return Err(USAGE);
    }

    validate_service_name(service_name)?;
    validate_elf_path(new_path)?;

    let mut request = [0u8; REQUEST_LEN];
    request[0] = OP_HOTSWAP;
    let service_bytes = service_name.as_bytes();
    request[1..1 + service_bytes.len()].copy_from_slice(service_bytes);
    let path_bytes = new_path.as_bytes();
    request[1 + SVC_NAME_LEN..1 + SVC_NAME_LEN + path_bytes.len()].copy_from_slice(path_bytes);
    Ok(request)
}

fn send_request(supervisor_tid: usize, request: &[u8; REQUEST_LEN]) -> bool {
    let mut envelope = [0u8; APP_MESSAGE_PREFIX.len() + REQUEST_LEN];
    envelope[..APP_MESSAGE_PREFIX.len()].copy_from_slice(&APP_MESSAGE_PREFIX);
    envelope[APP_MESSAGE_PREFIX.len()..].copy_from_slice(request);
    matches!(sys_send(supervisor_tid, &envelope), SyscallResult::Ok(_))
}

fn recv_status(supervisor_tid: usize) -> Result<(), StatusError> {
    let mut status = [0u8; STATUS_LEN];
    match sys_recv_timeout(supervisor_tid, &mut status, REPLY_TIMEOUT_TICKS) {
        SyscallResult::Ok(0) => Err(StatusError::Message(
            "hotswap: timed out waiting for supervisor status",
        )),
        SyscallResult::Ok(sender) if sender != supervisor_tid => Err(StatusError::Message(
            "hotswap: protocol error: reply came from an unexpected sender",
        )),
        SyscallResult::Ok(_) => parse_status(status),
        SyscallResult::Err(_) => Err(StatusError::Message(
            "hotswap: protocol error: recv from supervisor failed",
        )),
    }
}

fn parse_status(status: [u8; STATUS_LEN]) -> Result<(), StatusError> {
    if status[0] != OP_STATUS {
        return Err(StatusError::Message(MALFORMED_REPLY));
    }

    match status {
        [OP_STATUS, SUCCESS_PHASE, SUCCESS_CODE] => Ok(()),
        [OP_STATUS, 0, 0xFF] => Err(StatusError::Message(
            "hotswap: protocol error: supervisor rejected request framing",
        )),
        [OP_STATUS, 0, 0xFE] => Err(StatusError::Message(
            "hotswap: target service is not registered with the supervisor",
        )),
        [OP_STATUS, 0, 0xFD] => Err(StatusError::Message(
            "hotswap: supervisor only accepts requests from the shell-launched hotswap CLI",
        )),
        [OP_STATUS, 0xFF, code] => Err(StatusError::Message(supervisor_error_message(code))),
        [OP_STATUS, phase, code] => Err(StatusError::UnexpectedStatus { phase, code }),
        _ => Err(StatusError::Message(MALFORMED_REPLY)),
    }
}

fn validate_service_name(service_name: &str) -> Result<(), &'static str> {
    let bytes = service_name.as_bytes();
    if bytes.contains(&0) || bytes.len() >= SVC_NAME_LEN {
        return Err("hotswap: service name must fit in 63 bytes plus trailing NUL");
    }
    if !bytes.is_ascii()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    {
        return Err("hotswap: service name must be ASCII letters, digits, or '-'");
    }
    Ok(())
}

fn validate_elf_path(path: &str) -> Result<(), &'static str> {
    let bytes = path.as_bytes();
    if bytes.contains(&0) || bytes.len() >= ELF_PATH_LEN {
        return Err("hotswap: ELF path must fit in 127 bytes plus trailing NUL");
    }
    let Some(suffix) = path.strip_prefix("/bin/") else {
        return Err(EXACT_BIN_TARGET_ERROR);
    };
    if suffix.is_empty() || suffix.contains('/') || suffix.contains("..") || path.contains("//") {
        return Err(EXACT_BIN_TARGET_ERROR);
    }
    if !bytes.is_ascii() {
        return Err("hotswap: ELF path must be ASCII");
    }
    Ok(())
}

fn supervisor_error_message(code: u8) -> &'static str {
    match code {
        0 => "hotswap: supervisor could not find the target service",
        1 => "hotswap: supervisor could not freeze the target service",
        2 => "hotswap: supervisor could not deliver the snapshot request",
        3 => "hotswap: supervisor timed out waiting for the snapshot",
        4 => "hotswap: supervisor could not spawn the replacement ELF",
        5 => "hotswap: supervisor could not deliver the restore request",
        6 => "hotswap: supervisor timed out waiting for the replacement to become ready",
        7 => "hotswap: supervisor could not pause the service for cutover",
        8 => "hotswap: supervisor could not publish the replacement service",
        _ => "hotswap: supervisor returned an unknown error code",
    }
}

fn exit_with_message(stream: &ostd::io::OutputStream, message: &str) -> ! {
    emit(stream, &[message.as_bytes(), b"\n"]);
    syscall::sys_exit(1)
}

enum StatusError {
    Message(&'static str),
    UnexpectedStatus { phase: u8, code: u8 },
}

fn decimal(mut value: u8, buffer: &mut [u8; 3]) -> &[u8] {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + value % 10;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    &buffer[cursor..]
}

fn emit(stream: &ostd::io::OutputStream, parts: &[&[u8]]) {
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => syscall::sys_exit(1),
    };
    for bytes in parts {
        if stream.write_all(bytes, deadline).is_err() {
            syscall::sys_exit(1);
        }
    }
}
