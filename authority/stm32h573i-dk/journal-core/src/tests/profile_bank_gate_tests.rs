use super::codec_tests::active_record_at;
use super::profile_bank_support::{BankAuth, BankStorage};
use super::snapshot_tests::upload_metadata;
use super::successor_tests::{completed_uploading_record, uploading_record, UPLOAD_PROFILE};
use super::support::pending_record;
use crate::*;
use authority_protocol::Bounded;
use sha2::{Digest, Sha256};

#[test]
fn service_gate_rejects_missing_and_rolled_back_active_banks() {
    let bytes = [0x5a; 100];
    let metadata = active_metadata(1, &bytes);
    let mut bank = ProfileBank::new(BankStorage::empty(), BankAuth);
    bank.initialize(&metadata).unwrap();
    let next = bank.write_chunk(&metadata, 0, 0, &bytes).unwrap();
    bank.complete(&metadata, next).unwrap();
    let (storage, _) = bank.into_parts();

    let token = UnvalidatedRecoveredRecord {
        record: active_record_at(1),
    };
    let mut valid = ProfileBank::new(storage.clone(), BankAuth);
    assert!(valid.validate_recovered(token).is_ok());

    let missing = UnvalidatedRecoveredRecord {
        record: active_record_at(1),
    };
    let mut absent = ProfileBank::new(BankStorage::empty(), BankAuth);
    assert_eq!(absent.validate_recovered(missing), Err(BankError::Sealed));

    let rollback = UnvalidatedRecoveredRecord {
        record: active_record_at(2),
    };
    let mut old = ProfileBank::new(storage, BankAuth);
    assert_eq!(old.validate_recovered(rollback), Err(BankError::Sealed));
}

#[test]
fn uploading_gate_requires_header_and_complete_profile_hash() {
    let uploading = uploading_record(&pending_record());
    let metadata = upload_metadata(&uploading);
    let token = UnvalidatedRecoveredRecord {
        record: uploading.clone(),
    };
    let mut missing = ProfileBank::new(BankStorage::empty(), BankAuth);
    assert_eq!(missing.validate_recovered(token), Err(BankError::Sealed));

    let mut partial = ProfileBank::new(BankStorage::empty(), BankAuth);
    partial.initialize(&metadata).unwrap();
    let recovered = partial
        .validate_recovered(UnvalidatedRecoveredRecord { record: uploading })
        .unwrap();
    assert!(recovered.pending_enrollment_snapshot().is_none());

    let complete = completed_uploading_record(recovered.record());
    let mut missing_complete = ProfileBank::new(BankStorage::empty(), BankAuth);
    assert_eq!(
        missing_complete.validate_recovered(UnvalidatedRecoveredRecord {
            record: complete.clone(),
        }),
        Err(BankError::Sealed)
    );

    let mut wrong = ProfileBank::new(BankStorage::empty(), BankAuth);
    wrong.initialize(&metadata).unwrap();
    wrong.write_chunk(&metadata, 0, 0, &[0x33; 100]).unwrap();
    assert_eq!(
        wrong.validate_recovered(UnvalidatedRecoveredRecord { record: complete }),
        Err(BankError::Sealed)
    );

    let mut valid = ProfileBank::new(BankStorage::empty(), BankAuth);
    valid.initialize(&metadata).unwrap();
    valid.write_chunk(&metadata, 0, 0, &UPLOAD_PROFILE).unwrap();
    let complete = completed_uploading_record(&uploading_record(&pending_record()));
    assert!(valid
        .validate_recovered(UnvalidatedRecoveredRecord { record: complete })
        .unwrap()
        .pending_enrollment_snapshot()
        .is_some());
}

fn active_metadata(generation: u64, bytes: &[u8]) -> ProfileBankMetadata {
    ProfileBankMetadata {
        slot: 0,
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 1,
        boot_epoch: 1,
        generation,
        policy_epoch: 1,
        upload_handle: 9,
        profile_len: bytes.len() as u32,
        profile_digest: Sha256::digest(bytes).into(),
        pending_spki_digest: Sha256::digest(b"bank-spki").into(),
        spki: Bounded::from_slice(b"bank-spki").unwrap(),
        tpm_public_digest: [23; 32],
    }
}
