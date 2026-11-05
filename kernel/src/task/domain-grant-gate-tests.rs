//! Host model of the publish/revoke protocol: no receiver mapping may survive
//! a completed revocation, whatever the interleaving.

use super::*;
use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;
use std::sync::Barrier;
use std::thread;

/// One grant: the lifecycle plus the receiver PTE, the record of it that the
/// drain consults, and whether the frames went back to the allocator.
struct Grant {
    life: GrantLifecycle,
    receiver_pte: AtomicBool,
    receiver_recorded: AtomicBool,
    frames_freed: AtomicBool,
}

impl Grant {
    fn new() -> Self {
        Self {
            life: GrantLifecycle::new(),
            receiver_pte: AtomicBool::new(false),
            receiver_recorded: AtomicBool::new(false),
            frames_freed: AtomicBool::new(false),
        }
    }

    /// `DomainGrant::publish`: the state check, a map loop long enough to be
    /// overtaken by a whole revoke, then the receiver record.
    fn publish(&self) -> Result<(), ()> {
        self.life.while_live((), || {
            for _ in 0..2_000 {
                core::hint::spin_loop();
            }
            self.receiver_pte.store(true, Ordering::SeqCst);
            self.receiver_recorded.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    /// `DomainGrant::revoke` followed by the table's frame release.
    fn revoke_and_free(&self) {
        if self.life.begin_revoke() == DomainGrantState::Revoked {
            return;
        }
        self.life.serialized(|| {
            if self.receiver_recorded.swap(false, Ordering::SeqCst) {
                self.receiver_pte.store(false, Ordering::SeqCst);
            }
        });
        self.life.finish_revoke();
        self.frames_freed.store(true, Ordering::SeqCst);
    }
}

#[test]
fn no_receiver_mapping_survives_revoke() {
    for round in 0..500 {
        let grant = Arc::new(Grant::new());
        let start = Arc::new(Barrier::new(2));
        let publisher = {
            let (grant, start) = (Arc::clone(&grant), Arc::clone(&start));
            thread::spawn(move || {
                start.wait();
                let _ = grant.publish();
            })
        };
        start.wait();
        grant.revoke_and_free();
        publisher.join().expect("publisher");
        assert!(grant.frames_freed.load(Ordering::SeqCst));
        assert!(
            !grant.receiver_pte.load(Ordering::SeqCst),
            "round {round}: receiver still maps frames freed by revoke"
        );
    }
}

#[test]
fn publication_after_revoke_is_refused() {
    let grant = Grant::new();
    assert_eq!(grant.publish(), Ok(()));
    grant.revoke_and_free();
    assert!(!grant.receiver_pte.load(Ordering::SeqCst));
    assert_eq!(grant.life.state(), DomainGrantState::Revoked);
    assert_eq!(grant.publish(), Err(()));
    assert!(!grant.receiver_pte.load(Ordering::SeqCst));
    // A retried revoke of a finished record is a no-op.
    assert_eq!(grant.life.begin_revoke(), DomainGrantState::Revoked);
}

#[test]
fn revoking_record_refuses_publication() {
    let life = GrantLifecycle::new();
    assert_eq!(life.begin_revoke(), DomainGrantState::Revoking);
    let mut ran = false;
    assert_eq!(
        life.while_live("not live", || {
            ran = true;
            Ok(())
        }),
        Err("not live")
    );
    assert!(!ran, "publication body ran on a Revoking record");
}
