use super::*;

#[test]
fn epochs_and_monotonic_ids_define_the_replay_floor() {
    let mut cache = DedupCache::new();
    let original = key(40);
    assert_eq!(
        cache.begin(original, RetryClass::Never, 0),
        DedupDecision::Dispatch
    );

    let mut next_server = original;
    next_server.request_id += 1;
    next_server.dst_server_epoch = ServerEpoch::new(original.dst_server_epoch.get() + 1).unwrap();
    assert_eq!(
        cache.begin(next_server, RetryClass::Never, 0),
        DedupDecision::Dispatch
    );

    let mut next_boot = original;
    next_boot.src_boot_epoch += 1;
    assert_eq!(
        cache.begin(next_boot, RetryClass::Never, 0),
        DedupDecision::Dispatch
    );
    assert_eq!(
        cache.begin(original, RetryClass::Never, 0),
        DedupDecision::Indeterminate
    );
}

#[test]
fn multiple_sources_reuse_their_existing_replay_windows() {
    let mut cache = DedupCache::new();
    let mut source_a = key(1);
    source_a.src_node = CellNetId([0xa1; 32]);
    let mut source_b = key(1);
    source_b.src_node = CellNetId([0xb2; 32]);

    for request in [source_a, source_b] {
        assert_eq!(
            cache.begin(request, RetryClass::Never, 0),
            DedupDecision::Dispatch
        );
        cache
            .complete(request, C2cStatus::Success, b"done")
            .unwrap();
    }
    source_b.request_id = 2;
    assert_eq!(
        cache.begin(source_b, RetryClass::Never, 0),
        DedupDecision::Dispatch
    );
    assert_eq!(cache.sources.iter().flatten().count(), 2);

    cache
        .complete(source_b, C2cStatus::Success, b"done")
        .unwrap();
    for id in 3..=15 {
        source_b.request_id = id;
        assert_eq!(
            cache.begin(source_b, RetryClass::Never, 0),
            DedupDecision::Dispatch
        );
        cache
            .complete(source_b, C2cStatus::Success, b"done")
            .unwrap();
    }
    for id in 16..=17 {
        source_b.request_id = id;
        assert_eq!(
            cache.begin(source_b, RetryClass::Never, DEDUP_TTL_MS),
            DedupDecision::Dispatch
        );
    }
    source_b.request_id = 1;
    assert_eq!(
        cache.begin(source_b, RetryClass::Never, DEDUP_TTL_MS),
        DedupDecision::Indeterminate
    );
}

#[test]
fn newer_boot_observation_survives_entry_saturation() {
    let mut cache = DedupCache::new();
    for id in 1..=DEDUP_CAPACITY as u64 {
        assert_eq!(
            cache.begin(key(id), RetryClass::Never, 0),
            DedupDecision::Dispatch
        );
        cache.mark_dispatched(key(id)).unwrap();
    }

    let mut newer_boot = key(1);
    newer_boot.src_boot_epoch += 1;
    assert_eq!(
        cache.begin(newer_boot, RetryClass::Never, 1),
        DedupDecision::Busy
    );
    assert_eq!(
        cache.begin(key(DEDUP_CAPACITY as u64 + 1), RetryClass::Never, 1),
        DedupDecision::Indeterminate
    );

    cache.complete(key(1), C2cStatus::Success, b"done").unwrap();
    assert_eq!(
        cache.begin(newer_boot, RetryClass::Never, DEDUP_TTL_MS),
        DedupDecision::Dispatch
    );
}

#[test]
fn saturation_never_evicts_expired_inflight_idempotent_entries() {
    let mut cache = DedupCache::new();
    for id in 1..=DEDUP_CAPACITY as u64 {
        assert_eq!(
            cache.begin(key(id), RetryClass::Idempotent, 0),
            DedupDecision::Dispatch
        );
        cache.mark_dispatched(key(id)).unwrap();
    }
    assert_eq!(
        cache.begin(key(100), RetryClass::Never, DEDUP_TTL_MS),
        DedupDecision::Busy
    );
    assert_eq!(
        cache.begin(key(1), RetryClass::Idempotent, DEDUP_TTL_MS),
        DedupDecision::Busy
    );
}

#[test]
fn expired_completed_non_idempotent_entries_release_capacity() {
    let mut cache = DedupCache::new();
    for id in 1..=DEDUP_CAPACITY as u64 {
        assert_eq!(
            cache.begin(key(id), RetryClass::Never, 0),
            DedupDecision::Dispatch
        );
        cache.mark_dispatched(key(id)).unwrap();
        cache
            .complete(key(id), C2cStatus::Success, b"retained")
            .unwrap();
    }
    assert_eq!(cache.len(), DEDUP_CAPACITY);
    assert_eq!(
        cache.begin(key(100), RetryClass::Never, DEDUP_TTL_MS + 1),
        DedupDecision::Dispatch
    );
    assert_eq!(
        cache.begin(key(1), RetryClass::Never, DEDUP_TTL_MS + 1),
        DedupDecision::Indeterminate
    );
}

#[test]
fn expired_completed_idempotent_entry_releases_capacity() {
    let mut cache = DedupCache::new();
    for id in 1..=DEDUP_CAPACITY as u64 {
        assert_eq!(
            cache.begin(key(id), RetryClass::Idempotent, 0),
            DedupDecision::Dispatch
        );
        cache
            .complete(key(id), C2cStatus::Success, b"done")
            .unwrap();
    }
    assert_eq!(
        cache.begin(key(100), RetryClass::Conditional, DEDUP_TTL_MS),
        DedupDecision::Dispatch
    );
    assert_eq!(cache.len(), DEDUP_CAPACITY);
}

#[test]
fn stale_completion_and_oversize_payload_fail_closed() {
    let mut cache = DedupCache::new();
    assert_eq!(
        cache.complete(key(1), C2cStatus::Success, b"late"),
        Err(DedupError::Stale)
    );
    assert_eq!(
        cache.begin(key(1), RetryClass::Never, 0),
        DedupDecision::Dispatch
    );
    let payload = [0u8; MAX_C2C_PAYLOAD + 1];
    assert_eq!(
        cache.complete(key(1), C2cStatus::Success, &payload),
        Err(DedupError::PayloadTooLarge)
    );
}
