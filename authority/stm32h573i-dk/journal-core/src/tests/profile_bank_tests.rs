use super::profile_bank_support::*;
use crate::*;
use std::vec;

#[test]
fn cut_prefixes_are_reclaimable_only_at_current_region() {
    let bytes = vec![0x5a; 100];
    let (mut bank, metadata) = initialized(&bytes);
    assert_eq!(bank.write_chunk(&metadata, 0, 0, &bytes), Ok(1));
    let (storage, _) = bank.into_parts();
    let complete_region = storage.chunks[1][0].clone();

    for cut in 0..complete_region.len() {
        let mut cut_storage = storage.clone();
        cut_storage.erased_chunks.clear();
        cut_storage.chunks[1][0].truncate(cut);
        let mut current = ProfileBank::new(cut_storage, BankAuth);
        assert_eq!(
            current.recover_upload(&metadata, 0),
            Ok(UploadHead::AwaitingWrite)
        );
        assert_eq!(current.write_chunk(&metadata, 0, 0, &bytes), Ok(1));
        let (found, _) = current.into_parts();
        assert_eq!(found.erased_chunks, [(1, 0)]);

        let mut committed_storage = storage.clone();
        committed_storage.chunks[1][0].truncate(cut);
        let mut committed = ProfileBank::new(committed_storage, BankAuth);
        assert_eq!(
            committed.recover_upload(&metadata, 1),
            Err(BankError::Sealed)
        );
        assert!(committed.into_parts().0.sealed);
    }
}

#[test]
fn initialization_cut_prefixes_rebuild_the_inactive_bank() {
    let bytes = vec![0x21; 10];
    let (bank, metadata) = initialized(&bytes);
    let header = bank.into_parts().0.headers[1].clone();
    for cut in 0..header.len() {
        let mut storage = BankStorage::empty();
        storage.headers[1] = header[..cut].to_vec();
        storage.chunks[1][0] = vec![0xaa];
        let mut recovering = ProfileBank::new(storage, BankAuth);
        assert_eq!(recovering.initialize(&metadata), Ok(()));
        let (found, _) = recovering.into_parts();
        assert_eq!(found.headers[1], header);
        assert!(found.chunks[1][0].is_empty());
    }
}

#[test]
fn exact_retry_is_idempotent_and_conflict_seals() {
    let bytes = vec![0x33; 40];
    let (mut bank, metadata) = initialized(&bytes);
    assert_eq!(bank.write_chunk(&metadata, 0, 0, &bytes), Ok(1));
    assert_eq!(bank.initialize(&metadata), Ok(()));
    assert_eq!(
        bank.recover_upload(&metadata, 0),
        Ok(UploadHead::ExactRetryCandidate)
    );
    assert_eq!(bank.write_chunk(&metadata, 1, 0, &bytes), Ok(1));

    let mut conflict = bytes.clone();
    conflict[0] ^= 1;
    assert_eq!(
        bank.write_chunk(&metadata, 1, 0, &conflict),
        Err(BankError::Sealed)
    );
    assert!(bank.into_parts().0.sealed);
}

#[test]
fn authenticated_future_chunk_and_metadata_replay_seal() {
    let bytes = vec![0x44; PROFILE_CHUNK_SIZE + 2];
    let (mut bank, metadata) = initialized(&bytes);
    let next = bank
        .write_chunk(&metadata, 0, 0, &bytes[..PROFILE_CHUNK_SIZE])
        .unwrap();
    bank.write_chunk(&metadata, next, 1, &bytes[PROFILE_CHUNK_SIZE..])
        .unwrap();
    let (storage, _) = bank.into_parts();
    let mut future = ProfileBank::new(storage.clone(), BankAuth);
    assert_eq!(future.recover_upload(&metadata, 0), Err(BankError::Sealed));

    let replay = storage.chunks[1][0].clone();
    let mut changed = metadata.clone();
    changed.authority_epoch += 1;
    let mut fresh = ProfileBank::new(storage, BankAuth);
    fresh.initialize(&changed).unwrap();
    let (mut fresh_storage, _) = fresh.into_parts();
    fresh_storage.chunks[1][0] = replay;
    let mut replayed = ProfileBank::new(fresh_storage, BankAuth);
    assert_eq!(replayed.recover_upload(&changed, 0), Err(BankError::Sealed));
}

#[test]
fn chunk_lengths_and_indices_are_exact() {
    let bytes = vec![0x6c; PROFILE_CHUNK_SIZE + 2];
    let (mut bank, metadata) = initialized(&bytes);
    assert_eq!(
        bank.write_chunk(&metadata, 0, 0, &bytes[..PROFILE_CHUNK_SIZE - 1]),
        Err(BankError::InvalidChunk)
    );
    assert_eq!(
        bank.write_chunk(&metadata, 0, 1, &bytes[PROFILE_CHUNK_SIZE..]),
        Err(BankError::InvalidSequence)
    );
    let next = bank
        .write_chunk(&metadata, 0, 0, &bytes[..PROFILE_CHUNK_SIZE])
        .unwrap();
    assert_eq!(
        bank.write_chunk(
            &metadata,
            next,
            1,
            &bytes[PROFILE_CHUNK_SIZE..PROFILE_CHUNK_SIZE + 1]
        ),
        Err(BankError::InvalidChunk)
    );
    assert_eq!(
        bank.write_chunk(&metadata, next, 1, &bytes[PROFILE_CHUNK_SIZE..]),
        Ok(2)
    );
}

#[test]
fn exact_chunk_boundaries_complete_with_streaming_digest() {
    let bytes = vec![0xa5; authority_protocol::PROFILE_MAX_LEN];
    let (mut bank, metadata) = initialized(&bytes);
    let mut next = 0;
    for (index, chunk) in bytes.chunks(PROFILE_CHUNK_SIZE).enumerate() {
        next = bank
            .write_chunk(&metadata, next, index as u8, chunk)
            .unwrap();
    }
    assert_eq!(next as usize, PROFILE_CHUNK_REGIONS);
    let reference = bank.complete(&metadata, next).unwrap();
    assert_eq!(reference.profile_len as usize, bytes.len());
    assert_eq!(bank.validate_reference(&reference), Ok(()));

    let mut oversized = metadata;
    oversized.profile_len += 1;
    assert_eq!(bank.initialize(&oversized), Err(BankError::InvalidMetadata));
}

#[test]
fn referenced_missing_or_mismatched_bank_seals() {
    let bytes = vec![0x17; PROFILE_CHUNK_SIZE + 1];
    let (mut bank, metadata) = initialized(&bytes);
    let mut next = 0;
    for (index, chunk) in bytes.chunks(PROFILE_CHUNK_SIZE).enumerate() {
        next = bank
            .write_chunk(&metadata, next, index as u8, chunk)
            .unwrap();
    }
    let reference = bank.complete(&metadata, next).unwrap();
    let (storage, _) = bank.into_parts();

    let mut valid = ProfileBank::new(storage.clone(), BankAuth);
    assert_eq!(valid.validate_reference(&reference), Ok(()));

    let mut mismatched = reference.clone();
    mismatched.authority_epoch += 1;
    let mut replay = ProfileBank::new(storage, BankAuth);
    assert_eq!(
        replay.validate_reference(&mismatched),
        Err(BankError::Sealed)
    );

    let mut absent = ProfileBank::new(BankStorage::empty(), BankAuth);
    assert_eq!(
        absent.validate_reference(&reference),
        Err(BankError::Sealed)
    );
}
