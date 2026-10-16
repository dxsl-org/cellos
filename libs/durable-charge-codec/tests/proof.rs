use durable_charge_codec::{decode, encode, schema_id, Header, Limits};
use durable_charge_codec::proof::{
    self, Bindings, Lane, Owner, ProofError, Purpose, RecordBinding, Role, Side,
    Status, VerifyContext, BODY_LEN, CARRIER_LEN, SIGNED_LEN, SIGNING_DOMAIN,
};
use ed25519_compact::{KeyPair, Seed};

// These deterministic host-only seeds are public regression fixtures, NOT native
// assignments, production keys, producer truth or storage qualification evidence.
fn key(seed: u8) -> KeyPair { KeyPair::from_seed(Seed::new([seed; 32])) }
fn limits() -> Limits {
    Limits { max_record_bytes: 4096, max_entries: 32, max_records: 16,
        max_reference_depth: 16, max_total_bytes: 65536, max_work: 100000 }
}
fn owner(n: u8) -> Owner {
    Owner { principal: [n; 32], incarnation: [n + 1; 32], home: [n + 2; 32], epoch: 3 }
}
fn record_bytes() -> [u8; 308] {
    let h = Header { journal_ledger: [21; 32], journal_genesis: [22; 32],
        obligation_id: [0; 16], adoption_id: [0; 16], local_transaction: [23; 16],
        local_revision: 3, journal_writer_epoch: 4, previous_record_digest: [24; 32] };
    let mut payload = [0; 60];
    payload[..32].fill(25);
    payload[32..36].copy_from_slice(&1u32.to_le_bytes());
    payload[36..38].copy_from_slice(&1u16.to_le_bytes());
    payload[38..40].copy_from_slice(&1u16.to_le_bytes());
    payload[44..52].copy_from_slice(&5u64.to_le_bytes());
    payload[52..60].copy_from_slice(&1u64.to_le_bytes());
    let mut out = [0; 308];
    assert_eq!(encode(32, &h, &payload, &mut out, &limits()).unwrap(), out.len());
    out
}
fn context(bytes: &[u8], keys: &KeyPair) -> VerifyContext {
    let record = decode(bytes, &limits()).unwrap();
    let h = &record.header;
    let mut public_key = [0; 32];
    public_key.copy_from_slice(&keys.pk[..]);
    VerifyContext { public_key, max_carrier_bytes: CARRIER_LEN, record_limits: limits(), expected: Bindings {
        role: Role::OwnerCommit, purpose: Purpose::CommitResult, lane: Lane::Fixture,
        side: Side::Local, status: Status::Committed,
        producer: owner(1), key_id: [4; 32], roster_id: [5; 32], roster_revision: 6,
        revocation_revision: 7, audience: [8; 32], admission_decision: [9; 32],
        admission_revision: 10, policy_id: [11; 32], policy_revision: 12,
        application: [13; 32], workload: [14; 32], scope_purpose: [15; 32],
        journal: h.journal_ledger, genesis: h.journal_genesis, volume: [16; 32],
        floor_namespace: [17; 32], obligation: h.obligation_id, adoption: h.adoption_id,
        transaction: h.local_transaction, origin_journal: [0; 32], origin_genesis: [0; 32],
        origin_transaction: [0; 16], origin_revision: 0,
        create_key: [0; 16], operation_key: [0; 16], release_key: [0; 16],
        transition_map: [0; 32], request_schema: [26; 32], request_length: 27,
        request_digest: [28; 32], effect: [29; 32], profile: [25; 32],
        destination: [30; 32], requesting_endpoint: [31; 32], responding_endpoint: [32; 32],
        correlation: [33; 32], provenance: owner(34), custodian: owner(37),
        writer_epoch: h.journal_writer_epoch, authority_fence: 40, controller_epoch: 41,
        custody_revision: 42, allocation_revision: 0, input_revision: 2,
        output_revision: h.local_revision, input_floor_revision: 43, output_floor_revision: 44,
        input_record: RecordBinding { encoded_length: 308, kind: 32, digest: h.previous_record_digest },
        record: RecordBinding { encoded_length: bytes.len() as u32, kind: record.kind, digest: record.digest },
        record_schema: schema_id(record.kind).unwrap(), coverage_roster: [45; 32],
        coverage_revision: 46, coverage_count: 1, checkpoint: [47; 32],
        backend_admission: [48; 32], failure_model: [49; 32],
    } }
}
fn put(out: &mut [u8], at: usize, value: &[u8]) {
    out[at..at + value.len()].copy_from_slice(value);
}
fn put_owner(out: &mut [u8], at: usize, owner: &Owner) {
    put(out, at, &owner.principal); put(out, at + 32, &owner.incarnation);
    put(out, at + 64, &owner.home); put(out, at + 96, &owner.epoch.to_le_bytes());
}
fn put_ref(out: &mut [u8], at: usize, r: &RecordBinding) {
    put(out, at, &r.encoded_length.to_le_bytes()); put(out, at + 4, &r.kind.to_le_bytes());
    put(out, at + 8, &r.digest);
}
fn resign(out: &mut [u8; CARRIER_LEN], keys: &KeyPair) {
    let mut message = [0; SIGNING_DOMAIN.len() + SIGNED_LEN];
    message[..SIGNING_DOMAIN.len()].copy_from_slice(SIGNING_DOMAIN);
    message[SIGNING_DOMAIN.len()..].copy_from_slice(&out[..SIGNED_LEN]);
    let signature = keys.sk.sign(message, None);
    out[SIGNED_LEN..].copy_from_slice(&signature[..]);
}
fn carrier(context: &VerifyContext, keys: &KeyPair) -> [u8; CARRIER_LEN] {
    let b = &context.expected;
    let mut out = [0; CARRIER_LEN];
    put(&mut out, 0, b"NCF1"); put(&mut out, 4, &1u16.to_le_bytes());
    put(&mut out, 6, &1u16.to_le_bytes()); put(&mut out, 8, &(CARRIER_LEN as u32).to_le_bytes());
    put(&mut out, 12, &(BODY_LEN as u32).to_le_bytes());
    put(&mut out, 16, &(b.role as u16).to_le_bytes());
    put(&mut out, 18, &(b.purpose as u16).to_le_bytes());
    put(&mut out, 20, &(b.lane as u16).to_le_bytes());
    put(&mut out, 22, &(b.side as u16).to_le_bytes());
    put(&mut out, 24, &(b.status as u16).to_le_bytes());
    put(&mut out, 26, &1u16.to_le_bytes());
    put_owner(&mut out, 32, &b.producer);
    put(&mut out, 136, &b.key_id); put(&mut out, 168, &b.roster_id);
    put(&mut out, 200, &b.roster_revision.to_le_bytes());
    put(&mut out, 208, &b.revocation_revision.to_le_bytes());
    put(&mut out, 216, &b.audience); put(&mut out, 248, &b.admission_decision);
    put(&mut out, 280, &b.admission_revision.to_le_bytes()); put(&mut out, 288, &b.policy_id);
    put(&mut out, 320, &b.policy_revision.to_le_bytes());
    put(&mut out, 328, &b.application); put(&mut out, 360, &b.workload);
    put(&mut out, 392, &b.scope_purpose); put(&mut out, 424, &b.journal);
    put(&mut out, 456, &b.genesis); put(&mut out, 488, &b.volume);
    put(&mut out, 520, &b.floor_namespace); put(&mut out, 552, &b.obligation);
    put(&mut out, 568, &b.adoption); put(&mut out, 584, &b.transaction);
    put(&mut out, 600, &b.origin_journal); put(&mut out, 632, &b.origin_genesis);
    put(&mut out, 664, &b.origin_transaction); put(&mut out, 680, &b.origin_revision.to_le_bytes());
    put(&mut out, 688, &b.create_key); put(&mut out, 704, &b.operation_key);
    put(&mut out, 720, &b.release_key); put(&mut out, 736, &b.transition_map);
    put(&mut out, 768, &b.request_schema); put(&mut out, 800, &b.request_length.to_le_bytes());
    put(&mut out, 808, &b.request_digest); put(&mut out, 840, &b.effect);
    put(&mut out, 872, &b.profile); put(&mut out, 904, &b.destination);
    put(&mut out, 936, &b.requesting_endpoint); put(&mut out, 968, &b.responding_endpoint);
    put(&mut out, 1000, &b.correlation); put_owner(&mut out, 1032, &b.provenance);
    put_owner(&mut out, 1136, &b.custodian); put(&mut out, 1240, &b.writer_epoch.to_le_bytes());
    put(&mut out, 1248, &b.authority_fence.to_le_bytes());
    put(&mut out, 1256, &b.controller_epoch.to_le_bytes());
    put(&mut out, 1264, &b.custody_revision.to_le_bytes());
    put(&mut out, 1272, &b.allocation_revision.to_le_bytes());
    put(&mut out, 1280, &b.input_revision.to_le_bytes());
    put(&mut out, 1288, &b.output_revision.to_le_bytes());
    put(&mut out, 1296, &b.input_floor_revision.to_le_bytes());
    put(&mut out, 1304, &b.output_floor_revision.to_le_bytes());
    put_ref(&mut out, 1312, &b.input_record); put_ref(&mut out, 1352, &b.record);
    put(&mut out, 1392, &b.record_schema); put(&mut out, 1424, &b.coverage_roster);
    put(&mut out, 1456, &b.coverage_revision.to_le_bytes());
    put(&mut out, 1464, &b.coverage_count.to_le_bytes()); put(&mut out, 1468, &b.checkpoint);
    put(&mut out, 1500, &b.backend_admission); put(&mut out, 1532, &b.failure_model);
    put(&mut out, 1564, &context.public_key);
    resign(&mut out, keys);
    out
}
fn rejected(carrier: &[u8], bytes: &[u8], context: &VerifyContext) -> ProofError {
    let record = decode(bytes, &limits()).unwrap();
    proof::verify(carrier, &record, context).unwrap_err()
}

#[test]
fn deterministic_signature_yields_only_the_borrowed_assigned_statement() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let first = carrier(&context, &keys); let second = carrier(&context, &keys);
    assert_eq!(first, second);
    let record = decode(&bytes, &limits()).unwrap();
    let fact = proof::verify(&first, &record, &context).unwrap();
    assert_eq!(fact.role(), Role::OwnerCommit);
    assert_eq!(fact.purpose(), Purpose::CommitResult);
    assert_eq!(fact.status(), Status::Committed);
    assert!(core::ptr::eq(fact.carrier().as_ptr(), first.as_ptr()));
    assert!(core::ptr::eq(fact.record_bytes().as_ptr(), bytes.as_ptr()));
    assert!(core::ptr::eq(fact.statement(), &context.expected));
}

#[test]
fn every_changed_signed_byte_and_signature_byte_rejects() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let original = carrier(&context, &keys);
    for at in 0..CARRIER_LEN {
        let mut changed = original; changed[at] ^= 1;
        let record = decode(&bytes, &limits()).unwrap();
        assert!(proof::verify(&changed, &record, &context).is_err(), "byte {at}");
    }
}

#[test]
fn valid_signatures_cannot_substitute_any_context_field() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let original = carrier(&context, &keys);
    // Every body field, each identity component, every counter, both reference
    // length/kind/digest bindings, public pin, and finite header discriminator.
    let offsets = [16,18,20,22,24,26,32,64,96,128,136,168,200,208,216,248,280,
        288,320,328,360,392,424,456,488,520,552,568,584,600,632,664,680,688,
        704,720,736,768,800,808,840,872,904,936,968,1000,1032,1064,1096,1128,
        1136,1168,1200,1232,1240,1248,1256,1264,1272,1280,1288,1296,1304,
        1312,1316,1320,1352,1356,1360,1392,1424,1456,1464,1468,1500,1532,1564];
    for at in offsets {
        let mut changed = original; changed[at] ^= 1; resign(&mut changed, &keys);
        let record = decode(&bytes, &limits()).unwrap();
        assert!(proof::verify(&changed, &record, &context).is_err(), "binding {at}");
    }
}

#[test]
fn valid_wrong_role_and_wrong_purpose_are_not_generic_authority() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let mut changed = carrier(&context, &keys);
    put(&mut changed, 16, &(Role::NonregressionFloor as u16).to_le_bytes());
    put(&mut changed, 18, &(Purpose::Floor as u16).to_le_bytes());
    resign(&mut changed, &keys);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::Context);
    put(&mut changed, 16, &(Role::OwnerCommit as u16).to_le_bytes());
    resign(&mut changed, &keys);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::RolePurpose);
}

#[test]
fn public_pin_is_external_and_cannot_be_replaced_by_the_carrier() {
    let keys = key(7); let other = key(8); let bytes = record_bytes();
    let context = context(&bytes, &keys); let mut changed = carrier(&context, &keys);
    resign(&mut changed, &other);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::Signature);
    put(&mut changed, 1564, &other.pk[..]); resign(&mut changed, &other);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::PublicPin);
}

#[test]
fn wrong_signing_domain_and_noncanonical_signature_scalar_reject() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let mut changed = carrier(&context, &keys);
    // A signature over the raw carrier, without the exact domain, is not valid.
    let signature = keys.sk.sign(&changed[..SIGNED_LEN], None);
    changed[SIGNED_LEN..].copy_from_slice(&signature[..]);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::Signature);
    let mut changed = carrier(&context, &keys); changed[SIGNED_LEN + 32..].fill(255);
    assert_eq!(rejected(&changed, &bytes, &context), ProofError::Signature);
}

#[test]
fn current_fence_roster_owner_and_all_revision_expectations_are_exact() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let signed = carrier(&context, &keys);
    let mut contexts = [context.clone(), context.clone(), context.clone(), context.clone(),
        context.clone(), context.clone(), context.clone(), context.clone(), context.clone(),
        context.clone(), context.clone(), context.clone()];
    contexts[0].expected.authority_fence += 1;
    contexts[1].expected.roster_revision += 1;
    contexts[2].expected.revocation_revision += 1;
    contexts[3].expected.custodian.epoch += 1;
    contexts[4].expected.producer.incarnation[0] ^= 1;
    contexts[5].expected.writer_epoch += 1;
    contexts[6].expected.controller_epoch += 1;
    contexts[7].expected.custody_revision += 1;
    contexts[8].expected.allocation_revision += 1;
    contexts[9].expected.input_revision += 1;
    contexts[10].expected.output_revision += 1;
    contexts[11].expected.input_floor_revision += 1;
    for stale in contexts { assert_eq!(rejected(&signed, &bytes, &stale), ProofError::Context); }
}

#[test]
fn unknown_format_reserved_and_size_or_work_exhaustion_reject() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let original = carrier(&context, &keys);
    for (at, expected) in [(4, ProofError::Version),(6, ProofError::Suite),
        (16, ProofError::Discriminant),(18, ProofError::Discriminant),
        (20, ProofError::Discriminant),(22, ProofError::Discriminant),
        (24, ProofError::Discriminant),(26, ProofError::Discriminant),
        (28, ProofError::Reserved),(1318, ProofError::Reserved),(1358, ProofError::Reserved)] {
        let mut changed = original; changed[at] = 255;
        assert_eq!(rejected(&changed, &bytes, &context), expected, "offset {at}");
    }
    assert_eq!(rejected(&original[..CARRIER_LEN - 1], &bytes, &context), ProofError::Length);
    let mut overlong = [0; CARRIER_LEN + 1]; overlong[..CARRIER_LEN].copy_from_slice(&original);
    assert_eq!(rejected(&overlong, &bytes, &context), ProofError::Limit);
    let mut short_limit = context.clone(); short_limit.max_carrier_bytes -= 1;
    assert_eq!(rejected(&original, &bytes, &short_limit), ProofError::Limit);
    let mut short_work = context.clone(); short_work.record_limits.max_work = 0;
    assert_eq!(rejected(&original, &bytes, &short_work), ProofError::Record);
}

#[test]
fn signed_record_binding_must_match_actual_canonical_retained_bytes() {
    let keys = key(7); let bytes = record_bytes(); let mut context = context(&bytes, &keys);
    context.expected.record.digest[0] ^= 1;
    let signed = carrier(&context, &keys);
    let mut fabricated = decode(&bytes, &limits()).unwrap();
    fabricated.digest = context.expected.record.digest;
    assert_eq!(proof::verify(&signed, &fabricated, &context).unwrap_err(), ProofError::Record);
    let mut corrupt = bytes; corrupt[300] ^= 1;
    fabricated.bytes = &corrupt;
    assert_eq!(proof::verify(&signed, &fabricated, &context).unwrap_err(), ProofError::Record);
}

#[test]
fn zero_and_exhausted_context_counters_are_not_silently_reinitialized() {
    let keys = key(7); let bytes = record_bytes(); let mut context = context(&bytes, &keys);
    for value in [0, u64::MAX] {
        context.expected.authority_fence = value;
        let signed = carrier(&context, &keys);
        assert_eq!(rejected(&signed, &bytes, &context), ProofError::Counter);
    }
}

#[test]
fn authenticated_unresolved_is_preserved_as_unresolved_not_terminal_absence() {
    let keys = key(7); let bytes = record_bytes(); let mut context = context(&bytes, &keys);
    context.expected.status = Status::Unresolved;
    let signed = carrier(&context, &keys); let record = decode(&bytes, &limits()).unwrap();
    let fact = proof::verify(&signed, &record, &context).unwrap();
    assert_eq!(fact.status(), Status::Unresolved);
}

#[test]
fn all_defined_role_purpose_pairs_are_distinct_typed_statements() {
    let keys = key(7); let bytes = record_bytes(); let mut context = context(&bytes, &keys);
    let pairs = [
        (Role::OwnerCommit, Purpose::CommitResult),
        (Role::NonregressionFloor, Purpose::Floor),
        (Role::WriterFence, Purpose::InstallFence),
        (Role::CompleteCoverage, Purpose::CompleteCoverage),
        (Role::CompleteCoverage, Purpose::CompleteReconstruction),
        (Role::Destination, Purpose::DestinationBinding),
        (Role::Destination, Purpose::LocalReservation),
        (Role::Destination, Purpose::WholeVectorApplied),
        (Role::Closure, Purpose::FullClosure),
        (Role::ResourceOutcome, Purpose::WholeVectorApplied),
        (Role::ResourceOutcome, Purpose::ShrinkQuiescence),
        (Role::ResourceOutcome, Purpose::NoApplication),
        (Role::StateEffectOutcome, Purpose::WholeVectorApplied),
        (Role::StateEffectOutcome, Purpose::ShrinkQuiescence),
        (Role::StateEffectOutcome, Purpose::NoApplication),
        (Role::Succession, Purpose::AuthorizedSuccession),
        (Role::Issuer, Purpose::ImmutableMaximumProfile),
        (Role::Issuer, Purpose::GenesisAuthorization),
        (Role::Lookup, Purpose::CurrentLookup),
        (Role::BootLifecycle, Purpose::BootGone),
    ];
    for (role, purpose) in pairs {
        context.expected.role = role; context.expected.purpose = purpose;
        let signed = carrier(&context, &keys); let record = decode(&bytes, &limits()).unwrap();
        let fact = proof::verify(&signed, &record, &context).unwrap();
        assert_eq!((fact.role(), fact.purpose()), (role, purpose));
    }
}

#[test]
fn even_expected_valid_signatures_cannot_lie_about_the_bound_record() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let mut contexts = [context.clone(), context.clone(), context.clone(), context.clone(),
        context.clone(), context.clone(), context.clone(), context.clone()];
    contexts[0].expected.record.encoded_length += 1;
    contexts[1].expected.record.kind = 33;
    contexts[1].expected.record_schema = schema_id(33).unwrap();
    contexts[2].expected.record_schema[0] ^= 1;
    contexts[3].expected.journal[0] ^= 1;
    contexts[4].expected.transaction[0] ^= 1;
    contexts[5].expected.input_record.digest[0] ^= 1;
    contexts[6].expected.writer_epoch += 1;
    contexts[7].expected.profile[0] ^= 1;
    for contradictory in contexts {
        let signed = carrier(&contradictory, &keys);
        assert_eq!(rejected(&signed, &bytes, &contradictory), ProofError::Record);
    }
}

#[test]
fn explicit_inapplicability_cannot_hide_a_debit_origin_or_adoption_map() {
    let keys = key(7); let bytes = record_bytes(); let context = context(&bytes, &keys);
    let mut contexts = [context.clone(), context.clone(), context.clone(), context.clone()];
    contexts[0].expected.origin_journal[0] = 1;
    contexts[1].expected.origin_revision = 1;
    contexts[2].expected.create_key[0] = 1;
    contexts[3].expected.transition_map[0] = 1;
    for impossible in contexts {
        let signed = carrier(&impossible, &keys);
        assert_eq!(rejected(&signed, &bytes, &impossible), ProofError::Identity);
    }
}

#[test]
fn signed_expected_scope_must_also_equal_the_actual_dco1_payload_scope() {
    let keys = key(7);
    let header = Header { journal_ledger: [21; 32], journal_genesis: [22; 32],
        obligation_id: [0; 16], adoption_id: [50; 16], local_transaction: [23; 16],
        local_revision: 3, journal_writer_epoch: 4, previous_record_digest: [24; 32] };
    let mut payload = [0; 148];
    payload[..32].fill(13); payload[32..64].fill(14); payload[64..96].fill(15);
    put(&mut payload, 96, &(Lane::Fixture as u32).to_le_bytes());
    put(&mut payload, 100, &12u64.to_le_bytes());
    payload[108..124].fill(50); put(&mut payload, 124, &1u32.to_le_bytes());
    payload[129] = 9; payload[132..148].fill(51);
    let mut bytes = [0; 396];
    assert_eq!(encode(35, &header, &payload, &mut bytes, &limits()).unwrap(), bytes.len());
    let mut context = context(&bytes, &keys); context.expected.transition_map = [52; 32];
    let signed = carrier(&context, &keys); let record = decode(&bytes, &limits()).unwrap();
    proof::verify(&signed, &record, &context).unwrap();
    let mut contexts = [context.clone(), context.clone(), context.clone(), context.clone(), context.clone()];
    contexts[0].expected.application[0] ^= 1;
    contexts[1].expected.workload[0] ^= 1;
    contexts[2].expected.scope_purpose[0] ^= 1;
    contexts[3].expected.lane = Lane::Native;
    contexts[4].expected.policy_revision += 1;
    for contradictory in contexts {
        let signed = carrier(&contradictory, &keys);
        assert_eq!(rejected(&signed, &bytes, &contradictory), ProofError::Record);
    }
}
