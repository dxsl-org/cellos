use super::snapshot_flow::pending_state;
use super::snapshot_support::{Auth, MemoryBank};
use super::upload_cut_support::*;
use authority_protocol::{AuthorityMode, ProtectedAuthorityRecord, RelayProfileState};
use core::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use stm32_authority_journal::{
    begin_profile_upload, write_profile_chunk, BankError, ProfileBank, ProfileBankStorage,
    UploadFlowError,
};

fn assert_pending_after_begin_cut(cut: MediaCut, acknowledge_next: Option<u8>) {
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let acknowledge = Cell::new(acknowledge_next);
    let acknowledge_fired = Cell::new(false);
    let mut state = pending_state(CutStore {
        revision: &revision,
        record: &record,
        acknowledge_next: &acknowledge,
        fired: &acknowledge_fired,
    });
    let metadata = upload_metadata();
    let request = begin_request(&metadata, 5);
    let storage = CuttingBank {
        inner: MemoryBank::default(),
        cut,
        header_reads: 0,
        chunk_reads: 0,
        fired: false,
    };
    let mut bank = ProfileBank::new(storage, Auth);

    assert!(catch_unwind(AssertUnwindSafe(|| {
        begin_profile_upload(&mut state, &mut bank, &request, &metadata).unwrap();
    }))
    .is_err());
    let (storage, _) = bank.into_parts();
    assert!(storage.fired || acknowledge_fired.get());
    assert!(matches!(
        reboot_relay(record.get().unwrap()),
        RelayProfileState::Pending { .. }
    ));
}

fn assert_uploading_zero(record: &Cell<Option<ProtectedAuthorityRecord>>) {
    assert!(matches!(
        reboot_relay(record.get().unwrap()),
        RelayProfileState::Uploading(upload) if upload.next_index == 0
    ));
}

fn assert_uploading_after_chunk_cut(cut: MediaCut, acknowledge_next: Option<u8>) {
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let acknowledge = Cell::new(None);
    let acknowledge_fired = Cell::new(false);
    let mut state = pending_state(CutStore {
        revision: &revision,
        record: &record,
        acknowledge_next: &acknowledge,
        fired: &acknowledge_fired,
    });
    let metadata = upload_metadata();
    let mut bank = ProfileBank::new(MemoryBank::default(), Auth);
    begin_profile_upload(
        &mut state,
        &mut bank,
        &begin_request(&metadata, 5),
        &metadata,
    )
    .unwrap();
    let (inner, _) = bank.into_parts();
    acknowledge.set(acknowledge_next);
    let mut bank = ProfileBank::new(
        CuttingBank {
            inner,
            cut,
            header_reads: 0,
            chunk_reads: 0,
            fired: false,
        },
        Auth,
    );

    assert!(catch_unwind(AssertUnwindSafe(|| {
        write_profile_chunk(&mut state, &mut bank, &chunk_request(), &metadata).unwrap();
    }))
    .is_err());
    let (storage, _) = bank.into_parts();
    assert!(storage.fired || acknowledge_fired.get());
    assert_uploading_zero(&record);
}

#[test]
fn begin_cuts_never_persist_uploading_ahead_of_bank() {
    assert_pending_after_begin_cut(MediaCut::FirstHeaderRead, None);
    assert_pending_after_begin_cut(MediaCut::HeaderReadback, None);
    assert_pending_after_begin_cut(MediaCut::None, Some(0));
}

#[test]
fn chunk_cuts_never_advance_progress_ahead_of_bank() {
    assert_uploading_after_chunk_cut(MediaCut::FirstChunkRead, None);
    assert_uploading_after_chunk_cut(MediaCut::ChunkReadback, None);
    assert_uploading_after_chunk_cut(MediaCut::None, Some(1));
}

#[derive(Clone, Copy)]
enum RetryDamage {
    AbsentHeader,
    CorruptHeader,
    CorruptCommittedPrefix,
}

fn assert_retry_damage_seals(damage: RetryDamage) {
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let acknowledge = Cell::new(None);
    let fired = Cell::new(false);
    let mut state = pending_state(CutStore {
        revision: &revision,
        record: &record,
        acknowledge_next: &acknowledge,
        fired: &fired,
    });
    let metadata = upload_metadata();
    let mut bank = ProfileBank::new(MemoryBank::default(), Auth);
    begin_profile_upload(
        &mut state,
        &mut bank,
        &begin_request(&metadata, 5),
        &metadata,
    )
    .unwrap();
    let sequence = if matches!(damage, RetryDamage::CorruptCommittedPrefix) {
        write_profile_chunk(&mut state, &mut bank, &chunk_request(), &metadata).unwrap();
        7
    } else {
        6
    };
    let (mut storage, _) = bank.into_parts();
    match damage {
        RetryDamage::AbsentHeader => storage.erase_bank(0).unwrap(),
        RetryDamage::CorruptHeader => storage.write_header(0, b"corrupt").unwrap(),
        RetryDamage::CorruptCommittedPrefix => storage.write_chunk(0, 0, b"corrupt").unwrap(),
    }
    let mut bank = ProfileBank::new(storage, Auth);

    assert_eq!(
        begin_profile_upload(
            &mut state,
            &mut bank,
            &begin_request(&metadata, sequence),
            &metadata,
        ),
        Err(UploadFlowError::Bank(BankError::Sealed))
    );
    assert_eq!(state.mode(), AuthorityMode::Sealed);
}

#[test]
fn begin_retries_require_the_existing_authenticated_bank() {
    assert_retry_damage_seals(RetryDamage::AbsentHeader);
    assert_retry_damage_seals(RetryDamage::CorruptHeader);
    assert_retry_damage_seals(RetryDamage::CorruptCommittedPrefix);
}
