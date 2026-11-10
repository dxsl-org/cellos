//! `XE_GPU` IPC transport (service id 21, CP1 D-G8): one DRM file operation per request,
//! the `xe` ioctl carried unchanged.
//!
//! The transport adds no GPU semantics (ADR-0033 G1, "no new GPU API"): the request code
//! and the struct bytes are Linux's. The one thing IPC cannot do in place is touch the
//! client's memory through the user pointers inside a struct, so the client declares the
//! user ranges a request may touch:
//! - an `IN` segment carries its bytes to the driver, which may read only inside it;
//! - an `OUT` segment is a range the driver may write; the reply returns every write and the
//!   client applies it only inside a range it declared.
//!
//! Anything outside the declared segments fails with `EFAULT`, as a bad user pointer does.
//!
//! Request (little-endian): `op u8, segment_count u8, arg_len u16, file u32, request u32,
//! reserved u32 (0)`, then `segment_count` × `{addr u64, len u32, flags u32}`, the `arg`
//! bytes, and the bytes of each `IN` segment in order; the counts give the frame length.
//! Reply: `errno u32 (0 = success), value u32, arg_len u16, write_count u8, reserved u8 (0),
//! reserved u32 (0)`, then `write_count` × `{addr u64, len u32, reserved u32 (0)}`, the `arg`
//! bytes and the bytes of each write in order. The layout is recorded in
//! `docs/abi-boundary-records.md`.

mod reply;
mod request;

pub use reply::{decode_reply, encode_reply, Reply};
pub use request::{decode_request, encode_request, Request, SegmentMemory};

/// Fixed header of a request and of a reply.
pub const HEADER_LEN: usize = 16;
/// One segment (request) or write (reply) descriptor.
pub const DESCRIPTOR_LEN: usize = 16;
/// Segments one request may declare. `VERSION` needs three; no served ioctl needs more.
pub const MAX_SEGMENTS: usize = 4;
/// Writes one reply may carry.
pub const MAX_WRITES: usize = 8;
/// Largest request: an `IPC_BUF_SIZE` (4096) receive buffer minus the 32-byte
/// caller-identity trailer the kernel writes at its end.
pub const MAX_REQUEST_LEN: usize = 4096 - 32;
/// Largest reply: one `IPC_BUF_SIZE` IPC payload.
pub const MAX_REPLY_LEN: usize = 4096;

/// Open a DRM file; the reply `value` is its file id.
pub const OP_OPEN: u8 = 1;
/// Close the DRM file named by `file` (`postclose`).
pub const OP_CLOSE: u8 = 2;
/// Run `request` on the DRM file named by `file`.
pub const OP_IOCTL: u8 = 3;

/// The driver may read the segment; its bytes follow the `arg`.
pub const SEG_IN: u32 = 1 << 0;
/// The driver may write the segment.
pub const SEG_OUT: u32 = 1 << 1;

/// A user range a request may touch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub addr: u64,
    pub len: u32,
    pub flags: u32,
}

impl Segment {
    fn covers(&self, addr: u64, len: usize, flag: u32) -> bool {
        let Some(offset) = addr.checked_sub(self.addr) else {
            return false;
        };
        self.flags & flag != 0
            && offset
                .checked_add(len as u64)
                .is_some_and(|end| end <= u64::from(self.len))
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
