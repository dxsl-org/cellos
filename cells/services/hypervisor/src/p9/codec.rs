//! 9P message framing: `size[4] type[1] tag[2]` then the body, every field
//! little-endian, a string `len[2]` then its bytes, a qid `type[1]
//! version[4] path[8]`. Reads are bounds-checked: a body shorter than its
//! fields is `EINVAL`, never a panic.

use alloc::vec::Vec;

use super::errno::EINVAL;

/// Bytes of `size[4] type[1] tag[2]`.
pub const HEADER_LEN: usize = 7;
/// The tag of a message outside any request (Tversion, an unparsable header).
pub const NOTAG: u16 = 0xFFFF;
/// Qid type bits.
pub const QT_DIR: u8 = 0x80;
pub const QT_FILE: u8 = 0;

/// A server's identity for a file: kind, version and a path unique per file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qid {
    pub kind: u8,
    pub version: u32,
    pub path: u64,
}

/// `(size, type, tag)` of a message whose header is complete.
pub fn header(msg: &[u8]) -> Option<(u32, u8, u16)> {
    let head = msg.get(..HEADER_LEN)?;
    let size = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    Some((size, head[4], u16::from_le_bytes([head[5], head[6]])))
}

/// Fields read in order from one message body.
pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(body: &'a [u8]) -> Self {
        Self { rest: body }
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], u32> {
        if len > self.rest.len() {
            return Err(EINVAL);
        }
        let (head, tail) = self.rest.split_at(len);
        self.rest = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], u32> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    #[cfg(test)]
    pub fn u8(&mut self) -> Result<u8, u32> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, u32> {
        self.array().map(u16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub fn u64(&mut self) -> Result<u64, u32> {
        self.array().map(u64::from_le_bytes)
    }

    /// The raw bytes of a string; the caller decides what a valid one is.
    pub fn string(&mut self) -> Result<&'a [u8], u32> {
        let len = self.u16()?;
        self.bytes(usize::from(len))
    }

    #[cfg(test)]
    pub fn qid(&mut self) -> Result<Qid, u32> {
        Ok(Qid {
            kind: self.u8()?,
            version: self.u32()?,
            path: self.u64()?,
        })
    }
}

/// One message being built; [`Writer::finish`] fills in its size.
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn message(kind: u8, tag: u16) -> Self {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&[0, 0, 0, 0, kind]);
        buf.extend_from_slice(&tag.to_le_bytes());
        Self { buf }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub fn u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn bytes(&mut self, value: &[u8]) {
        self.buf.extend_from_slice(value);
    }

    /// A string of at most `u16::MAX` bytes (names are at most 255).
    pub fn string(&mut self, value: &[u8]) {
        let len = value.len().min(usize::from(u16::MAX));
        self.u16(len as u16);
        self.bytes(&value[..len]);
    }

    pub fn qid(&mut self, qid: Qid) {
        self.u8(qid.kind);
        self.u32(qid.version);
        self.u64(qid.path);
    }

    pub fn finish(mut self) -> Vec<u8> {
        let size = self.buf.len() as u32;
        self.buf[..4].copy_from_slice(&size.to_le_bytes());
        self.buf
    }
}

/// `Rlerror` for `tag`.
pub fn lerror(tag: u16, errno: u32) -> Vec<u8> {
    let mut w = Writer::message(super::msg::RLERROR, tag);
    w.u32(errno);
    w.finish()
}
