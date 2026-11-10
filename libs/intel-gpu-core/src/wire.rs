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
//! - an `EXPORT` segment is not memory at all: it attaches one client-memory export lease
//!   (`ExportDmaRange`, syscall 437) to the request, the way an fd rides beside an ioctl.
//!   Its descriptor is `{addr = export id, len = 0, flags = SEG_EXPORT}` and nothing else;
//!   the driver maps it with `MapDmaExport` (438) and checks that the lease's owner is the
//!   request's attested sender. `GEM_CREATE` carries exactly one (CP1 D-G1 option B); every
//!   other request carries none.
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
/// Segments one request may declare. `VERSION` needs three (`VM_BIND` and `EXEC` two at
/// most, `GEM_CREATE` one attachment); no served ioctl needs more.
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
/// The descriptor attaches export lease `addr`; `len` is 0 and no other flag is set.
pub const SEG_EXPORT: u32 = 1 << 2;
/// Largest export id the kernel hands out (`1..=isize::MAX`, CP1 §5.1).
pub const MAX_EXPORT_ID: u64 = i64::MAX as u64;

/// A user range a request may touch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub addr: u64,
    pub len: u32,
    pub flags: u32,
}

impl Segment {
    /// An attachment of export lease `export_id`.
    pub const fn export(export_id: u64) -> Self {
        Self {
            addr: export_id,
            len: 0,
            flags: SEG_EXPORT,
        }
    }

    /// Whether this descriptor is well formed: a non-empty mix of `IN`/`OUT`, or an export
    /// attachment with a valid id, zero length and no other flag.
    fn is_valid(&self) -> bool {
        if self.flags & SEG_EXPORT != 0 {
            return self.flags == SEG_EXPORT
                && self.len == 0
                && (1..=MAX_EXPORT_ID).contains(&self.addr);
        }
        self.flags != 0 && self.flags & !(SEG_IN | SEG_OUT) == 0
    }

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
