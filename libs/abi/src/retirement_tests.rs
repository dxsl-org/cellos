// SPDX-License-Identifier: Apache-2.0
//! Host tests for the `ViRetirementV1` record.

use super::*;

fn sample() -> ViRetirementV1 {
    ViRetirementV1 {
        version: RETIREMENT_RECORD_VERSION,
        state: RetirementState::Quarantined as u16,
        reasons: reason::WRITABLE_BORROW | reason::TRACKED_QUARANTINE,
        member_threads: 3,
        reserved0: 0,
        cell_id: 0x1122_3344_5566_7788,
        generation: 9,
        root_tid: 42,
        remote_hart_mask: 0b1010,
        grants_reclaimed: 1,
        grantee_refs_cleared: 2,
        writable_borrows: 3,
        pins_quarantined: 4,
        vfs_leases_pending: 5,
        shm_frames_released: 0,
        dma_deferred: 0,
        reserved1: 0,
        quarantined_pages: 77,
        leaked_pages: 0,
    }
}

#[test]
fn round_trip_preserves_every_field() {
    let record = sample();
    let bytes = record.encode();
    assert_eq!(bytes.len(), RETIREMENT_RECORD_LEN);
    assert_eq!(ViRetirementV1::decode(&bytes), Ok(record));
    assert_eq!(
        record.retirement_state(),
        Some(RetirementState::Quarantined)
    );
}

#[test]
fn encoding_is_little_endian_at_declared_offsets() {
    let bytes = sample().encode();
    assert_eq!(&bytes[0..2], &1u16.to_le_bytes());
    assert_eq!(&bytes[2..4], &3u16.to_le_bytes());
    assert_eq!(&bytes[16..24], &0x1122_3344_5566_7788u64.to_le_bytes());
    assert_eq!(&bytes[32..40], &42u64.to_le_bytes());
    assert_eq!(&bytes[64..68], &5u32.to_le_bytes());
    assert_eq!(&bytes[80..88], &77u64.to_le_bytes());
}

#[test]
fn length_other_than_exact_is_refused() {
    let bytes = sample().encode();
    assert_eq!(
        ViRetirementV1::decode(&bytes[..95]),
        Err(RecordError::Length)
    );
    let mut long = [0u8; RETIREMENT_RECORD_LEN + 1];
    long[..RETIREMENT_RECORD_LEN].copy_from_slice(&bytes);
    assert_eq!(ViRetirementV1::decode(&long), Err(RecordError::Length));
    assert_eq!(ViRetirementV1::decode(&[]), Err(RecordError::Length));
}

#[test]
fn unknown_version_is_refused() {
    for version in [0u16, 2, u16::MAX] {
        let record = ViRetirementV1 {
            version,
            ..sample()
        };
        assert_eq!(
            ViRetirementV1::decode(&record.encode()),
            Err(RecordError::UnsupportedVersion)
        );
    }
}

#[test]
fn nonzero_reserved_is_refused() {
    let r0 = ViRetirementV1 {
        reserved0: 1,
        ..sample()
    };
    assert_eq!(
        ViRetirementV1::decode(&r0.encode()),
        Err(RecordError::Reserved)
    );
    let r1 = ViRetirementV1 {
        reserved1: 1 << 31,
        ..sample()
    };
    assert_eq!(
        ViRetirementV1::decode(&r1.encode()),
        Err(RecordError::Reserved)
    );
}

#[test]
fn undefined_state_is_refused() {
    for state in [0u16, 5, u16::MAX] {
        let record = ViRetirementV1 { state, ..sample() };
        assert_eq!(record.retirement_state(), None);
        assert_eq!(
            ViRetirementV1::decode(&record.encode()),
            Err(RecordError::UnknownState)
        );
    }
}

#[test]
fn undefined_reason_bit_is_refused() {
    for bit in [1u32 << 5, 1 << 31] {
        let record = ViRetirementV1 {
            reasons: reason::KNOWN | bit,
            ..sample()
        };
        assert_eq!(
            ViRetirementV1::decode(&record.encode()),
            Err(RecordError::UnknownReason)
        );
    }
    let all = ViRetirementV1 {
        reasons: reason::KNOWN,
        ..sample()
    };
    assert!(ViRetirementV1::decode(&all.encode()).is_ok());
}

/// The exit reasons init's policy keys on are ABI: pin their bytes so a drift
/// in either is a deliberate change, never an accident.
#[test]
fn exit_reason_sentinels_are_byte_pinned() {
    assert_eq!(HOTSWAP_RETIRED.to_le_bytes(), [0xAA, 0xAA, 0xAA, 0xAA]);
    assert_eq!(
        (INIT_BOOT_STOP as u64).to_le_bytes(),
        [0x09, 0x57, 0x07, 0xB0, 0, 0, 0, 0]
    );
    assert_ne!(HOTSWAP_RETIRED as usize, INIT_BOOT_STOP);
}
