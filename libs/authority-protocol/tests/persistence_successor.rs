mod support;
use authority_protocol::*;
use support::*;

#[test]
fn exact_same_state_revision_successor_is_allowed() {
    let first = opened_record();
    let second = rewrite(&first, 2, |_| {});
    assert_eq!(verify_protected_successor(&first, &second), Ok(()));
}

#[test]
fn sealed_record_is_absorbing() {
    let serving = opened_record();
    let sealed = rewrite(&serving, 2, |bytes| bytes[13] = 3);
    assert_eq!(verify_protected_successor(&serving, &sealed), Ok(()));
    let next_serving = rewrite(&serving, 3, |_| {});
    assert_eq!(
        verify_protected_successor(&sealed, &next_serving),
        Err(AuthorityFault::PersistenceFailure)
    );
}

#[test]
fn unchanged_time_state_cannot_advance_time_floors() {
    let opened = opened_record();
    let jumped = rewrite(&opened, 2, |bytes| {
        let floors = bytes.len() - 24;
        bytes[floors..].fill(0xff);
    });
    assert_eq!(
        verify_protected_successor(&opened, &jumped),
        Err(AuthorityFault::PersistenceFailure)
    );
}

#[test]
fn pending_time_request_is_one_legal_edge() {
    let opened = opened_record();
    let pending = pending_time_record();
    assert_eq!(verify_protected_successor(&opened, &pending), Ok(()));
}

#[test]
fn accepted_time_binds_pending_identity_purpose_and_strict_floors() {
    let pending = pending_time_record();
    let accepted = accepted_time_record();
    assert_eq!(verify_protected_successor(&pending, &accepted), Ok(()));

    for changed in [
        rewrite(&accepted, 3, |bytes| bytes[48] ^= 1),
        rewrite(&accepted, 3, |bytes| {
            bytes[64] = TimePurpose::TlsCertificateVerify as u8
        }),
        rewrite(&accepted, 3, |bytes| {
            bytes[24..40].fill(0);
            let floors = bytes.len() - 24;
            bytes[floors..].fill(0);
        }),
    ] {
        assert_eq!(
            verify_protected_successor(&pending, &changed),
            Err(AuthorityFault::PersistenceFailure)
        );
    }
}

#[test]
fn pending_time_challenge_cannot_change_without_a_legal_edge() {
    let pending = pending_time_record();
    let changed = rewrite(&pending, 3, |bytes| {
        let nonce_tail = bytes.len() - 25;
        bytes[nonce_tail] ^= 1;
    });
    assert_eq!(
        verify_protected_successor(&pending, &changed),
        Err(AuthorityFault::PersistenceFailure)
    );
}

fn opened_record() -> ProtectedAuthorityRecord {
    let mut authority = state(0, 0);
    authority.open_boot(&open(1), &measurement()).unwrap();
    authority.into_store().into_record().unwrap()
}

fn accepted_time_record() -> ProtectedAuthorityRecord {
    let mut authority = state(0, 0);
    authority.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut authority,
        &mut Challenges(4),
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    authority.into_store().into_record().unwrap()
}

fn pending_time_record() -> ProtectedAuthorityRecord {
    let mut authority = state(0, 0);
    authority.open_boot(&open(1), &measurement()).unwrap();
    let request = validated(RequestSignedTimeRequest {
        context: context(2, 1, Operation::RequestSignedTime),
        purpose: TimePurpose::Enrollment as u8,
    });
    authority
        .request_signed_time(&request, &mut Challenges(4))
        .unwrap();
    authority.into_store().into_record().unwrap()
}

fn rewrite(
    record: &ProtectedAuthorityRecord,
    revision: u64,
    mutate: impl FnOnce(&mut [u8]),
) -> ProtectedAuthorityRecord {
    let mut bytes = [0u8; PROTECTED_RECORD_MAX];
    let length = record.encode_canonical(&mut bytes).unwrap();
    bytes[5..13].copy_from_slice(&revision.to_le_bytes());
    mutate(&mut bytes[..length]);
    ProtectedAuthorityRecord::decode_canonical(&bytes[..length]).unwrap()
}
