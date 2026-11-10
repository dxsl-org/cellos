//! Wire of the ADR-0033 CP1 §7 QEMU `edu` export lanes between `/bin/export-client`
//! (export owner) and `/bin/edu-holder` (export holder driving QEMU's `edu` DMA).
//!
//! One fixed 32-byte little-endian message in both directions: `op` (u32), four
//! reserved bytes, then three u64 arguments. Clients submit with `IpcSubmit`; the
//! holder answers each request with `IpcReply` carrying [`OP_STATUS`].

#![no_std]
#![forbid(unsafe_code)]

/// Holder service id in `test-hooks` images (`kernel::task::dma_export::TEST_EXPORT_SERVICE`).
pub const SERVICE_ID: u16 = 0x7AE0;
/// Encoded message length.
pub const MSG_LEN: usize = 32;
/// QEMU `edu` PCI identity.
pub const EDU_VENDOR: u16 = 0x1234;
/// QEMU `edu` PCI device id.
pub const EDU_DEVICE: u16 = 0x11e8;

/// Client hello; the reply's `a` is the client's role (1 or 2).
pub const OP_HELLO: u32 = 1;
/// L1: `a` = export id of two pages; the holder maps it and copies page 0 to page 1
/// through the device. Reply `a` = 0 on success, `b` = window IOVA.
pub const OP_COPY: u32 = 2;
/// `a` = export id; the holder maps it. Reply `a` = 0 or an error code, `b` = IOVA.
pub const OP_MAP: u32 = 3;
/// L2: `a` = IOVA, `b` = bytes; the holder makes the device write 0xEE there.
pub const OP_DMA_TO: u32 = 4;
/// L3: `a` = export id, `b` = IOVA, `c` = owner VA; the client dies next. The
/// holder replies at once, then waits for the lease to vanish and DMAs to `b`.
pub const OP_STALE_AFTER_DEATH: u32 = 5;
/// L4: the holder replies, then exits with its lease still mapped.
pub const OP_DIE: u32 = 6;
/// Holder reply.
pub const OP_STATUS: u32 = 0x80;

/// Reply status: done.
pub const STATUS_OK: u64 = 0;
/// Reply status: the kernel refused the holder's 438.
pub const STATUS_MAP_REFUSED: u64 = 1;
/// Reply status: the device DMA did not complete.
pub const STATUS_DMA_FAILED: u64 = 2;
/// Reply status: unknown request.
pub const STATUS_BAD_REQUEST: u64 = 3;

/// One lane message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    pub op: u32,
    pub a: u64,
    pub b: u64,
    pub c: u64,
}

impl Msg {
    /// A message with `op` and its three arguments.
    pub const fn new(op: u32, a: u64, b: u64, c: u64) -> Self {
        Self { op, a, b, c }
    }

    /// Little-endian encoding, reserved bytes zero.
    pub fn encode(&self) -> [u8; MSG_LEN] {
        let mut out = [0u8; MSG_LEN];
        out[0..4].copy_from_slice(&self.op.to_le_bytes());
        out[8..16].copy_from_slice(&self.a.to_le_bytes());
        out[16..24].copy_from_slice(&self.b.to_le_bytes());
        out[24..32].copy_from_slice(&self.c.to_le_bytes());
        out
    }

    /// Decode exactly [`MSG_LEN`] leading bytes; `None` when short or reserved bytes are set.
    pub fn decode(buf: &[u8]) -> Option<Self> {
        let buf = buf.get(..MSG_LEN)?;
        if buf[4..8] != [0; 4] {
            return None;
        }
        let word = |at: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&buf[at..at + 8]);
            u64::from_le_bytes(w)
        };
        Some(Self {
            op: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            a: word(8),
            b: word(16),
            c: word(24),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_reserved_refusal() {
        let msg = Msg::new(OP_STALE_AFTER_DEATH, 7, 1 << 38, 0x1234_5000);
        let mut bytes = msg.encode();
        assert_eq!(Msg::decode(&bytes), Some(msg));
        assert_eq!(Msg::decode(&bytes[..31]), None);
        bytes[5] = 1;
        assert_eq!(Msg::decode(&bytes), None);
    }
}
