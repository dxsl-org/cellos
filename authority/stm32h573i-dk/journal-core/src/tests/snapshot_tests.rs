use super::profile_bank_support::{BankAuth, BankStorage};
use super::successor_tests::{completed_uploading_record, uploading_record, UPLOAD_PROFILE};
use super::support::{encode_full, full_record, identity, pending_record, TestAuth};
use crate::*;
use authority_protocol::RelayProfileState;
use sha2::{Digest, Sha256};

#[test]
fn snapshot_is_issued_only_from_bank_gated_complete_upload() {
    let pending = pending_record();
    let uploading = uploading_record(&pending);
    let complete = completed_uploading_record(&uploading);
    let current_bytes = encode_full(&complete);
    let prior_bytes = encode_full(&uploading);
    let token = recover(
        4,
        [Some(&prior_bytes), Some(&current_bytes)],
        &TestAuth,
        &identity(),
    )
    .unwrap();
    let mut bank = completed_bank(&complete);
    let recovered = bank.validate_recovered(token).unwrap();
    let snapshot = recovered.pending_enrollment_snapshot().unwrap();

    assert_eq!(snapshot.journal_revision(), 4);
    assert_eq!(snapshot.protected_revision(), 4);
    assert_eq!(snapshot.csr_handle(), 1);
    assert_eq!(snapshot.device_id(), &[1; 32]);
    assert_eq!(snapshot.authority_id(), &[2; 32]);
    assert_eq!(snapshot.authority_epoch(), 1);
    assert_eq!(snapshot.boot_epoch(), 1);
    assert_eq!(snapshot.generation(), 1);
    assert_eq!(snapshot.policy_epoch(), 1);
    assert_eq!(snapshot.upload_handle(), 9);
    assert_eq!(snapshot.pending_slot(), 0);
    assert_eq!(snapshot.spki(), b"pending-spki");
    assert_eq!(
        snapshot.spki_digest(),
        &<[u8; 32]>::from(Sha256::digest(b"pending-spki"))
    );
    assert_eq!(snapshot.profile_len(), UPLOAD_PROFILE.len() as u32);
    assert_eq!(
        snapshot.profile_digest(),
        &<[u8; 32]>::from(Sha256::digest(UPLOAD_PROFILE))
    );
    assert_eq!(snapshot.tpm_public_digest(), &[13; 32]);
}

#[test]
fn incomplete_stale_and_non_uploading_states_have_no_snapshot() {
    let pending = pending_record();
    let uploading = uploading_record(&pending);
    let mut partial_bank = upload_bank(&uploading);
    let partial = partial_bank
        .validate_recovered(UnvalidatedRecoveredRecord {
            record: uploading.clone(),
        })
        .unwrap();
    assert!(partial.pending_enrollment_snapshot().is_none());

    let mut complete = completed_uploading_record(&uploading);
    let mut bank = completed_bank(&complete);
    complete.counter += 1;
    let stale = bank
        .validate_recovered(UnvalidatedRecoveredRecord { record: complete })
        .unwrap();
    assert!(stale.pending_enrollment_snapshot().is_none());

    let genesis = full_record(SlotRole::A);
    let genesis_bytes = encode_full(&genesis);
    let pending_bytes = encode_full(&pending);
    let pending_token = recover(
        2,
        [Some(&genesis_bytes), Some(&pending_bytes)],
        &TestAuth,
        &identity(),
    )
    .unwrap();
    let pending = ProfileBank::new(BankStorage::empty(), BankAuth)
        .validate_recovered(pending_token)
        .unwrap();
    assert!(pending.pending_enrollment_snapshot().is_none());
}

pub fn upload_metadata(record: &FullRecord) -> ProfileBankMetadata {
    let RelayProfileState::Uploading(intent) = record.protected.bindings().relay else {
        panic!("uploading record required")
    };
    let pending = record.pending.as_ref().unwrap();
    ProfileBankMetadata {
        slot: intent.pending_slot,
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        boot_epoch: intent.boot_epoch,
        generation: intent.generation,
        policy_epoch: intent.policy_epoch,
        upload_handle: intent.upload_handle,
        profile_len: intent.profile_len,
        profile_digest: intent.profile_digest,
        pending_spki_digest: intent.pending_spki_digest,
        spki: pending.spki,
        tpm_public_digest: intent.tpm_public_digest,
    }
}

fn upload_bank(record: &FullRecord) -> ProfileBank<BankStorage, BankAuth> {
    let metadata = upload_metadata(record);
    let mut bank = ProfileBank::new(BankStorage::empty(), BankAuth);
    bank.initialize(&metadata).unwrap();
    bank
}

fn completed_bank(record: &FullRecord) -> ProfileBank<BankStorage, BankAuth> {
    let metadata = upload_metadata(record);
    let mut bank = upload_bank(record);
    bank.write_chunk(&metadata, 0, 0, &UPLOAD_PROFILE).unwrap();
    bank
}
