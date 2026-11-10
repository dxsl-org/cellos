//! `XE_GPU` reply frames: built by the driver, checked and applied by the client.

use super::{Segment, SegmentMemory, DESCRIPTOR_LEN, HEADER_LEN, MAX_WRITES, SEG_OUT};
use crate::uapi::ioctl::{EFAULT, EINVAL};
use crate::uapi::{u16_at, u32_at, u64_at, UserMemory};
use alloc::vec::Vec;

/// Build a reply into `out` (at least [`super::MAX_REPLY_LEN`] bytes) and return its length.
pub fn encode_reply(
    out: &mut [u8],
    errno: i32,
    value: u32,
    arg: &[u8],
    mem: Option<&SegmentMemory<'_>>,
) -> usize {
    let writes = mem.map_or(&[][..], |m| &m.writes[..]);
    let data = mem.map_or(&[][..], |m| &m.data[..]);
    out[..HEADER_LEN].fill(0);
    out[0..4].copy_from_slice(&(errno as u32).to_le_bytes());
    out[4..8].copy_from_slice(&value.to_le_bytes());
    out[8..10].copy_from_slice(&(arg.len() as u16).to_le_bytes());
    out[10] = writes.len() as u8;
    let mut at = HEADER_LEN;
    for &(addr, len) in writes {
        out[at..at + DESCRIPTOR_LEN].fill(0);
        out[at..at + 8].copy_from_slice(&addr.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&(len as u32).to_le_bytes());
        at += DESCRIPTOR_LEN;
    }
    out[at..at + arg.len()].copy_from_slice(arg);
    at += arg.len();
    out[at..at + data.len()].copy_from_slice(data);
    at + data.len()
}

/// A decoded reply; borrows the received frame.
#[derive(Debug)]
pub struct Reply<'a> {
    /// `0` on success, else the positive errno.
    pub errno: i32,
    pub value: u32,
    pub arg: &'a [u8],
    writes: Vec<(u64, &'a [u8])>,
}

/// Decode a reply to a request that declared `segments`. A write outside the declared
/// `OUT` ranges is a broken reply and is refused whole (`EFAULT`), never partly applied.
pub fn decode_reply<'a>(frame: &'a [u8], segments: &[Segment]) -> Result<Reply<'a>, i32> {
    let count = *frame.get(10).ok_or(EINVAL)? as usize;
    if frame.len() < HEADER_LEN || frame[11] != 0 || u32_at(frame, 12) != 0 || count > MAX_WRITES {
        return Err(EINVAL);
    }
    let arg_len = u16_at(frame, 8) as usize;
    let mut at = HEADER_LEN;
    let mut spans = Vec::with_capacity(count);
    for _ in 0..count {
        let d = frame.get(at..at + DESCRIPTOR_LEN).ok_or(EINVAL)?;
        if u32_at(d, 12) != 0 {
            return Err(EINVAL);
        }
        spans.push((u64_at(d, 0), u32_at(d, 8) as usize));
        at += DESCRIPTOR_LEN;
    }
    let arg = frame.get(at..at + arg_len).ok_or(EINVAL)?;
    at += arg_len;
    let mut writes = Vec::with_capacity(count);
    for (addr, len) in spans {
        if !segments.iter().any(|s| s.covers(addr, len, SEG_OUT)) {
            return Err(EFAULT);
        }
        writes.push((addr, frame.get(at..at + len).ok_or(EINVAL)?));
        at += len;
    }
    if at != frame.len() {
        return Err(EINVAL);
    }
    Ok(Reply {
        errno: u32_at(frame, 0) as i32,
        value: u32_at(frame, 4),
        arg,
        writes,
    })
}

impl Reply<'_> {
    /// Apply the driver's user-memory writes to the client's memory.
    pub fn apply_writes(&self, mem: &mut impl UserMemory) -> Result<(), i32> {
        for &(addr, bytes) in &self.writes {
            mem.write(addr, bytes)?;
        }
        Ok(())
    }
}
