// SPDX-License-Identifier: MPL-2.0
//! The kernel's statement about where a cell's directory-handle set came from.
//!
//! [`super::caller_identity`] answers "which cell is calling me?" in a fixed
//! 32-byte trailer written past the payload of a recv buffer. A handle set is
//! variable-length and would not fit there, and padding it into the trailer
//! would mean either a hard truncation or reserving the tail of every recv
//! buffer for a field almost no message needs.
//!
//! So this record is not pushed; it is pulled. The filesystem service, having
//! learned a caller's cell id from the attested trailer, asks the kernel for
//! that cell's record and the kernel writes this structure into the service's
//! own buffer during the service's own syscall. Its trust basis is identical to
//! the trailer's — produced by the kernel from live scheduler state, never
//! relayed through a message any cell composed — while the size question
//! disappears, because the service supplies a buffer it sized itself and an
//! undersized one is an error rather than a silent truncation.
//!
//! ## Invariants
//! - A record with `count > 0` always names a spawner. "These handles came from
//!   somewhere" is not a claim a service can check.
//! - `cell_id` of `0` is never a cell (the kernel uses it for its own
//!   allocations) and is rejected at parse time.
//! - The kernel states only provenance. Whether the spawner genuinely held these
//!   handles is a question about the filesystem service's own table, and only it
//!   can answer it.

use super::dir_handles::{DirHandleSet, MAX_SPAWN_DIR_HANDLES};

/// Encoded size of a [`ViDirHandleAttestation`].
pub const DIR_ATTESTATION_LEN: usize = 48 + 8 * MAX_SPAWN_DIR_HANDLES;

/// Layout version of the encoded record.
pub const DIR_ATTESTATION_VERSION: u32 = 1;

/// Tag distinguishing a kernel-written record from whatever the buffer held.
///
/// Not a security control — in a single address space a cell that can write
/// another's buffer can write anything. It exists so a service can tell "the
/// kernel filled this in" from "my buffer still holds the last reply".
const MAGIC: u32 = 0x5649_4448; // "VIDH"

/// What the kernel says about a cell's inherited directory handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViDirHandleAttestation {
    /// The cell the record describes.
    pub cell_id: u64,
    /// That cell's generation, so a service cannot bind a dead cell's set to the
    /// respawned cell that took its id.
    pub generation: u64,
    /// The cell that named the set at spawn. `0` only when the set is empty.
    pub spawner_cell_id: u64,
    /// The spawner's generation at the moment of the spawn.
    pub spawner_generation: u64,
    /// The handles the spawner named. Unvalidated authority: the kernel asserts
    /// only that this spawner named them.
    pub set: DirHandleSet,
}

impl ViDirHandleAttestation {
    /// Encode into the fixed-size record the kernel writes.
    pub fn to_bytes(&self) -> [u8; DIR_ATTESTATION_LEN] {
        let mut out = [0u8; DIR_ATTESTATION_LEN];
        out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        out[4..8].copy_from_slice(&DIR_ATTESTATION_VERSION.to_le_bytes());
        out[8..16].copy_from_slice(&self.cell_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.generation.to_le_bytes());
        out[24..32].copy_from_slice(&self.spawner_cell_id.to_le_bytes());
        out[32..40].copy_from_slice(&self.spawner_generation.to_le_bytes());
        out[40..44].copy_from_slice(&(self.set.len() as u32).to_le_bytes());
        // [44..48] reserved: zero today so a flag word can be added later
        // without moving the handle array.
        for (i, h) in self.set.as_slice().iter().enumerate() {
            let at = 48 + i * 8;
            out[at..at + 8].copy_from_slice(&h.to_le_bytes());
        }
        out
    }

    /// Parse a record the kernel wrote into a service-owned buffer.
    ///
    /// Returns `None` for a short buffer, an untagged or wrong-version record, a
    /// `cell_id` of `0`, a count past [`MAX_SPAWN_DIR_HANDLES`], a non-empty set
    /// with no spawner, or any handle the set rebuilds as invalid. Every one of
    /// those means "no attested set", and the only correct response to that is to
    /// bind nothing.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < DIR_ATTESTATION_LEN {
            return None;
        }
        let word32 = |at: usize| -> u32 {
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[at..at + 4]);
            u32::from_le_bytes(b)
        };
        let word64 = |at: usize| -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[at..at + 8]);
            u64::from_le_bytes(b)
        };
        if word32(0) != MAGIC || word32(4) != DIR_ATTESTATION_VERSION {
            return None;
        }
        let cell_id = word64(8);
        if cell_id == 0 {
            return None;
        }
        let count = word32(40) as usize;
        if count > MAX_SPAWN_DIR_HANDLES {
            return None;
        }
        let spawner_cell_id = word64(24);
        if count > 0 && spawner_cell_id == 0 {
            return None; // a set nobody is named as the source of
        }
        let mut carrier = super::dir_handles::ViSpawnDirHandles::EMPTY;
        carrier.count = count as u32;
        for i in 0..count {
            carrier.handles[i] = word64(48 + i * 8);
        }
        Some(Self {
            cell_id,
            generation: word64(16),
            spawner_cell_id,
            spawner_generation: word64(32),
            set: DirHandleSet::from_carrier(&carrier).ok()?,
        })
    }
}
