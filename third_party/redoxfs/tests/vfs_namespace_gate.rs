#[path = "../../../cells/services/vfs/src/namespace.rs"]
mod namespace;

#[cfg(test)]
mod tests {
    use super::namespace::*;

    fn key(path: &str) -> NamespaceKey {
        NamespaceKey::parse(path).expect("canonical /srv key")
    }

    #[test]
    fn vfs_shipping_namespace_keys_pass_host_gate() {
        for path in ["/srv", "/srv/a", "/srv/a b", "/srv/a/b"] {
            let parsed = NamespaceKey::parse(path).expect(path);
            assert_eq!(parsed.as_str(), path);
        }
        for path in [
            "",
            "srv/a",
            "//srv/a",
            "/",
            "/srv/",
            "/srv//a",
            "/srv/a//b",
            "/srv/./a",
            "/srv/a/../b",
            "/srv/a/",
            "/srv-other/a",
            "/srv/a\0b",
        ] {
            assert_eq!(
                NamespaceKey::parse(path),
                Err(InvalidNamespaceKey),
                "{path:?}"
            );
        }
    }

    #[test]
    fn vfs_shipping_namespace_ledger_shared_blocks_exclusive() {
        let ledger = NamespaceLedger::new();
        let a = key("/srv/a");
        let transient = ledger.acquire_transient(&a).expect("transient");
        assert!(matches!(
            ledger.reserve_one(&a),
            Err(AcquireError::Conflict)
        ));
        let handle = ledger
            .acquire_service_handle(&a)
            .expect("compatible shared lease");
        drop(transient);
        assert!(matches!(
            ledger.reserve_one(&a),
            Err(AcquireError::Conflict)
        ));
        drop(handle);
        let exclusive = ledger
            .reserve_one(&a)
            .expect("exclusive after shared release");
        drop(exclusive);
    }

    #[test]
    fn vfs_shipping_namespace_ledger_exclusive_blocks_shared() {
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
        drop(reservation);
        let shared = ledger
            .acquire_transient(&a)
            .expect("shared after exclusive");
        drop(shared);
    }

    #[test]
    fn vfs_shipping_namespace_ledger_atomic_two_key_reservation() {
        let ledger = NamespaceLedger::new();
        let a = key("/srv/a");
        let z = key("/srv/z");
        let blocker = ledger.acquire_transient(&z).expect("blocker");
        assert!(matches!(
            ledger.reserve_two(&z, &a),
            Err(AcquireError::Conflict)
        ));
        let a_only = ledger
            .reserve_one(&a)
            .expect("no partial reservation on failure");
        drop(a_only);
        drop(blocker);
        let both = ledger.reserve_two(&a, &z).expect("both after unblocked");
        drop(both);
    }
}
