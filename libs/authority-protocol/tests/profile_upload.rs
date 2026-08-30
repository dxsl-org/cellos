mod support;
use authority_protocol::*;
use support::*;

struct RecordPolicy([u8; 32]);
impl ProtectedRecordVerifier for RecordPolicy {
    fn verify(&self, record: &ProtectedAuthorityRecord) -> bool {
        self.0 == record.authentication_digest()
    }
}

fn pending() -> TestState {
    let mut authority = state(0, 0);
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut authority,
        &mut challenges,
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    authority
        .begin_enrollment(&begin(4, 1), &Clock(101))
        .unwrap();
    authority
}

#[test]
fn exact_begin_and_chunk_retries_are_idempotent() {
    let digest = [5; 32];
    let mut authority = pending();
    let upload = authority
        .authorize_profile_upload(&begin_upload(5, 1, 1, digest))
        .unwrap();
    assert_eq!(upload.next_index, 0);
    authority.acknowledge_profile_upload(&upload).unwrap();

    let retry = authority
        .authorize_profile_upload(&begin_upload(6, 1, 1, digest))
        .unwrap();
    assert_eq!(retry, upload);
    assert_eq!(authority.acknowledge_profile_upload(&retry), Ok(upload));

    let chunk = authority
        .authorize_profile_chunk(&write_profile(7, 1, digest))
        .unwrap();
    assert_eq!(chunk.mode, ProfileChunkMode::Write);
    let complete = authority.acknowledge_profile_chunk(&chunk).unwrap();
    assert!(complete.complete());

    let retry = authority
        .authorize_profile_chunk(&write_profile(8, 1, digest))
        .unwrap();
    assert_eq!(retry.mode, ProfileChunkMode::VerifyExisting);
    assert_eq!(authority.acknowledge_profile_chunk(&retry), Ok(complete));
    assert_eq!(
        authority.relay_state(),
        RelayProfileState::Uploading(complete)
    );
}

#[test]
fn begin_metadata_mismatch_and_chunk_gap_seal() {
    let digest = [5; 32];
    let mut authority = pending();
    let upload = authority
        .authorize_profile_upload(&begin_upload(5, 1, 1, digest))
        .unwrap();
    authority.acknowledge_profile_upload(&upload).unwrap();
    let mismatch = validated(BeginRelayProfileUploadRequest {
        upload_handle: 45,
        context: context(6, 1, Operation::BeginRelayProfileUpload),
        ..upload_request(6, 1, 1, digest)
    });
    assert_eq!(
        authority.authorize_profile_upload(&mismatch),
        Err(AuthorityFault::ProfileRejected)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);

    let mut authority = pending();
    let mut begin = upload_request(5, 1, 1, digest);
    begin.profile_len = 769;
    let upload = authority
        .authorize_profile_upload(&validated(begin))
        .unwrap();
    authority.acknowledge_profile_upload(&upload).unwrap();
    let gap = validated(WriteRelayProfileChunkRequest {
        context: context(6, 1, Operation::WriteRelayProfileChunk),
        upload_handle: upload.upload_handle,
        chunk_index: 1,
        chunk: Bounded::from_slice(&[5]).unwrap(),
    });
    assert_eq!(
        authority.authorize_profile_chunk(&gap),
        Err(AuthorityFault::ProfileRejected)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
}

#[test]
fn abort_admits_uploading_and_restores_empty_state() {
    let digest = [5; 32];
    let mut authority = pending();
    let upload = authority
        .authorize_profile_upload(&begin_upload(5, 1, 1, digest))
        .unwrap();
    authority.acknowledge_profile_upload(&upload).unwrap();
    let abort = validated(AbortRelayEnrollmentRequest {
        context: context(6, 1, Operation::AbortRelayEnrollment),
        generation: 1,
    });
    assert_eq!(authority.abort(&abort), Ok(1));
    assert_eq!(authority.relay_state(), RelayProfileState::Empty);
}

#[test]
fn authorization_floor_survives_reboot_before_bank_ack() {
    let digest = [5; 32];
    let mut authority = pending();
    let captured = begin_upload(5, 1, 1, digest);
    let intent = authority.authorize_profile_upload(&captured).unwrap();
    assert_eq!(intent.csr_handle, 1);
    let record = authority.into_store().into_record().unwrap();
    assert_eq!(record.bindings().last_request_sequence, 5);
    assert!(matches!(
        record.bindings().relay,
        RelayProfileState::Pending {
            pending_slot: 0,
            ..
        }
    ));
    let verified =
        verify_protected_record(record, &RecordPolicy(record.authentication_digest())).unwrap();
    let mut restored =
        AuthorityState::restore(MemoryStore::from_record(record), &verified, [3; 32]);
    restored.open_boot(&open(6), &measurement()).unwrap();
    assert_eq!(
        restored.authorize_profile_upload(&captured),
        Err(AuthorityFault::Replay)
    );
}

#[test]
fn active_bank_slot_cannot_be_selected_for_next_upload() {
    let digest = [5; 32];
    let mut authority = pending();
    complete_upload(&mut authority, 5, 1, 1, digest);
    authority
        .consume_receipt(&consume(8, 1, 1, digest))
        .unwrap();
    promote(&mut authority, &commit(9, 1, 1, digest));
    let mut challenges = Challenges(8);
    grant_time(
        &mut authority,
        &mut challenges,
        10,
        1,
        TimePurpose::Enrollment,
        2,
        250,
    );
    let enrollment = authority
        .begin_enrollment(&begin(12, 1), &Clock(102))
        .unwrap();
    assert_eq!(enrollment.pending_slot, 1);
    assert!(matches!(
        authority.relay_state(),
        RelayProfileState::Pending {
            pending_slot: 1,
            ..
        }
    ));
    assert_eq!(
        authority.authorize_profile_upload(&begin_upload(13, 1, 2, digest)),
        Err(AuthorityFault::ProfileRejected)
    );
}
