use super::*;
use crate::local_ingress::{ParsedLocalRequest, ReplyStatus};
use api::caller_identity::CallerIdentity;

fn identity(cell_id: u64, generation: u64, sender_tid: u64) -> CallerIdentity {
    CallerIdentity {
        flags: 0,
        cell_id,
        generation,
        sender_tid,
    }
}

fn parsed(seq: u64, payload: &[u8]) -> ParsedLocalRequest {
    let mut parsed = ParsedLocalRequest::zero();
    parsed.client_sequence = seq;
    parsed.payload_len = payload.len();
    parsed.payload[..payload.len()].copy_from_slice(payload);
    parsed
}

#[test]
fn enforces_per_caller_window_and_first_overflow_busy() {
    let mut state = BrokerState::new();
    for seq in 0..PER_CALLER_WINDOW as u64 {
        assert_eq!(
            state.handle_ingress(9, Some(identity(1, 2, 9)), Ok(parsed(seq, b"x"))),
            IngressDecision::Accepted
        );
    }
    let overflow = state.handle_ingress(9, Some(identity(1, 2, 9)), Ok(parsed(99, b"x")));
    match overflow {
        IngressDecision::Immediate(reply) => assert_eq!(reply.status, ReplyStatus::Busy),
        _ => panic!("expected busy"),
    }
}

#[test]
fn full_request_queue_returns_busy_before_rejecting_otherwise_valid_calls() {
    let mut state = BrokerState::new();
    for idx in 0..LOCAL_REQUEST_QUEUE_CAP {
        let tid = idx + 1;
        assert_eq!(
            state.handle_ingress(
                tid,
                Some(identity(tid as u64, 1, tid as u64)),
                Ok(parsed(tid as u64, b"x")),
            ),
            IngressDecision::Accepted
        );
    }
    let overflow = state.handle_ingress(999, Some(identity(999, 1, 999)), Ok(parsed(999, b"x")));
    match overflow {
        IngressDecision::Immediate(reply) => assert_eq!(reply.status, ReplyStatus::Busy),
        _ => panic!("expected busy"),
    }
    assert_eq!(state.counters.busy, 1);
}

#[test]
fn rejects_sender_tid_mismatch_and_generation_zero() {
    let mut state = BrokerState::new();
    match state.handle_ingress(5, Some(identity(1, 1, 99)), Ok(parsed(1, b"x"))) {
        IngressDecision::Immediate(reply) => assert_eq!(reply.status, ReplyStatus::Indeterminate),
        _ => panic!("expected indeterminate"),
    }
    match state.handle_ingress(5, Some(identity(1, 0, 5)), Ok(parsed(2, b"x"))) {
        IngressDecision::Immediate(reply) => assert_eq!(reply.status, ReplyStatus::Indeterminate),
        _ => panic!("expected indeterminate"),
    }
    assert_eq!(state.request_len(), 0);
}

#[test]
fn fairness_prefers_other_caller_before_same_caller_tail() {
    let mut state = BrokerState::new();
    for (tid, cell, seq) in [(1, 10, 1), (2, 20, 2), (3, 10, 3)] {
        let _ = state.handle_ingress(
            tid,
            Some(identity(cell, 1, tid as u64)),
            Ok(parsed(seq, b"a")),
        );
    }
    let first = state.take_next_request().unwrap();
    let second = state.take_next_request().unwrap();
    assert_ne!(first.key.cell_id, second.key.cell_id);
}

#[test]
fn completion_rejects_stale_duplicate_and_tracks_counters() {
    let mut state = BrokerState::new();
    let _ = state.handle_ingress(3, Some(identity(7, 8, 3)), Ok(parsed(4, b"zz")));
    let request = state.take_next_request().unwrap();
    let reply = QueuedReply::success(&request);
    state
        .complete_request(&request, reply)
        .expect("first completion");
    assert_eq!(
        state.complete_request(&request, reply),
        Err(CompletionError::Stale)
    );
    assert_eq!(state.counters.duplicate, 1);
    assert_eq!(state.counters.stale_reply, 1);
}

#[test]
fn monotonic_ids_reject_wrap_into_stale_history() {
    let mut state = BrokerState::new();
    state.handle_ingress(4, Some(identity(9, 1, 4)), Ok(parsed(1, b"a")));
    let req = state.take_next_request().unwrap();
    state
        .complete_request(&req, QueuedReply::success(&req))
        .unwrap();
    state.note_network_poll();
    assert_eq!(state.counters.network_progress, 1);
}

#[test]
fn busy_requeue_stays_behind_replies_present_at_turn_start() {
    let mut state = BrokerState::new();
    for (tid, seq) in [(1, 10), (2, 20)] {
        assert_eq!(
            state.handle_ingress(
                tid,
                Some(identity(tid as u64, 1, tid as u64)),
                Ok(parsed(seq, b"x")),
            ),
            IngressDecision::Accepted
        );
    }
    let first_request = state.take_next_request().unwrap();
    let second_request = state.take_next_request().unwrap();
    state
        .complete_request(&first_request, QueuedReply::success(&first_request))
        .unwrap();
    state
        .complete_request(&second_request, QueuedReply::success(&second_request))
        .unwrap();

    let first_reply = state.take_next_reply().unwrap();
    assert!(state.requeue_reply(first_reply));
    let next_reply = state.take_next_reply().unwrap();
    assert_eq!(next_reply.client_sequence, second_request.client_sequence);
}

#[test]
fn reply_retry_state_counts_busy_and_requeues() {
    let mut state = BrokerState::new();
    let reply = QueuedReply::new(5, 6, 7, ReplyStatus::Busy, &[], 0);
    assert!(state.requeue_reply(reply));
    assert_eq!(state.reply_len(), 1);
    state.note_try_send_busy();
    assert_eq!(state.counters.try_send_busy, 1);
}
