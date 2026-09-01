use super::*;
use crate::c2c_envelope::ServerEpoch;
use crate::c2c_envelope::MAX_C2C_PAYLOAD;
use api::services::cluster::CellNetId;
mod saturation;

fn key(id: u64) -> DedupKey {
    DedupKey {
        src_node: CellNetId([0x11; 32]),
        src_boot_epoch: 2,
        request_id: id,
        dst_server_epoch: ServerEpoch::new(3).unwrap(),
    }
}

#[test]
fn constants_and_static_budget_are_frozen() {
    assert_eq!(DEDUP_CAPACITY, 16);
    assert_eq!(DEDUP_TTL_MS, 30_000);
    assert_eq!(SOURCE_WINDOW_CAPACITY, 16);
    assert!(DEDUP_STATIC_BYTES <= 64 * 1024);
}

#[test]
fn empty_state_tracks_admitted_entries() {
    let mut cache = DedupCache::new();
    assert!(cache.is_empty());

    assert_eq!(
        cache.begin(key(1), RetryClass::Never, 0),
        DedupDecision::Dispatch
    );
    assert!(!cache.is_empty());
}

#[test]
fn duplicate_inflight_is_busy_and_completion_replays() {
    let mut cache = DedupCache::new();
    let request = key(1);
    assert_eq!(
        cache.begin(request, RetryClass::Never, 10),
        DedupDecision::Dispatch
    );
    assert_eq!(cache.mark_dispatched(request), Ok(()));
    assert_eq!(
        cache.begin(request, RetryClass::Never, 11),
        DedupDecision::Busy
    );
    assert_eq!(cache.complete(request, C2cStatus::Success, b"ok"), Ok(()));

    let DedupDecision::Replay(slot) = cache.begin(request, RetryClass::Never, 12) else {
        panic!("completed duplicate must replay");
    };
    let reply = cache.replay(slot, request, 12).expect("retained reply");
    assert_eq!(reply.status, C2cStatus::Success);
    assert_eq!(reply.payload, b"ok");
}

#[test]
fn expired_non_idempotent_request_never_redispatches() {
    let mut cache = DedupCache::new();
    let request = key(2);
    assert_eq!(
        cache.begin(request, RetryClass::Conditional, 0),
        DedupDecision::Dispatch
    );
    cache.mark_dispatched(request).unwrap();
    cache
        .complete(request, C2cStatus::Success, b"done")
        .unwrap();
    assert_eq!(
        cache.begin(request, RetryClass::Conditional, DEDUP_TTL_MS),
        DedupDecision::Indeterminate
    );
    assert_eq!(
        cache.begin(request, RetryClass::Conditional, DEDUP_TTL_MS + 1),
        DedupDecision::Indeterminate
    );
}

#[test]
fn expired_idempotent_request_can_dispatch_again() {
    let mut cache = DedupCache::new();
    let request = key(3);
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, 0),
        DedupDecision::Dispatch
    );
    cache.complete(request, C2cStatus::Success, b"old").unwrap();
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, DEDUP_TTL_MS),
        DedupDecision::Dispatch
    );
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, DEDUP_TTL_MS),
        DedupDecision::Busy
    );
}

#[test]
fn expired_idempotent_inflight_request_stays_busy() {
    let mut cache = DedupCache::new();
    let request = key(30);
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, 0),
        DedupDecision::Dispatch
    );
    cache.mark_dispatched(request).unwrap();
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, DEDUP_TTL_MS),
        DedupDecision::Busy
    );
}

#[test]
fn retry_class_conflict_is_indeterminate() {
    let mut cache = DedupCache::new();
    let request = key(4);
    assert_eq!(
        cache.begin(request, RetryClass::Never, 0),
        DedupDecision::Dispatch
    );
    assert_eq!(
        cache.begin(request, RetryClass::Idempotent, 1),
        DedupDecision::Indeterminate
    );
}
