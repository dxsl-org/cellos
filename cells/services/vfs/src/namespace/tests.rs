use super::*;

impl NamespaceLedger {
    #[cfg(test)]
    pub(crate) fn snapshot(&self, key: &NamespaceKey) -> Option<(u32, u32, bool)> {
        #[cfg(not(test))]
        let entries = self.entries.lock();
        #[cfg(test)]
        let entries = self.entries.lock().unwrap();
        entries.get(key).map(|state| {
            (state.transient, state.service_handle, state.exclusive)
        })
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        #[cfg(not(test))]
        let entries = self.entries.lock();
        #[cfg(test)]
        let entries = self.entries.lock().unwrap();
        entries.len()
    }
}

#[cfg(test)]
fn key(path: &str) -> NamespaceKey {
    NamespaceKey::parse(path).expect("canonical /srv key")
}

#[test]
fn canonical_srv_keys_reject_aliases_and_invalid_paths() {
    for path in ["/srv", "/srv/a", "/srv/a b", "/srv/a/b"] {
        let parsed = NamespaceKey::parse(path).expect(path);
        assert_eq!(parsed.as_str(), path);
    }
    for path in [
        "", "srv/a", "//srv/a", "/", "/srv/", "/srv//a", "/srv/a//b",
        "/srv/./a", "/srv/a/../b", "/srv/a/", "/srv-other/a", "/srv/a\0b",
    ] {
        assert_eq!(NamespaceKey::parse(path), Err(InvalidNamespaceKey), "{path:?}");
    }
}

#[test]
fn shared_leases_block_exclusive_reservations() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let transient = ledger.acquire_transient(&a).expect("transient");
    assert!(matches!(ledger.reserve_one(&a), Err(AcquireError::Conflict)));
    let handle = ledger
        .acquire_service_handle(&a)
        .expect("compatible shared lease");
    assert_eq!(ledger.snapshot(&a), Some((1, 1, false)));
    drop(transient);
    assert!(matches!(ledger.reserve_one(&a), Err(AcquireError::Conflict)));
    drop(handle);
    assert_eq!(ledger.snapshot(&a), None);
}

#[test]
fn exclusive_reservations_block_both_shared_lease_types() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let reservation = ledger.reserve_one(&a).expect("exclusive");
    assert!(matches!(
        ledger.acquire_transient(&a),
        Err(AcquireError::Conflict)
    ));
    assert!(matches!(
        ledger.acquire_service_handle(&a),
        Err(AcquireError::Conflict)
    ));
    assert_eq!(ledger.snapshot(&a), Some((0, 0, true)));
    drop(reservation);
    assert_eq!(ledger.snapshot(&a), None);
}

#[test]
fn sorted_two_key_failure_is_atomic_without_partial_acquisition() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let z = key("/srv/z");
    let blocker = ledger.acquire_transient(&z).expect("blocker");
    assert!(matches!(
        ledger.reserve_two(&z, &a),
        Err(AcquireError::Conflict)
    ));
    assert_eq!(ledger.snapshot(&a), None);
    let a_only = ledger.reserve_one(&a).expect("no partial reservation");
    drop(a_only);
    drop(blocker);
    assert_eq!(ledger.entry_count(), 0);
}

#[test]
fn equal_key_pair_is_deduplicated() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let reservation = ledger.reserve_two(&a, &a).expect("deduplicated");
    assert_eq!(ledger.snapshot(&a), Some((0, 0, true)));
    assert_eq!(ledger.entry_count(), 1);
    assert!(matches!(ledger.reserve_one(&a), Err(AcquireError::Conflict)));
    drop(reservation);
    assert_eq!(ledger.entry_count(), 0);
}

#[test]
fn leases_release_once_without_underflow_and_remove_empty_entries() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let first = ledger.acquire_transient(&a).expect("first");
    let second = ledger.acquire_transient(&a).expect("second");
    let handle = ledger.acquire_service_handle(&a).expect("handle");
    assert_eq!(ledger.snapshot(&a), Some((2, 1, false)));
    drop(first);
    assert_eq!(ledger.snapshot(&a), Some((1, 1, false)));
    drop(second);
    assert_eq!(ledger.snapshot(&a), Some((0, 1, false)));
    drop(handle);
    assert_eq!(ledger.snapshot(&a), None);
    assert_eq!(ledger.entry_count(), 0);
}

#[test]
fn two_key_reservation_cleans_both_entries() {
    let ledger = NamespaceLedger::new();
    let a = key("/srv/a");
    let b = key("/srv/b");
    let reservation = ledger.reserve_two(&b, &a).expect("reservation");
    assert_eq!(ledger.snapshot(&a), Some((0, 0, true)));
    assert_eq!(ledger.snapshot(&b), Some((0, 0, true)));
    drop(reservation);
    assert_eq!(ledger.entry_count(), 0);
}
