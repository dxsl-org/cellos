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
