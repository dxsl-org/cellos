use super::*;

#[test]
fn request_id_wraps_once_and_rejects_exhausted_collisions() {
    let mut state = BrokerState::new();
    state.next_request_id = u64::MAX;

    let wrapped = state.alloc_request_id().expect("u64::MAX is still usable");
    assert_eq!(wrapped, u64::MAX);
    assert_eq!(state.next_request_id, 0);

    state.next_request_id = 0;
    state.stale[0] = Some(RequestKey {
        request_id: 1,
        caller_tid: 1,
        cell_id: 1,
        generation: 1,
    });
    assert_eq!(state.alloc_request_id(), None);
}
