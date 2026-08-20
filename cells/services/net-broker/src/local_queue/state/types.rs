use crate::local_ingress::{encode_reply, ReplyStatus, MAX_REQUEST_BODY};
use api::ipc::IPC_BUF_SIZE;

pub const LOCAL_REQUEST_QUEUE_CAP: usize = 16;
pub const LOCAL_REPLY_QUEUE_CAP: usize = 16;
pub const PER_CALLER_WINDOW: usize = 4;
pub const IN_FLIGHT_CAP: usize = 32;
pub const STALE_REPLY_RING_CAP: usize = 64;
pub const REPLY_TRY_SEND_BUDGET: usize = 8;
// The broker's bounded saturation probe can occupy 512 cooperative turns, so
// its roles need a local deadline with scheduling margin.
pub const WORKER_HEARTBEAT_TICKS: u64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestKey {
    pub request_id: u64,
    pub caller_tid: usize,
    pub cell_id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerRequest {
    pub key: RequestKey,
    pub client_sequence: u64,
    pub payload_len: usize,
    pub payload: [u8; MAX_REQUEST_BODY],
    pub order: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedReply {
    caller_tid: usize,
    pub request_id: u64,
    pub client_sequence: u64,
    pub status: ReplyStatus,
    pub payload_len: usize,
    pub payload: [u8; crate::local_ingress::MAX_REPLY_BODY],
    pub order: u64,
    pub attempts: u8,
}

// This no-alloc boundary returns the fixed-size reply by value so saturation
// never drops ownership or hides an allocation failure.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngressDecision {
    Accepted,
    Immediate(QueuedReply),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionError {
    ReplyQueueFull,
    Stale,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrokerCounters {
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
    pub peak_request_queue: usize,
    pub peak_reply_queue: usize,
    pub peak_in_flight: usize,
    pub peak_bytes: usize,
    pub network_progress: u64,
}

impl WorkerRequest {
    pub fn for_test(
        cell_id: u64,
        generation: u64,
        caller_tid: usize,
        request_id: u64,
        client_sequence: u64,
        payload: &[u8],
    ) -> Self {
        let mut body = [0; MAX_REQUEST_BODY];
        let len = payload.len().min(MAX_REQUEST_BODY);
        body[..len].copy_from_slice(&payload[..len]);
        Self {
            key: RequestKey {
                request_id,
                caller_tid,
                cell_id,
                generation,
            },
            client_sequence,
            payload_len: len,
            payload: body,
            order: 0,
        }
    }
}

impl QueuedReply {
    pub fn new(
        caller_tid: usize,
        request_id: u64,
        client_sequence: u64,
        status: ReplyStatus,
        payload: &[u8],
        order: u64,
    ) -> Self {
        let mut body = [0; crate::local_ingress::MAX_REPLY_BODY];
        let len = payload.len().min(crate::local_ingress::MAX_REPLY_BODY);
        body[..len].copy_from_slice(&payload[..len]);
        Self {
            caller_tid,
            request_id,
            client_sequence,
            status,
            payload_len: len,
            payload: body,
            order,
            attempts: 0,
        }
    }

    pub fn success(request: &WorkerRequest) -> Self {
        Self::new(
            request.key.caller_tid,
            request.key.request_id,
            request.client_sequence,
            ReplyStatus::Success,
            &request.payload[..request.payload_len],
            request.order,
        )
    }

    pub fn encode(&self, out: &mut [u8; IPC_BUF_SIZE]) -> usize {
        encode_reply(
            self.status,
            self.request_id,
            self.client_sequence,
            &self.payload[..self.payload_len],
            out,
        )
    }

    pub fn caller_tid(&self) -> usize {
        self.caller_tid
    }
}
