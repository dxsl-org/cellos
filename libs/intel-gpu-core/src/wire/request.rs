//! `XE_GPU` request frames: built by the client, decoded by the driver, plus the user
//! memory view the decoded request gives the ioctl decoder.

use super::{
    Segment, DESCRIPTOR_LEN, HEADER_LEN, MAX_REPLY_LEN, MAX_REQUEST_LEN, MAX_SEGMENTS, MAX_WRITES,
    SEG_EXPORT, SEG_IN, SEG_OUT,
};
use crate::uapi::ioctl::{EFAULT, EINVAL, ENOMEM};
use crate::uapi::{u16_at, u32_at, u64_at, UserMemory};
use alloc::vec::Vec;

/// A decoded request; borrows the received frame.
#[derive(Debug)]
pub struct Request<'a> {
    pub op: u8,
    pub file: u32,
    pub request: u32,
    pub arg: &'a [u8],
    segments: Vec<Segment>,
    in_data: &'a [u8],
}

/// Build a request into `out` and return its length. `IN` segment bytes are read from the
/// caller's own memory, so an unreadable range fails here with the caller's `EFAULT`.
pub fn encode_request(
    out: &mut [u8],
    op: u8,
    file: u32,
    request: u32,
    arg: &[u8],
    segments: &[Segment],
    mem: &impl UserMemory,
) -> Result<usize, i32> {
    if segments.len() > MAX_SEGMENTS || !segments.iter().all(Segment::is_valid) {
        return Err(EINVAL);
    }
    let arg_len = u16::try_from(arg.len()).map_err(|_| EINVAL)?;
    let mut len = HEADER_LEN + segments.len() * DESCRIPTOR_LEN;
    out.get_mut(..len).ok_or(EINVAL)?.fill(0);
    out[0] = op;
    out[1] = segments.len() as u8;
    out[2..4].copy_from_slice(&arg_len.to_le_bytes());
    out[4..8].copy_from_slice(&file.to_le_bytes());
    out[8..12].copy_from_slice(&request.to_le_bytes());
    for (i, s) in segments.iter().enumerate() {
        let at = HEADER_LEN + i * DESCRIPTOR_LEN;
        out[at..at + 8].copy_from_slice(&s.addr.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&s.len.to_le_bytes());
        out[at + 12..at + 16].copy_from_slice(&s.flags.to_le_bytes());
    }
    let limit = out.len().min(MAX_REQUEST_LEN);
    let append = |len: &mut usize, bytes: usize| -> Result<core::ops::Range<usize>, i32> {
        let end = len
            .checked_add(bytes)
            .filter(|&e| e <= limit)
            .ok_or(ENOMEM)?;
        let range = *len..end;
        *len = end;
        Ok(range)
    };
    let range = append(&mut len, arg.len())?;
    out[range].copy_from_slice(arg);
    for s in segments.iter().filter(|s| s.flags & SEG_IN != 0) {
        let range = append(&mut len, s.len as usize)?;
        mem.read(s.addr, &mut out[range])?;
    }
    Ok(len)
}

/// Decode the request at the start of `buf`. A receive reports no length, so the header's
/// counts define the frame; it must lie within `buf` and [`MAX_REQUEST_LEN`]. `EINVAL` for
/// any malformed frame.
pub fn decode_request(buf: &[u8]) -> Result<Request<'_>, i32> {
    let frame = &buf[..buf.len().min(MAX_REQUEST_LEN)];
    if frame.len() < HEADER_LEN || u32_at(frame, 12) != 0 {
        return Err(EINVAL);
    }
    let count = frame[1] as usize;
    let arg_len = u16_at(frame, 2) as usize;
    if count > MAX_SEGMENTS {
        return Err(EINVAL);
    }
    let mut at = HEADER_LEN;
    let mut segments = Vec::with_capacity(count);
    let mut in_len = 0usize;
    for _ in 0..count {
        let segment = Segment {
            addr: u64_at(frame.get(at..at + DESCRIPTOR_LEN).ok_or(EINVAL)?, 0),
            len: u32_at(frame, at + 8),
            flags: u32_at(frame, at + 12),
        };
        if !segment.is_valid() {
            return Err(EINVAL);
        }
        if segment.flags & SEG_IN != 0 {
            in_len = in_len.checked_add(segment.len as usize).ok_or(EINVAL)?;
        }
        segments.push(segment);
        at += DESCRIPTOR_LEN;
    }
    let arg_end = at.checked_add(arg_len).ok_or(EINVAL)?;
    let end = arg_end
        .checked_add(in_len)
        .filter(|&end| end <= frame.len())
        .ok_or(EINVAL)?;
    Ok(Request {
        op: frame[0],
        file: u32_at(frame, 4),
        request: u32_at(frame, 8),
        arg: &frame[at..arg_end],
        segments,
        in_data: &frame[arg_end..end],
    })
}

impl Request<'_> {
    /// The client memory this request exposes, for [`crate::uapi::DrmFile::ioctl`].
    pub fn memory(&self) -> SegmentMemory<'_> {
        SegmentMemory {
            segments: &self.segments,
            in_data: self.in_data,
            writes: Vec::new(),
            data: Vec::new(),
            // Every write costs its descriptor and bytes in a reply that also carries the
            // header and the echoed arg.
            budget: MAX_REPLY_LEN.saturating_sub(HEADER_LEN + self.arg.len()),
        }
    }

    /// The export ids attached to this request, in declaration order.
    pub fn exports(&self) -> Vec<u64> {
        self.segments
            .iter()
            .filter(|s| s.flags == SEG_EXPORT)
            .map(|s| s.addr)
            .collect()
    }
}

/// Driver-side [`UserMemory`]: reads from `IN` segments, records writes to `OUT` segments.
pub struct SegmentMemory<'a> {
    segments: &'a [Segment],
    in_data: &'a [u8],
    /// `(addr, len)` of each recorded write, in order; their bytes are in `data`.
    pub(super) writes: Vec<(u64, usize)>,
    pub(super) data: Vec<u8>,
    /// Reply bytes still free for write descriptors and data.
    budget: usize,
}

impl UserMemory for SegmentMemory<'_> {
    fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), i32> {
        let mut base = 0usize;
        for s in self.segments.iter().filter(|s| s.flags & SEG_IN != 0) {
            if s.covers(addr, out.len(), SEG_IN) {
                let start = base + (addr - s.addr) as usize;
                out.copy_from_slice(&self.in_data[start..start + out.len()]);
                return Ok(());
            }
            base += s.len as usize;
        }
        Err(EFAULT)
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), i32> {
        if !self
            .segments
            .iter()
            .any(|s| s.covers(addr, data.len(), SEG_OUT))
        {
            return Err(EFAULT);
        }
        let cost = DESCRIPTOR_LEN + data.len();
        if self.writes.len() == MAX_WRITES || cost > self.budget {
            return Err(ENOMEM);
        }
        self.budget -= cost;
        self.writes.push((addr, data.len()));
        self.data.extend_from_slice(data);
        Ok(())
    }
}
