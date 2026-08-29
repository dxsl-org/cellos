use api::services::cluster::CellNetId;

use crate::c2c_envelope::{RetryClass, ServerEpoch, MAX_C2C_PAYLOAD};

pub const DEDUP_CAPACITY: usize = 16;
pub const DEDUP_TTL_MS: u64 = 30_000;
pub const SOURCE_WINDOW_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DedupKey {
    pub src_node: CellNetId,
    pub src_boot_epoch: u64,
    pub request_id: u64,
    pub dst_server_epoch: ServerEpoch,
}

impl DedupKey {
    pub(super) fn is_valid(self) -> bool {
        self.src_node.0.iter().any(|byte| *byte != 0)
            && self.src_boot_epoch != 0
            && self.request_id != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct SourceWindow {
    pub(super) node: CellNetId,
    pub(super) boot_epoch: u64,
    pub(super) high_request_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C2cStatus {
    Success,
    Busy,
    Indeterminate,
    NoService,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DedupDecision {
    Dispatch,
    Busy,
    Replay(usize),
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DedupError {
    Stale,
    InvalidTransition,
    PayloadTooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryState {
    Accepted,
    Dispatched,
    Completed,
    ExpiredNonReplayable,
}

#[derive(Clone, Copy)]
pub(super) struct Entry {
    pub(super) key: DedupKey,
    pub(super) retry_class: RetryClass,
    pub(super) state: EntryState,
    pub(super) first_seen_ms: u64,
    pub(super) status: C2cStatus,
    pub(super) payload_len: usize,
    pub(super) payload: [u8; MAX_C2C_PAYLOAD],
}

pub struct CachedReply<'a> {
    pub status: C2cStatus,
    pub payload: &'a [u8],
}

impl Entry {
    pub(super) const fn accepted(
        key: DedupKey,
        retry_class: RetryClass,
        first_seen_ms: u64,
    ) -> Self {
        Self {
            key,
            retry_class,
            state: EntryState::Accepted,
            first_seen_ms,
            status: C2cStatus::Indeterminate,
            payload_len: 0,
            payload: [0; MAX_C2C_PAYLOAD],
        }
    }
}
