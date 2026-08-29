use super::*;
use crate::c2c_dedup::DEDUP_CAPACITY;
use crate::c2c_envelope::{RelativeDeadline, RetryClass, MAX_C2C_PAYLOAD};
use api::services::cluster::{CellNetId, ClusterId};

fn request(epoch: ServerEpoch, request_id: u64) -> C2cEnvelope<'static> {
    C2cEnvelope {
        kind: EnvelopeKind::Request,
        retry_class: RetryClass::Never,
        request_id,
        src_node: CellNetId([0x11; 32]),
        dst_node: CellNetId([0x22; 32]),
        src_boot_epoch: 3,
        dst_server_epoch: epoch,
        cluster_id: ClusterId(4),
        service_id: 5,
        export_id: 6,
        relative_deadline: RelativeDeadline::new(7).unwrap(),
        payload: &[],
    }
}

#[test]
fn stale_target_never_enters_dedup_state() {
    let current = ServerEpoch::new(2).unwrap();
    let stale = ServerEpoch::new(1).unwrap();
    let mut gate = ReceiveGate::new(current);
    assert_eq!(
        gate.begin(&request(stale, 1), 0),
        DedupDecision::Indeterminate
    );
    assert!(gate.is_empty());
    assert_eq!(gate.begin(&request(current, 1), 0), DedupDecision::Dispatch);
    assert_eq!(gate.len(), 1);
}

#[test]
fn replacement_rejects_old_epoch_before_new_request_admission() {
    let first = ServerEpoch::new(1).unwrap();
    let replacement = ServerEpoch::new(2).unwrap();
    let mut gate = ReceiveGate::new(first);
    assert_eq!(gate.begin(&request(first, 1), 0), DedupDecision::Dispatch);
    gate.replace_server(replacement).unwrap();
    assert_eq!(
        gate.begin(&request(first, 2), 1),
        DedupDecision::Indeterminate
    );
    assert!(gate.is_empty());
    assert_eq!(
        gate.begin(&request(replacement, 2), 1),
        DedupDecision::Dispatch
    );
    assert_eq!(gate.len(), 1);
}

#[test]
fn replacement_retires_saturated_old_epoch_state() {
    let first = ServerEpoch::new(1).unwrap();
    let replacement = ServerEpoch::new(2).unwrap();
    let mut gate = ReceiveGate::new(first);
    for request_id in 1..=DEDUP_CAPACITY as u64 {
        assert_eq!(
            gate.begin(&request(first, request_id), 0),
            DedupDecision::Dispatch
        );
    }
    assert_eq!(gate.len(), DEDUP_CAPACITY);

    gate.replace_server(replacement).unwrap();
    assert_eq!(
        gate.begin(&request(replacement, 1), 1),
        DedupDecision::Indeterminate
    );
    assert!(gate.is_empty());
    assert_eq!(
        gate.begin(&request(replacement, DEDUP_CAPACITY as u64 + 1), 1),
        DedupDecision::Dispatch
    );
}

#[test]
fn replacement_rejects_same_or_lower_epoch_without_mutation() {
    let current = ServerEpoch::new(2).unwrap();
    let mut gate = ReceiveGate::new(current);
    assert_eq!(gate.begin(&request(current, 1), 0), DedupDecision::Dispatch);

    assert_eq!(
        gate.replace_server(current),
        Err(ReplaceServerError::NonIncreasingEpoch)
    );
    assert_eq!(
        gate.replace_server(ServerEpoch::new(1).unwrap()),
        Err(ReplaceServerError::NonIncreasingEpoch)
    );
    assert_eq!(gate.len(), 1);
    assert_eq!(gate.begin(&request(current, 1), 1), DedupDecision::Busy);
    assert_eq!(gate.begin(&request(current, 2), 1), DedupDecision::Dispatch);
}

#[test]
fn non_request_frame_is_not_admitted() {
    let epoch = ServerEpoch::new(1).unwrap();
    let mut gate = ReceiveGate::new(epoch);
    let mut response = request(epoch, 1);
    response.kind = EnvelopeKind::Response;
    response.payload = &[0; MAX_C2C_PAYLOAD];
    assert_eq!(gate.begin(&response, 0), DedupDecision::Indeterminate);
    assert!(gate.is_empty());
}
