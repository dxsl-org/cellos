//! Snapshot and restore IPC envelopes shared by the supervisor swap flow.

use api::services::proto::app_envelope::{
    HEADER_LEN, KIND_RESTORE, KIND_SNAPSHOT, MAGIC, RESTORE_KEY_LEN, RESTORE_LEN, SNAPSHOT_LEN,
};
use api::services::proto::supervisor::HotswapError;
use ostd::ipc::post;

pub fn send_snapshot_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; SNAPSHOT_LEN];
    buf[0] = MAGIC;
    buf[1] = KIND_SNAPSHOT;
    buf[HEADER_LEN..].copy_from_slice(&swap_id.to_le_bytes());
    post(tid, &buf).map_err(|_| HotswapError::SnapshotIpcFailed)
}

pub fn send_restore_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; RESTORE_LEN];
    buf[0] = MAGIC;
    buf[1] = KIND_RESTORE;
    let mut tmp = [0u8; 20];
    let len = fmt_u64_decimal(swap_id, &mut tmp).min(RESTORE_KEY_LEN - 1);
    buf[HEADER_LEN..HEADER_LEN + len].copy_from_slice(&tmp[..len]);
    post(tid, &buf).map_err(|_| HotswapError::RestoreIpcFailed)
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
