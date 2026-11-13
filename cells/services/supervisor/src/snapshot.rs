//! Snapshot orchestration for the Supervisor Cell.

use crate::protocol::{encode_snapshot_ok, encode_snapshot_unavailable};
use ostd::syscall::{sys_snapshot, SyscallResult};
use wire::proto::supervisor::STATUS_REPLY_LEN;

/// Execute the kernel snapshot syscall and map it to a bounded supervisor reply.
pub fn run() -> [u8; STATUS_REPLY_LEN] {
    map_snapshot_result(sys_snapshot())
}

fn map_snapshot_result(result: SyscallResult) -> [u8; STATUS_REPLY_LEN] {
    match result {
        SyscallResult::Ok(_) => encode_snapshot_ok(),
        SyscallResult::Err(_) => encode_snapshot_unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use wire::proto::supervisor::{OP_STATUS, PHASE_SNAPSHOT, STATUS_OK, STATUS_UNAVAILABLE};

    #[test]
    fn success_status_is_stable() {
        assert_eq!(
            super::map_snapshot_result(ostd::syscall::SyscallResult::Ok(7)),
            [OP_STATUS, PHASE_SNAPSHOT, STATUS_OK]
        );
    }

    #[test]
    fn failure_status_is_bounded() {
        assert_eq!(
            super::map_snapshot_result(ostd::syscall::SyscallResult::Err(
                ostd::syscall::SyscallError::Unknown
            )),
            [OP_STATUS, PHASE_SNAPSHOT, STATUS_UNAVAILABLE]
        );
    }
}
