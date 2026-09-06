// SPDX-License-Identifier: MPL-2.0
//! Kernel-attested caller identity for service IPC.
//!
//! A service cannot derive who called it. `sys_recv` returns a **tid**, and a tid
//! is not a cell: a thread carries its own tid while inheriting its parent cell's
//! `CellId`, so `CellId(tid)` misattributes every thread. Anything the request
//! payload claims about the caller is chosen by the caller and therefore worth
//! nothing to an authorization decision.
//!
//! So the kernel states it. A receiver opts in by passing
//! [`RECV_ATTEST_CALLER`] as the fourth argument of `ViSyscall::Recv`; the kernel
//! then writes a [`CallerIdentity`] trailer into the **last
//! [`CALLER_IDENTITY_LEN`] bytes** of that receiver's own recv buffer, after the
//! message payload has been copied.
//!
//! ## Why the tail of the buffer, and not a new message field
//! Postcard byte 0 is the request discriminant; a trailer at a fixed offset
//! past the payload is invisible to existing parsers on both kernel and cell sides.
//! The trailer reports sender diagnostics; VFS obtains root lifetime endpoints
//! separately via `ResolveCellOwner` or `WatchCellOwner`.
//! ## Invariants
//! - Opting in **reserves** the tail: a receiver that passes the flag must treat
//!   `buf[buf_len - CALLER_IDENTITY_LEN ..]` as kernel-owned and must not expect
//!   payload bytes there. Cell recv buffers are `IPC_BUF_SIZE` (4096) and the
//!   largest message in the system is under 1 KiB, so the reserved tail costs no
//!   usable payload today.
//! - The kernel writes the trailer **after** copying the payload, so a sender
//!   that pads its message out to the full buffer cannot pre-place a forged
//!   trailer: its bytes there are overwritten.
//! - Absent or garbage tail → [`CallerIdentity::from_trailer`] returns
//!   `None`, which the receiver MUST treat as "unknown caller" and deny. There is
//!   no permissive interpretation of a missing trailer.
//! - A `cell_id` of 0 is never a cell (the kernel uses it for its own
//!   allocations), so it is rejected at parse time rather than deep in a policy
//!   table.

/// Fourth `ViSyscall::Recv` argument requesting a caller-identity trailer.
///
/// `0` (what every pre-existing caller passes) means "no trailer" — the kernel
/// then leaves the receiver's buffer exactly as before, so opting in is the only
/// way to change any receiver's observable behaviour.
pub const RECV_ATTEST_CALLER: usize = 1;

/// Caller identity flag indicating the calling Cell has declared VfsMutate authority.
pub const CALLER_FLAG_VFS_MUTATE: u32 = 1 << 0;

/// Size in bytes of the identity trailer reserved at the end of a recv buffer.
pub const CALLER_IDENTITY_LEN: usize = 32;
/// Tag distinguishing a kernel-written trailer from leftover payload bytes.
///
/// Not a security control — a cell that can write another cell's recv buffer in a
/// single address space can write anything, including the service's policy table.
/// It exists so a receiver that opted in can tell "kernel wrote identity here"
/// from "previous, longer message left bytes here".
const MAGIC: u32 = 0x5649_4349; // "VICI"

/// Who the kernel says called.
///
/// Construct only from a kernel-written trailer ([`CallerIdentity::from_trailer`])
/// or, inside the kernel, from live scheduler state — never from request bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallerIdentity {
    /// Flag word unpacked from trailer bytes 4..8 (e.g. [`CALLER_FLAG_VFS_MUTATE`]).
    pub flags: u32,
    /// Owning cell of the calling task. A thread reports its parent cell here.
    pub cell_id: u64,
    /// Monotonic epoch assigned when the cell was created, inherited by its threads.
    pub generation: u64,
    /// Task id that actually sent the message. Diagnostics only.
    pub sender_tid: u64,
}

impl CallerIdentity {
    /// Serialize into the fixed-size trailer the kernel writes.
    pub fn to_trailer(&self) -> [u8; CALLER_IDENTITY_LEN] {
        let mut out = [0u8; CALLER_IDENTITY_LEN];
        out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        out[4..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..16].copy_from_slice(&self.cell_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.generation.to_le_bytes());
        out[24..32].copy_from_slice(&self.sender_tid.to_le_bytes());
        out
    }

    /// Parse the trailer out of a recv buffer that opted in with
    /// [`RECV_ATTEST_CALLER`].
    ///
    /// Returns `None` when the buffer is too small to hold a trailer, when the
    /// tail is not a kernel-written trailer, or when the attested `cell_id` is 0.
    /// Every one of those is "identity unknown", and the only correct response to
    /// an unknown identity is to deny the request.
    pub fn from_recv_buf(buf: &[u8]) -> Option<Self> {
        let start = buf.len().checked_sub(CALLER_IDENTITY_LEN)?;
        Self::from_trailer(&buf[start..])
    }

    /// Parse a trailer from exactly the trailer bytes.
    ///
    /// Split out from [`Self::from_recv_buf`] so the kernel and host tests can
    /// round-trip the encoding without constructing a whole recv buffer.
    pub fn from_trailer(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < CALLER_IDENTITY_LEN {
            return None;
        }
        let word = |at: usize| -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[at..at + 8]);
            u64::from_le_bytes(b)
        };
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&bytes[0..4]);
        if u32::from_le_bytes(tag) != MAGIC {
            return None;
        }
        let mut flag_bytes = [0u8; 4];
        flag_bytes.copy_from_slice(&bytes[4..8]);
        let flags = u32::from_le_bytes(flag_bytes);
        let cell_id = word(8);
        if cell_id == 0 {
            return None; // kernel/unattributable — never a cell
        }
        Some(Self {
            flags,
            cell_id,
            generation: word(16),
            sender_tid: word(24),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: CallerIdentity = CallerIdentity {
        flags: 0,
        cell_id: 7,
        generation: 42,
        sender_tid: 9,
    };

    #[test]
    fn trailer_round_trips() {
        assert_eq!(
            CallerIdentity::from_trailer(&SAMPLE.to_trailer()),
            Some(SAMPLE)
        );
    }
    #[test]
    fn flags_round_trip() {
        let mut sample = SAMPLE;
        sample.flags = CALLER_FLAG_VFS_MUTATE;
        assert_eq!(
            CallerIdentity::from_trailer(&sample.to_trailer()),
            Some(sample)
        );
    }

    #[test]
    fn untagged_tail_is_not_an_identity() {
        // A recv buffer whose tail still holds bytes of a previous message.
        assert_eq!(
            CallerIdentity::from_trailer(&[0xABu8; CALLER_IDENTITY_LEN]),
            None
        );
        assert_eq!(
            CallerIdentity::from_trailer(&[0u8; CALLER_IDENTITY_LEN]),
            None
        );
    }

    #[test]
    fn cell_zero_is_rejected() {
        let mut trailer = SAMPLE.to_trailer();
        trailer[8..16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(CallerIdentity::from_trailer(&trailer), None);
    }

    #[test]
    fn short_buffer_yields_no_identity() {
        assert_eq!(CallerIdentity::from_trailer(&[0u8; 8]), None);
        assert_eq!(CallerIdentity::from_recv_buf(&[0u8; 4]), None);
    }

    #[test]
    fn identity_is_read_from_the_tail_not_the_head() {
        let mut buf = [0u8; 512];
        let start = buf.len() - CALLER_IDENTITY_LEN;
        buf[start..].copy_from_slice(&SAMPLE.to_trailer());
        // A payload that happens to contain the tag at the head must not be
        // mistaken for the trailer.
        buf[0..CALLER_IDENTITY_LEN].copy_from_slice(&SAMPLE.to_trailer());
        assert_eq!(CallerIdentity::from_recv_buf(&buf), Some(SAMPLE));
    }
}
