use crate::local_ingress::{MAX_REPLY_BODY, MAX_REQUEST_BODY};
use crate::local_queue::{
    BrokerCounters, QueuedReply, RequestKey, WorkerRequest, IN_FLIGHT_CAP, LOCAL_REPLY_QUEUE_CAP,
    LOCAL_REQUEST_QUEUE_CAP, STALE_REPLY_RING_CAP,
};
use core::mem::size_of;

pub const STATIC_FOOTPRINT_BYTES: usize =
    size_of::<[Option<WorkerRequest>; LOCAL_REQUEST_QUEUE_CAP]>()
        + size_of::<[Option<QueuedReply>; LOCAL_REPLY_QUEUE_CAP]>()
        + size_of::<[Option<RequestKey>; IN_FLIGHT_CAP]>()
        + size_of::<[Option<RequestKey>; STALE_REPLY_RING_CAP]>()
        + size_of::<BrokerCounters>()
        + MAX_REQUEST_BODY
        + MAX_REPLY_BODY;
pub const STATIC_FOOTPRINT_LIMIT_BYTES: usize = 512 * 1024;
const _: usize = STATIC_FOOTPRINT_LIMIT_BYTES - STATIC_FOOTPRINT_BYTES;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_footprint_stays_below_cap() {
        assert!(STATIC_FOOTPRINT_BYTES <= STATIC_FOOTPRINT_LIMIT_BYTES);
    }
}
