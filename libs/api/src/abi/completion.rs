// SPDX-License-Identifier: MPL-2.0
//! The record a finished asynchronous operation lands in.
//!
//! A completion names a *submission*, not a task: the slot is handed out when
//! the operation is submitted and is meaningful only inside the cell that
//! reserved it. A task id would say nothing about which of that task's
//! operations finished, and is reused after a restart.
//!
//! The kernel writes this record into a buffer the caller supplies during the
//! caller's own syscall, the same shape [`super::dir_attestation`] uses: the
//! caller sizes the buffer, and one that is too small is an error rather than a
//! silent truncation.
//!
//! ## Invariants
//! - A record is written only when the syscall reports that one was written. A
//!   caller that ignores the return value and reads the buffer anyway may be
//!   reading whatever it last held; [`COMPLETION_MAGIC`] exists so that mistake
//!   is detectable, not so it is safe.
//! - `result` is signed. Non-negative values are operation-defined success
//!   values; negative values are errors, and every error a caller must
//!   distinguish is a named constant here.

/// Encoded size of a [`ViCompletion`].
pub const COMPLETION_LEN: usize = 24;

/// Layout version of the encoded record.
pub const COMPLETION_VERSION: u32 = 1;

/// Tag distinguishing a kernel-written record from whatever the buffer held.
///
/// Not a security control — in a single address space a cell that can write
/// another's buffer can write anything. It exists so a caller can tell "the
/// kernel filled this in" from "my buffer still holds the last result".
pub const COMPLETION_MAGIC: u32 = 0x5649_4351; // "VICQ"

/// The waiter's reservation was released without its event ever firing.
///
/// Reported to a waiter whose outstanding reservation was displaced by a later
/// waiter for the same event source. It is an ordinary result rather than a
/// hang: a submission that has been given a landing place must always land in
/// it, or the waiter never runs again.
pub const RESULT_ABANDONED: i64 = -1;

/// A finished operation, as the kernel hands it to the cell that submitted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViCompletion {
    /// The submission this result belongs to, as issued at submission time.
    pub slot: u32,
    /// Operation result. Negative values are errors; see [`RESULT_ABANDONED`].
    pub result: i64,
}

impl ViCompletion {
    /// Encode into the fixed-size record the kernel writes.
    pub fn to_bytes(&self) -> [u8; COMPLETION_LEN] {
        let mut out = [0u8; COMPLETION_LEN];
        out[0..4].copy_from_slice(&COMPLETION_MAGIC.to_le_bytes());
        out[4..8].copy_from_slice(&COMPLETION_VERSION.to_le_bytes());
        out[8..12].copy_from_slice(&self.slot.to_le_bytes());
        // [12..16] reserved: zero today so a source or flag word can be added
        // later without moving `result`.
        out[16..24].copy_from_slice(&self.result.to_le_bytes());
        out
    }

    /// Parse a record the kernel wrote into a caller-owned buffer.
    ///
    /// # Errors
    /// `None` for a short buffer, an untagged record, or a version this build
    /// does not know. All three mean "no completion here", and the only correct
    /// response is to treat the wait as having produced nothing.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < COMPLETION_LEN {
            return None;
        }
        let word32 = |at: usize| -> u32 {
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[at..at + 4]);
            u32::from_le_bytes(b)
        };
        if word32(0) != COMPLETION_MAGIC || word32(4) != COMPLETION_VERSION {
            return None;
        }
        let mut result = [0u8; 8];
        result.copy_from_slice(&bytes[16..24]);
        Some(Self {
            slot: word32(8),
            result: i64::from_le_bytes(result),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let original = ViCompletion {
            slot: 31,
            result: 0x0123_4567_89ab_cdef,
        };
        let decoded = ViCompletion::from_bytes(&original.to_bytes()).expect("valid record");
        assert_eq!(decoded, original);
    }

    #[test]
    fn round_trips_a_negative_result() {
        // The error half of the encoding is the half a sign-losing cast breaks.
        let original = ViCompletion {
            slot: 0,
            result: RESULT_ABANDONED,
        };
        let decoded = ViCompletion::from_bytes(&original.to_bytes()).expect("valid record");
        assert_eq!(decoded.result, RESULT_ABANDONED);
        assert!(decoded.result < 0);
    }

    #[test]
    fn rejects_a_buffer_the_kernel_never_wrote() {
        // A caller that reads its buffer after a wait that produced nothing must
        // not decode stale bytes as a completion.
        assert_eq!(ViCompletion::from_bytes(&[0u8; COMPLETION_LEN]), None);
        assert_eq!(ViCompletion::from_bytes(&[0xAAu8; COMPLETION_LEN]), None);
    }

    #[test]
    fn rejects_a_short_buffer_instead_of_reading_past_it() {
        let full = ViCompletion { slot: 1, result: 2 }.to_bytes();
        assert_eq!(ViCompletion::from_bytes(&full[..COMPLETION_LEN - 1]), None);
        assert_eq!(ViCompletion::from_bytes(&[]), None);
    }

    #[test]
    fn rejects_an_unknown_version() {
        let mut bytes = ViCompletion { slot: 1, result: 2 }.to_bytes();
        bytes[4] = COMPLETION_VERSION as u8 + 1;
        assert_eq!(ViCompletion::from_bytes(&bytes), None);
    }

    #[test]
    fn encoded_length_is_fixed_and_arch_independent() {
        // The kernel writes exactly this many bytes into a caller buffer; a
        // caller sized from `size_of::<ViCompletion>()` would be wrong.
        assert_eq!(ViCompletion { slot: 0, result: 0 }.to_bytes().len(), 24);
        assert_eq!(COMPLETION_LEN, 24);
    }
}
