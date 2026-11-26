// SPDX-License-Identifier: Apache-2.0
//! Sound output service protocol (`abi::syscall::service::AUDIO`).
//!
//! One PCM output stream in one fixed format ([`RATE`] Hz, [`CHANNELS`]
//! channels, signed 16-bit little-endian, interleaved). One client at a time
//! holds the stream: the first [`OPEN`] wins, other senders get
//! [`STATUS_IN_USE`] until it closes or exits.
//!
//! - [`AudioOpen`] (call): bind the stream to the sender and name a Grant the
//!   sender owns and shares read-only with the driver, at least [`MAX_CHUNK`]
//!   bytes long, through which PCM arrives.
//! - [`AudioWrite`] (call): the first `len` bytes of that Grant are the next
//!   PCM frames. The driver copies them (owner-checked) before it replies, so
//!   the sender may refill the Grant as soon as the reply arrives. A driver
//!   whose queue is full answers [`STATUS_BUSY`] and the chunk is dropped:
//!   a client never waits on audio, it loses sound instead.
//! - [`CLOSE`] (posted, opcode byte only): release the stream.
//!
//! Every call is answered with [`AudioReply`].

/// Frames per second.
pub const RATE: u32 = 48_000;
/// Interleaved channels per frame.
pub const CHANNELS: u32 = 2;
/// Bytes per frame (S16LE stereo).
pub const FRAME_BYTES: u32 = 4;
/// Largest chunk one write carries (about 85 ms), a whole number of frames.
pub const MAX_CHUNK: u32 = 16_384;

pub const OPEN: u8 = 1;
pub const WRITE: u8 = 2;
pub const CLOSE: u8 = 3;

pub const STATUS_OK: u32 = 0;
/// The driver's queue is full; the chunk was dropped.
pub const STATUS_BUSY: u32 = 1;
/// A malformed request, a length that is not a whole number of frames, or a
/// Grant the sender does not own or that is too short.
pub const STATUS_INVALID: u32 = 2;
/// A write from a sender that does not hold the stream.
pub const STATUS_NOT_OPEN: u32 = 3;
/// Another client holds the stream.
pub const STATUS_IN_USE: u32 = 4;
/// The sound device failed or is absent.
pub const STATUS_DEVICE: u32 = 5;

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// `bytes[..8]` with `opcode` and zero reserved bytes 1..4.
fn frame8(bytes: &[u8], opcode: u8) -> Option<&[u8]> {
    let bytes = bytes.get(..8)?;
    (bytes[0] == opcode && bytes[1..4] == [0; 3]).then_some(bytes)
}

/// LE layout: `opcode:u8, reserved:[u8;3], grant:u32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioOpen {
    pub grant: u32,
}

impl AudioOpen {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        out[0] = OPEN;
        out[4..8].copy_from_slice(&self.grant.to_le_bytes());
        out
    }

    /// `None` for a short frame, another opcode or nonzero reserved bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        frame8(bytes, OPEN).map(|bytes| Self {
            grant: le32(bytes, 4),
        })
    }
}

/// LE layout: `opcode:u8, reserved:[u8;3], len:u32`; `len` is a whole number
/// of frames, 1..=[`MAX_CHUNK`] bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioWrite {
    pub len: u32,
}

impl AudioWrite {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        out[0] = WRITE;
        out[4..8].copy_from_slice(&self.len.to_le_bytes());
        out
    }

    /// `None` for a short frame, another opcode, nonzero reserved bytes or a
    /// length that is empty, over [`MAX_CHUNK`] or not whole frames.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let len = le32(frame8(bytes, WRITE)?, 4);
        (len > 0 && len <= MAX_CHUNK && len.is_multiple_of(FRAME_BYTES)).then_some(Self { len })
    }
}

/// LE layout: `status:u32, queued:u32`. `queued` is how many PCM bytes the
/// driver holds that the device has not played yet (0 when unknown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioReply {
    pub status: u32,
    pub queued: u32,
}

impl AudioReply {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        out[0..4].copy_from_slice(&self.status.to_le_bytes());
        out[4..8].copy_from_slice(&self.queued.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..Self::LEN)?;
        Some(Self {
            status: le32(bytes, 0),
            queued: le32(bytes, 4),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let open = AudioOpen { grant: 0x7000 };
        assert_eq!(AudioOpen::decode(&open.encode()), Some(open));
        let write = AudioWrite { len: MAX_CHUNK };
        assert_eq!(AudioWrite::decode(&write.encode()), Some(write));
        let reply = AudioReply {
            status: STATUS_BUSY,
            queued: 9600,
        };
        assert_eq!(AudioReply::decode(&reply.encode()), Some(reply));
    }

    #[test]
    fn bad_lengths_opcodes_and_reserved_bytes_are_refused() {
        for len in [0, 2, MAX_CHUNK + FRAME_BYTES] {
            assert_eq!(AudioWrite::decode(&AudioWrite { len }.encode()), None);
        }
        let mut open = AudioOpen { grant: 1 }.encode();
        open[3] = 1;
        assert_eq!(AudioOpen::decode(&open), None);
        assert_eq!(AudioOpen::decode(&AudioWrite { len: 4 }.encode()), None);
        assert_eq!(AudioOpen::decode(&[OPEN, 0, 0, 0]), None);
        assert_eq!(AudioReply::decode(&[0; 7]), None);
    }
}
