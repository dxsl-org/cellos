use super::*;

#[test]
fn request_id_exhaustion_stays_terminal_after_max() {
    let mut state = BrokerState::new();
    state.next_request_id = u64::MAX;

    assert_eq!(state.alloc_request_id(), Some(u64::MAX));
    assert_eq!(state.next_request_id, 0);
    assert_eq!(state.alloc_request_id(), None);
    state.stale[0] = None;
    assert_eq!(state.alloc_request_id(), None);
}

#[test]
fn new_state_starts_a_fresh_volatile_epoch() {
    let mut prior = BrokerState::new();
    prior.counters.accepted = 7;
    prior.next_request_id = 19;
    prior.next_request_order = 23;
    prior.next_reply_order = 29;
    prior.stale[0] = Some(RequestKey {
        request_id: 17,
        caller_tid: 3,
        cell_id: 5,
        generation: 11,
    });
    assert_eq!(prior.counters.accepted, 7);
    assert_eq!(prior.next_request_id, 19);
    assert!(prior.stale[0].is_some());

    let fresh = BrokerState::new();
    assert_eq!(fresh.counters, BrokerCounters::default());
    assert_eq!(fresh.next_request_id, 1);
    assert_eq!(fresh.next_request_order, 0);
    assert_eq!(fresh.next_reply_order, 0);
    assert!(fresh.requests.iter().all(Option::is_none));
    assert!(fresh.replies.iter().all(Option::is_none));
    assert!(fresh.inflight.iter().all(Option::is_none));
    assert!(fresh.stale.iter().all(Option::is_none));
    assert_eq!(fresh.stale_cursor, 0);
    assert!(fresh.last_served.is_none());
}
