use crate::local_ingress::{MAX_REPLY_BODY, MAX_REQUEST_BODY};
use crate::local_queue::BrokerCounters;
use crate::local_queue::{
    QueuedReply, RequestKey, WorkerRequest, IN_FLIGHT_CAP, LOCAL_REPLY_QUEUE_CAP,
    LOCAL_REQUEST_QUEUE_CAP, STALE_REPLY_RING_CAP,
};
use core::mem::size_of;

mod commands;
mod frames;
mod snapshot;
#[cfg(test)]
mod tests;

pub use commands::{
    encode_echo_command, encode_hold_command, encode_snapshot_command, parse_command,
    OracleCommand, OracleError, MAX_HOLD_TURNS, OP_ECHO, OP_HOLD, OP_SNAPSHOT,
};
pub use frames::{
    decode_reply_frame, encode_echo_request, encode_hold_request, encode_snapshot_request,
    DecodedReply,
};
pub use snapshot::{
    decode_snapshot_payload, encode_snapshot_payload, OracleSnapshot, SNAPSHOT_BYTES,
    SNAPSHOT_U64_FIELDS, SNAPSHOT_VERSION,
};

pub const STATIC_FOOTPRINT_LIMIT_BYTES: usize = 512 * 1024;
pub const STATIC_FOOTPRINT_BYTES: usize =
    size_of::<[Option<WorkerRequest>; LOCAL_REQUEST_QUEUE_CAP]>()
        + size_of::<[Option<QueuedReply>; LOCAL_REPLY_QUEUE_CAP]>()
        + size_of::<[Option<RequestKey>; IN_FLIGHT_CAP]>()
        + size_of::<[Option<RequestKey>; STALE_REPLY_RING_CAP]>()
        + size_of::<BrokerCounters>()
        + MAX_REQUEST_BODY
        + MAX_REPLY_BODY;
const _: usize = STATIC_FOOTPRINT_LIMIT_BYTES - STATIC_FOOTPRINT_BYTES;
