//! Shell client for the Supervisor-owned snapshot trigger.

use api::ipc::IPC_BUF_SIZE;
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_send, SyscallResult};

const APP_MESSAGE_PREFIX: [u8; 2] = [0xAC, 0x00];
const OP_SNAPSHOT: u8 = 0x02;
const OP_STATUS: u8 = 0x03;
const STATUS_LEN: usize = 3;
const SNAPSHOT_STATUS_PHASE: u8 = 1;
const STATUS_OK: u8 = 0x00;
const STATUS_UNAVAILABLE: u8 = 0x01;
const STATUS_REJECTED_CALLER: u8 = 0xFD;
const STATUS_INVALID_REQUEST: u8 = 0xFF;
const REPLY_TIMEOUT_TICKS: u64 = 1_600;

/// Ask the Supervisor Cell to trigger a snapshot and print the bounded result.
pub fn run() -> i32 {
    let Some(supervisor_tid) = sys_lookup_service(service::SUPERVISOR) else {
        println("snapshot: supervisor unavailable");
        return 1;
    };

    let request = encode_request();
    if !matches!(sys_send(supervisor_tid, &request), SyscallResult::Ok(_)) {
        println("snapshot: cannot send request to supervisor");
        return 1;
    }

    let mut status = [0u8; STATUS_LEN];
    match sys_recv_timeout(supervisor_tid, &mut status, REPLY_TIMEOUT_TICKS) {
        SyscallResult::Ok(0) => {
            println("snapshot: timed out waiting for supervisor status");
            1
        }
        SyscallResult::Ok(sender) if sender != supervisor_tid => {
            println("snapshot: protocol error: reply came from an unexpected sender");
            1
        }
        SyscallResult::Ok(_) => match parse_status(status) {
            SnapshotOutcome::Success => {
                println("snapshot: success: snapshot saved; reboot for warm boot");
                0
            }
            SnapshotOutcome::Unavailable => {
                println("snapshot: unavailable on this platform");
                1
            }
            SnapshotOutcome::ProtocolError(message) => {
                println(message);
                1
            }
        },
        SyscallResult::Err(_) => {
            println("snapshot: protocol error: recv from supervisor failed");
            1
        }
    }
}

enum SnapshotOutcome {
    Success,
    Unavailable,
    ProtocolError(&'static str),
}

/// Build a full zeroed App envelope so the supervisor's reused receive buffer
/// is deterministically overwritten beyond the opcode byte.
fn encode_request() -> [u8; IPC_BUF_SIZE] {
    let mut request = [0u8; IPC_BUF_SIZE];
    request[..APP_MESSAGE_PREFIX.len()].copy_from_slice(&APP_MESSAGE_PREFIX);
    request[APP_MESSAGE_PREFIX.len()] = OP_SNAPSHOT;
    request
}

fn parse_status(status: [u8; STATUS_LEN]) -> SnapshotOutcome {
    match status {
        [OP_STATUS, SNAPSHOT_STATUS_PHASE, STATUS_OK] => SnapshotOutcome::Success,
        [OP_STATUS, SNAPSHOT_STATUS_PHASE, STATUS_UNAVAILABLE] => SnapshotOutcome::Unavailable,
        [OP_STATUS, 0, STATUS_REJECTED_CALLER] => {
            SnapshotOutcome::ProtocolError("snapshot: supervisor rejected the caller identity")
        }
        [OP_STATUS, 0, STATUS_INVALID_REQUEST] => SnapshotOutcome::ProtocolError(
            "snapshot: protocol error: supervisor rejected request framing",
        ),
        [OP_STATUS, ..] => {
            SnapshotOutcome::ProtocolError("snapshot: protocol error: unexpected supervisor status")
        }
        _ => SnapshotOutcome::ProtocolError("snapshot: protocol error: malformed supervisor reply"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        encode_request, parse_status, SnapshotOutcome, APP_MESSAGE_PREFIX, OP_SNAPSHOT, OP_STATUS,
        SNAPSHOT_STATUS_PHASE, STATUS_OK, STATUS_UNAVAILABLE,
    };
    use api::ipc::IPC_BUF_SIZE;

    #[test]
    fn encode_request_fills_full_ipc_buffer() {
        let request = encode_request();
        assert_eq!(request.len(), IPC_BUF_SIZE);
        assert_eq!(&request[..APP_MESSAGE_PREFIX.len()], &APP_MESSAGE_PREFIX);
        assert_eq!(request[APP_MESSAGE_PREFIX.len()], OP_SNAPSHOT);
        assert!(request[APP_MESSAGE_PREFIX.len() + 1..]
            .iter()
            .all(|&byte| byte == 0));
    }

    #[test]
    fn parse_status_accepts_success() {
        assert!(matches!(
            parse_status([OP_STATUS, SNAPSHOT_STATUS_PHASE, STATUS_OK]),
            SnapshotOutcome::Success
        ));
    }

    #[test]
    fn parse_status_accepts_unavailable() {
        assert!(matches!(
            parse_status([OP_STATUS, SNAPSHOT_STATUS_PHASE, STATUS_UNAVAILABLE]),
            SnapshotOutcome::Unavailable
        ));
    }

    #[test]
    fn parse_status_rejects_malformed_reply() {
        assert!(matches!(
            parse_status([0, 0, 0]),
            SnapshotOutcome::ProtocolError("snapshot: protocol error: malformed supervisor reply")
        ));
    }

    #[test]
    fn parse_status_rejects_unexpected_supervisor_status() {
        assert!(matches!(
            parse_status([OP_STATUS, 9, 9]),
            SnapshotOutcome::ProtocolError(
                "snapshot: protocol error: unexpected supervisor status"
            )
        ));
    }
}
