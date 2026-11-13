#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use abi::syscall::service;
use ostd::ipc::IpcError;
use ostd::service::provider_tid;
use ostd::syscall;
use wire::proto::app_envelope::{HEADER_LEN, KIND_MESSAGE, MAGIC};
use wire::proto::supervisor::{
    HotswapError, ELF_PATH_LEN, HOTSWAP_REQUEST_LEN, OP_HOTSWAP, OP_STATUS, PHASE_HOTSWAP_DONE,
    PHASE_HOTSWAP_ERROR, PHASE_REFUSED, STATUS_INVALID_REQUEST, STATUS_OK, STATUS_REJECTED_CALLER,
    STATUS_REPLY_LEN, STATUS_SERVICE_NOT_FOUND, SVC_NAME_LEN,
};

ostd::cell_main!(cell_main);

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
/// restore/publish sequence rather than a single IPC hop; it also bounds
/// admission of the request.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own().unwrap_or_else(|_| syscall::sys_exit(1));
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

    let Some(supervisor_tid) = provider_tid(service::SUPERVISOR) else {
        exit_with_message(output.stderr(), "hotswap: supervisor unavailable");
    };

    match request_status(supervisor_tid, &request) {
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
) -> Result<[u8; HOTSWAP_REQUEST_LEN], &'static str> {
    if service_name.is_empty() || new_path.is_empty() {
        return Err(USAGE);
    }

    validate_service_name(service_name)?;
    validate_elf_path(new_path)?;

    let mut request = [0u8; HOTSWAP_REQUEST_LEN];
    request[0] = OP_HOTSWAP;
    let service_bytes = service_name.as_bytes();
    request[1..1 + service_bytes.len()].copy_from_slice(service_bytes);
    let path_bytes = new_path.as_bytes();
    request[1 + SVC_NAME_LEN..1 + SVC_NAME_LEN + path_bytes.len()].copy_from_slice(path_bytes);
    Ok(request)
}

/// One hotswap exchange: the supervisor answers each request with exactly one
/// status record.
fn request_status(
    supervisor_tid: usize,
    request: &[u8; HOTSWAP_REQUEST_LEN],
) -> Result<(), StatusError> {
    let mut envelope = [0u8; HEADER_LEN + HOTSWAP_REQUEST_LEN];
    envelope[..HEADER_LEN].copy_from_slice(&[MAGIC, KIND_MESSAGE]);
    envelope[HEADER_LEN..].copy_from_slice(request);
    let mut status = [0u8; STATUS_REPLY_LEN];
    match ostd::ipc::call_bytes(supervisor_tid, &envelope, &mut status, REPLY_TIMEOUT_TICKS) {
        Ok(reply) => match <[u8; STATUS_REPLY_LEN]>::try_from(reply) {
            Ok(status) => parse_status(status),
            Err(_) => Err(StatusError::Message(MALFORMED_REPLY)),
        },
        Err(IpcError::Send | IpcError::Busy | IpcError::PeerGone) => Err(StatusError::Message(
            "hotswap: cannot send request to supervisor",
        )),
        Err(IpcError::PreDispatchTimeout | IpcError::Indeterminate) => Err(StatusError::Message(
            "hotswap: timed out waiting for supervisor status",
        )),
        Err(_) => Err(StatusError::Message(
            "hotswap: protocol error: recv from supervisor failed",
        )),
    }
}

fn parse_status(status: [u8; STATUS_REPLY_LEN]) -> Result<(), StatusError> {
    if status[0] != OP_STATUS {
        return Err(StatusError::Message(MALFORMED_REPLY));
    }

    match status {
        [OP_STATUS, PHASE_HOTSWAP_DONE, STATUS_OK] => Ok(()),
        [OP_STATUS, PHASE_REFUSED, STATUS_INVALID_REQUEST] => Err(StatusError::Message(
            "hotswap: protocol error: supervisor rejected request framing",
        )),
        [OP_STATUS, PHASE_REFUSED, STATUS_SERVICE_NOT_FOUND] => Err(StatusError::Message(
            "hotswap: target service is not registered with the supervisor",
        )),
        [OP_STATUS, PHASE_REFUSED, STATUS_REJECTED_CALLER] => Err(StatusError::Message(
            "hotswap: supervisor only accepts requests from the shell-launched hotswap CLI",
        )),
        [OP_STATUS, PHASE_HOTSWAP_ERROR, code] => {
            Err(StatusError::Message(supervisor_error_message(code)))
        }
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
    let Some(error) = HotswapError::from_u8(code) else {
        return "hotswap: supervisor returned an unknown error code";
    };
    match error {
        HotswapError::ServiceNotFound => "hotswap: supervisor could not find the target service",
        HotswapError::FreezeFailed => "hotswap: supervisor could not freeze the target service",
        HotswapError::SnapshotIpcFailed => {
            "hotswap: supervisor could not deliver the snapshot request"
        }
        HotswapError::SnapshotTimeout => "hotswap: supervisor timed out waiting for the snapshot",
        HotswapError::SpawnFailed => "hotswap: supervisor could not spawn the replacement ELF",
        HotswapError::RestoreIpcFailed => {
            "hotswap: supervisor could not deliver the restore request"
        }
        HotswapError::ReadyTimeout => {
            "hotswap: supervisor timed out waiting for the replacement to become ready"
        }
        HotswapError::PauseFailed => "hotswap: supervisor could not pause the service for cutover",
        HotswapError::RegisterFailed => {
            "hotswap: supervisor could not publish the replacement service"
        }
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

/// Write `parts` as one record when they fit in a line buffer, so a kernel log
/// line cannot land inside the message and split it on the console.
fn emit(stream: &ostd::io::OutputStream, parts: &[&[u8]]) {
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => syscall::sys_exit(1),
    };
    let mut line = [0u8; 512];
    let mut len = 0;
    for bytes in parts {
        match line.get_mut(len..len + bytes.len()) {
            Some(slot) => {
                slot.copy_from_slice(bytes);
                len += bytes.len();
            }
            None => {
                len = usize::MAX;
                break;
            }
        }
    }
    let written = if len == usize::MAX {
        parts
            .iter()
            .try_for_each(|bytes| stream.write_all(bytes, deadline))
    } else {
        stream.write_all(&line[..len], deadline)
    };
    if written.is_err() {
        syscall::sys_exit(1);
    }
}
