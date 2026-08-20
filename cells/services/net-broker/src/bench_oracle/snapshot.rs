use super::commands::OracleError;
use crate::local_queue::BrokerCounters;

pub const SNAPSHOT_VERSION: u8 = 1;
pub const SNAPSHOT_U64_FIELDS: usize = 16;
pub const SNAPSHOT_BYTES: usize = 1 + SNAPSHOT_U64_FIELDS * 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OracleSnapshot {
    pub accepted: u64,
    pub completed: u64,
    pub busy: u64,
    pub terminal: u64,
    pub indeterminate: u64,
    pub duplicate: u64,
    pub stale_reply: u64,
    pub try_send_busy: u64,
    pub heartbeat_miss: u64,
    pub watchdog_expired: u64,
    pub peak_request_queue: u64,
    pub peak_reply_queue: u64,
    pub peak_in_flight: u64,
    pub peak_bytes: u64,
    pub network_progress: u64,
    pub static_footprint_bytes: u64,
}

pub fn encode_snapshot_payload(
    counters: &BrokerCounters,
    static_footprint_bytes: u64,
    out: &mut [u8; SNAPSHOT_BYTES],
) {
    out.fill(0);
    out[0] = SNAPSHOT_VERSION;
    let words = [
        counters.accepted,
        counters.completed,
        counters.busy,
        counters.terminal,
        counters.indeterminate,
        counters.duplicate,
        counters.stale_reply,
        counters.try_send_busy,
        counters.heartbeat_miss,
        counters.watchdog_expired,
        counters.peak_request_queue as u64,
        counters.peak_reply_queue as u64,
        counters.peak_in_flight as u64,
        counters.peak_bytes as u64,
        counters.network_progress,
        static_footprint_bytes,
    ];
    for (idx, value) in words.into_iter().enumerate() {
        let start = 1 + idx * 8;
        out[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
}

pub fn decode_snapshot_payload(payload: &[u8]) -> Result<OracleSnapshot, OracleError> {
    if payload.len() != SNAPSHOT_BYTES {
        return Err(OracleError::SnapshotSizeMismatch);
    }
    if payload[0] != SNAPSHOT_VERSION {
        return Err(OracleError::SnapshotVersionMismatch);
    }
    let read = |idx: usize| {
        let start = 1 + idx * 8;
        u64::from_le_bytes(payload[start..start + 8].try_into().unwrap_or([0; 8]))
    };
    Ok(OracleSnapshot {
        accepted: read(0),
        completed: read(1),
        busy: read(2),
        terminal: read(3),
        indeterminate: read(4),
        duplicate: read(5),
        stale_reply: read(6),
        try_send_busy: read(7),
        heartbeat_miss: read(8),
        watchdog_expired: read(9),
        peak_request_queue: read(10),
        peak_reply_queue: read(11),
        peak_in_flight: read(12),
        peak_bytes: read(13),
        network_progress: read(14),
        static_footprint_bytes: read(15),
    })
}
