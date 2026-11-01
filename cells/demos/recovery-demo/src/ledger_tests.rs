use crate::ledger::{Authority, Committed, Sender, Writer, METHOD_ADD_TWICE, METHOD_INC, SCHEMA};
use crate::wire::{Binding, Msg, Refusal};

const W1: Sender = Sender { tid: 10, cell: 7, generation: 1 };
const W2: Sender = Sender { tid: 11, cell: 7, generation: 2 };

fn bind(req: u64, method: u8, payload: u64) -> Binding {
    Binding { caller_cell: 3, caller_generation: 9, request_id: req, method, rights: 0, payload }
}

fn attached(a: &mut Authority, nonce: u64, w: Sender) -> u64 {
    a.expect_worker(nonce, w.tid);
    a.attach(nonce, w).expect("attach").0
}

#[test]
fn commit_is_bound_to_revision_and_replays_only_the_same_binding() {
    let mut a = Authority::new();
    let e = attached(&mut a, 5, W1);
    assert_eq!(a.lookup(e, W1, &bind(1, METHOD_INC, 1)), Msg::Absent);
    assert_eq!(a.commit(e, W1, 0, &bind(1, METHOD_INC, 1), 1), Msg::Committed { revision: 1, result: 1 });
    assert_eq!(a.lookup(e, W1, &bind(1, METHOD_INC, 1)), Msg::Present { revision: 1, result: 1 });
    assert_eq!(a.lookup(e, W1, &bind(1, METHOD_INC, 5)), Msg::Refused(Refusal::Conflict));
    assert_eq!(a.lookup(e, W1, &bind(1, METHOD_ADD_TWICE, 1)), Msg::Refused(Refusal::Conflict));
    assert_eq!(a.commit(e, W1, 0, &bind(2, METHOD_INC, 1), 2), Msg::Refused(Refusal::RevisionMismatch));
    assert_eq!(a.commit(e, W1, 1, &bind(2, METHOD_INC, 1), 7), Msg::Refused(Refusal::InvalidMutation));
    assert_eq!(a.committed().unwrap().counter, 1);
}

#[test]
fn another_principal_with_the_same_request_id_is_independent() {
    let mut a = Authority::new();
    let e = attached(&mut a, 5, W1);
    assert!(matches!(a.commit(e, W1, 0, &bind(1, METHOD_INC, 1), 1), Msg::Committed { .. }));
    let other = Binding { caller_generation: 10, ..bind(1, METHOD_INC, 1) };
    assert_eq!(a.lookup(e, W1, &other), Msg::Absent);
    assert_eq!(a.commit(e, W1, 1, &other, 2), Msg::Committed { revision: 2, result: 2 });
}

#[test]
fn fence_ends_the_old_writer_and_a_fresh_epoch_continues_at_the_watermark() {
    let mut a = Authority::new();
    let e1 = attached(&mut a, 5, W1);
    assert!(matches!(a.commit(e1, W1, 0, &bind(1, METHOD_INC, 1), 1), Msg::Committed { .. }));
    assert_eq!(a.fence(), 1);
    assert_eq!(a.commit(e1, W1, 1, &bind(2, METHOD_INC, 1), 2), Msg::Refused(Refusal::StaleEpoch));
    assert_eq!(a.lookup(e1, W1, &bind(1, METHOD_INC, 1)), Msg::Refused(Refusal::StaleEpoch));
    a.expect_worker(6, W2.tid);
    assert_eq!(a.attach(5, W2), Err(Refusal::Denied), "wrong nonce");
    let (e2, state) = a.attach(6, W2).expect("replacement attaches");
    assert!(e2 > e1);
    assert_eq!((state.revision, state.counter), (1, 1));
    assert_eq!(a.commit(e2, W1, 1, &bind(2, METHOD_INC, 1), 2), Msg::Refused(Refusal::StaleEpoch));
    assert_eq!(a.lookup(e2, W2, &bind(1, METHOD_INC, 1)), Msg::Present { revision: 1, result: 1 });
}

#[test]
fn attach_requires_the_exact_child_and_consumes_the_expectation() {
    let mut a = Authority::new();
    a.expect_worker(5, W1.tid);
    assert_eq!(a.attach(5, W2), Err(Refusal::Denied), "other tid");
    assert!(a.attach(5, W1).is_ok());
    assert_eq!(a.attach(5, W1), Err(Refusal::Denied), "nonce is one-shot");
}

#[test]
fn missing_corrupt_or_off_watermark_state_is_recovery_failed() {
    let pristine = Committed::new(SCHEMA, 0, 0);
    let corrupt = Committed { counter: 9, ..pristine };
    let ahead = Committed::new(SCHEMA, 4, 0);
    for state in [None, Some(corrupt), Some(ahead)] {
        let mut a = Authority::new();
        a.fence();
        a.replace_committed(state);
        a.expect_worker(5, W1.tid);
        assert_eq!(a.attach(5, W1), Err(Refusal::RecoveryFailed));
        assert_eq!(a.writer(), Writer::Fenced, "no writer after a failed restore");
    }
}

#[test]
fn ledger_is_bounded_per_principal() {
    let mut a = Authority::new();
    let e = attached(&mut a, 5, W1);
    let mut rev = 0;
    for req in 0..64 {
        assert!(matches!(a.commit(e, W1, rev, &bind(req, METHOD_INC, 1), rev + 1), Msg::Committed { .. }));
        rev += 1;
    }
    assert_eq!(a.commit(e, W1, rev, &bind(99, METHOD_INC, 1), rev + 1), Msg::Refused(Refusal::QuotaExceeded));
    assert_eq!(a.ledger_len(), 64);
}
