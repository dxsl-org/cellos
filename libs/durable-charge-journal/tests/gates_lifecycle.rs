use durable_charge_journal::{AuthorityGate, GateStatus, JournalError, ObligationStatus};

#[test]
fn reserve_and_resolve_lifecycle() {
    let mut gate = AuthorityGate::new([1u8; 32], [2u8; 32], 1, [3u8; 32], [1000, 100]);
    let writer = [3u8; 32];
    let ob_id = [9u8; 16];

    gate.reserve(ob_id, [1u8; 32], [2u8; 32], [4u8; 16], 1, [5u8; 16], [100, 10], &writer, 1).expect("reserve");
    assert_eq!(gate.charged, [100, 10]);
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.status, ObligationStatus::Unknown);

    // Resolve creation
    gate.resolve_creation(&ob_id, [100, 10], &writer, 1).expect("resolve");
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.status, ObligationStatus::Applied);
    assert_eq!(ob.effective_vector, [100, 10]);
}

#[test]
fn two_plane_mixed_resize_accounting() {
    let mut gate = AuthorityGate::new([1u8; 32], [2u8; 32], 1, [3u8; 32], [1000, 100]);
    let writer = [3u8; 32];
    let ob_id = [9u8; 16];
    gate.reserve(ob_id, [1u8; 32], [2u8; 32], [4u8; 16], 1, [5u8; 16], [100, 10], &writer, 1).unwrap();
    gate.resolve_creation(&ob_id, [100, 10], &writer, 1).unwrap();

    // Mixed grow/shrink resize: (100, 10) -> (140, 6)
    // G = max(Q - B, 0) = (40, 0)
    // L = max(Q - E, 0) = (40, 0)
    // Total charged increases by G to (140, 10)
    let resize_key = [7u8; 16];
    gate.resize(&ob_id, resize_key, [140, 6], &writer, 1).expect("resize");
    assert_eq!(gate.charged, [140, 10]);

    // Apply: fold positive delta, E becomes (140, 6), B is (140, 10)
    gate.resize_apply(&ob_id, &resize_key, &writer, 1).expect("apply");
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.effective_vector, [140, 6]);
    assert_eq!(ob.base_vector, [140, 10]);
    assert_eq!(gate.charged, [140, 10]); // Unchanged by fold!

    // Partial shrink credit installment 1: credit (0, 2) -> leaves B=(140, 8), charged=(140, 8)
    gate.shrink_credit(&ob_id, &resize_key, [0, 2], &writer, 1).expect("credit 1");
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.base_vector, [140, 8]);
    assert_eq!(gate.charged, [140, 8]);

    // Final shrink credit installment 2: credit (0, 2) -> leaves B=(140, 6) == E
    gate.shrink_credit(&ob_id, &resize_key, [0, 2], &writer, 1).expect("credit 2");
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.base_vector, [140, 6]);
    assert_eq!(gate.charged, [140, 6]);
    assert!(ob.resize.is_none()); // Fully settled!
}

#[test]
fn closing_intent_blocks_further_resize_and_allows_release() {
    let mut gate = AuthorityGate::new([1u8; 32], [2u8; 32], 1, [3u8; 32], [1000, 100]);
    let writer = [3u8; 32];
    let ob_id = [9u8; 16];
    gate.reserve(ob_id, [1u8; 32], [2u8; 32], [4u8; 16], 1, [5u8; 16], [100, 10], &writer, 1).unwrap();
    gate.resolve_creation(&ob_id, [100, 10], &writer, 1).unwrap();

    // Close intent
    gate.close_intent(&ob_id, &writer, 1).expect("close intent");
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.status, ObligationStatus::Closing);

    // Further resize rejected!
    assert_eq!(gate.resize(&ob_id, [8u8; 16], [120, 10], &writer, 1), Err(JournalError::State));

    // Release once: credits all remaining [100, 10], leaves charged at [0, 0]
    gate.release_once(&ob_id, &writer, 1).expect("release once");
    assert_eq!(gate.charged, [0, 0]);
    let ob = gate.find_obligation(&ob_id).unwrap();
    assert_eq!(ob.status, ObligationStatus::Released);
    assert_eq!(ob.base_vector, [0, 0]);
}

#[test]
fn bootstrap_adoption_and_reconciliation_gate() {
    let mut gate = AuthorityGate::new([1u8; 32], [2u8; 32], 1, [3u8; 32], [1000, 100]);
    let old_writer = [3u8; 32];
    let new_writer = [4u8; 32];

    gate.prepare_adoption(&[99u8; 32], &old_writer, 1).expect("prepare");
    assert_eq!(gate.gate_status, GateStatus::Fenced);

    // Accept adoption: enters Reconciling gate
    gate.accept_adoption(new_writer, 2).expect("accept");
    assert_eq!(gate.gate_status, GateStatus::Reconciling);

    // Normal mutation is closed while reconciling!
    assert_eq!(gate.reserve([9u8; 16], [1u8; 32], [2u8; 32], [5u8; 16], 1, [6u8; 16], [50, 5], &new_writer, 2), Err(JournalError::GateClosed));

    // Activation opens the gate
    gate.activate(&new_writer, 2).expect("activate");
    assert_eq!(gate.gate_status, GateStatus::Open);

    // Now mutation succeeds!
    gate.reserve([9u8; 16], [1u8; 32], [2u8; 32], [5u8; 16], 1, [6u8; 16], [50, 5], &new_writer, 2).expect("reserve after activate");
    assert_eq!(gate.charged, [50, 5]);
}
