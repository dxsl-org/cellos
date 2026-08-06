//! Snapshot and restore IPC envelopes shared by the supervisor swap flow.

use crate::error::HotswapError;
use ostd::syscall::{sys_send, SyscallResult};

const APP_MSG_MAGIC: u8 = 0xAC;
const DISC_SNAPSHOT: u8 = 0xF0;
const DISC_RESTORE: u8 = 0xF1;

pub fn send_snapshot_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; 10];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_SNAPSHOT;
    buf[2..10].copy_from_slice(&swap_id.to_le_bytes());
    match sys_send(tid, &buf) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(HotswapError::SnapshotIpcFailed),
    }
}

pub fn send_restore_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; 66];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_RESTORE;
    let mut tmp = [0u8; 20];
    let len = fmt_u64_decimal(swap_id, &mut tmp).min(63);
    buf[2..2 + len].copy_from_slice(&tmp[..len]);
    match sys_send(tid, &buf) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(HotswapError::RestoreIpcFailed),
    }
}

fn fmt_u64_decimal(mut value: u64, buf: &mut [u8; 20]) -> usize {
    if value == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut start = buf.len();
    while value > 0 {
        start -= 1;
        buf[start] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    let len = buf.len() - start;
    buf.copy_within(start.., 0);
    len
}
