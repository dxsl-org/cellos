// SPDX-License-Identifier: MPL-2.0
//! Host tests for the directory-handle spawn carrier and its attestation.
//!
//! These cover the boundary the kernel relies on: a malformed carrier must fail
//! the whole spawn, and a record that does not name its source must not parse.

#![cfg(test)]

use super::dir_attestation::{ViDirHandleAttestation, DIR_ATTESTATION_LEN};
use super::dir_handles::{
    DirHandleSet, DirHandleSetError, ViDirHandle, ViSpawnDirHandles, MAX_SPAWN_DIR_HANDLES,
    SPAWN_DIR_HANDLES_VERSION,
};

fn carrier(handles: &[u64]) -> ViSpawnDirHandles {
    let mut c = ViSpawnDirHandles::EMPTY;
    c.count = handles.len() as u32;
    for (slot, h) in c.handles.iter_mut().zip(handles) {
        *slot = *h;
    }
    c
}

#[test]
fn empty_carrier_yields_empty_set() {
    let set = DirHandleSet::from_carrier(&ViSpawnDirHandles::EMPTY).unwrap();
    assert!(set.is_empty());
    assert_eq!(set.as_slice(), &[] as &[u64]);
}

#[test]
fn handles_round_trip_in_order() {
    let set = DirHandleSet::from_carrier(&carrier(&[7, 3, 9])).unwrap();
    assert_eq!(set.as_slice(), &[7, 3, 9]);
    assert_eq!(set.len(), 3);
}

#[test]
fn a_full_set_is_accepted() {
    let full: [u64; MAX_SPAWN_DIR_HANDLES] = core::array::from_fn(|i| i as u64 + 1);
    assert_eq!(
        DirHandleSet::from_carrier(&carrier(&full)).unwrap().len(),
        MAX_SPAWN_DIR_HANDLES
    );
}

#[test]
fn an_over_long_count_fails_rather_than_truncating() {
    let mut c = carrier(&[1, 2]);
    c.count = MAX_SPAWN_DIR_HANDLES as u32 + 1;
    assert_eq!(
        DirHandleSet::from_carrier(&c),
        Err(DirHandleSetError::TooMany)
    );
}

#[test]
fn zero_is_not_a_handle() {
    assert_eq!(
        DirHandleSet::from_carrier(&carrier(&[4, 0])),
        Err(DirHandleSetError::ZeroHandle)
    );
}

#[test]
fn a_repeated_handle_fails_the_whole_set() {
    assert_eq!(
        DirHandleSet::from_carrier(&carrier(&[4, 5, 4])),
        Err(DirHandleSetError::Duplicate)
    );
}

#[test]
fn an_unknown_version_is_never_interpreted() {
    let mut c = carrier(&[1]);
    c.version = SPAWN_DIR_HANDLES_VERSION + 1;
    assert_eq!(
        DirHandleSet::from_carrier(&c),
        Err(DirHandleSetError::UnsupportedVersion)
    );
}

#[test]
fn entries_past_count_are_ignored_not_smuggled() {
    let mut c = carrier(&[1, 2, 3]);
    c.count = 1;
    assert_eq!(DirHandleSet::from_carrier(&c).unwrap().as_slice(), &[1]);
}

#[test]
fn builder_refuses_more_than_the_bound() {
    let too_many: [ViDirHandle; MAX_SPAWN_DIR_HANDLES + 1] =
        core::array::from_fn(|i| ViDirHandle(i as u64 + 1));
    assert_eq!(
        ViSpawnDirHandles::new(&too_many).unwrap_err(),
        DirHandleSetError::TooMany
    );
    let ok: [ViDirHandle; 2] = [ViDirHandle(11), ViDirHandle(12)];
    let built = ViSpawnDirHandles::new(&ok).unwrap();
    assert_eq!(
        DirHandleSet::from_carrier(&built).unwrap().as_slice(),
        &[11, 12]
    );
}

fn attestation(set: &[u64], spawner: u64) -> ViDirHandleAttestation {
    ViDirHandleAttestation {
        cell_id: 42,
        generation: 3,
        spawner_cell_id: spawner,
        spawner_generation: 2,
        set: DirHandleSet::from_carrier(&carrier(set)).unwrap(),
    }
}

#[test]
fn attestation_round_trips() {
    let a = attestation(&[8, 9], 7);
    assert_eq!(ViDirHandleAttestation::from_bytes(&a.to_bytes()), Some(a));
}

#[test]
fn empty_attestation_round_trips_without_a_spawner() {
    let a = attestation(&[], 0);
    assert_eq!(ViDirHandleAttestation::from_bytes(&a.to_bytes()), Some(a));
}

#[test]
fn a_non_empty_set_must_name_its_source() {
    let mut bytes = attestation(&[8], 7).to_bytes();
    bytes[24..32].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(ViDirHandleAttestation::from_bytes(&bytes), None);
}

#[test]
fn a_stale_buffer_is_not_an_attestation() {
    assert_eq!(
        ViDirHandleAttestation::from_bytes(&[0xABu8; DIR_ATTESTATION_LEN]),
        None
    );
    assert_eq!(
        ViDirHandleAttestation::from_bytes(&[0u8; DIR_ATTESTATION_LEN]),
        None
    );
}

#[test]
fn a_short_buffer_yields_nothing() {
    let a = attestation(&[8], 7);
    assert_eq!(
        ViDirHandleAttestation::from_bytes(&a.to_bytes()[..DIR_ATTESTATION_LEN - 1]),
        None
    );
}

#[test]
fn cell_zero_is_rejected() {
    let mut bytes = attestation(&[8], 7).to_bytes();
    bytes[8..16].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(ViDirHandleAttestation::from_bytes(&bytes), None);
}

#[test]
fn a_wrong_version_record_is_not_reinterpreted() {
    let mut bytes = attestation(&[8], 7).to_bytes();
    bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
    assert_eq!(ViDirHandleAttestation::from_bytes(&bytes), None);
}

#[test]
fn an_over_long_encoded_count_is_rejected() {
    let mut bytes = attestation(&[8], 7).to_bytes();
    bytes[40..44].copy_from_slice(&(MAX_SPAWN_DIR_HANDLES as u32 + 1).to_le_bytes());
    assert_eq!(ViDirHandleAttestation::from_bytes(&bytes), None);
}
