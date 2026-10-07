// SPDX-License-Identifier: MPL-2.0
//! Allocation-free, allowlisted engine activation. Caller identity never comes
//! from these frames: init obtains it from the kernel's receive trailer.
#![no_std]
#![forbid(unsafe_code)]

pub const FRAME_BYTES: usize = 32;
pub const MAX_LEASES: usize = 16;
const REQUEST_MAGIC: &[u8; 8] = b"OCLSREQ1";
const RESPONSE_MAGIC: &[u8; 8] = b"OCLSRSP1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Engine { JavaScript = 1, Pdf = 2 }
impl Engine {
    pub const fn path(self) -> &'static str {
        match self { Self::JavaScript => "/bin/ocel-quickjs", Self::Pdf => "/bin/ocel-pdf" }
    }
    fn decode(value: u8) -> Option<Self> {
        match value { 1 => Some(Self::JavaScript), 2 => Some(Self::Pdf), _ => None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Acquire { engine: Engine },
    Release { engine: Engine, lease: u64 },
}
impl Request {
    pub const fn engine(self) -> Engine {
        match self { Self::Acquire { engine } | Self::Release { engine, .. } => engine }
    }
    pub fn encode(self) -> [u8; FRAME_BYTES] {
        let mut frame = [0; FRAME_BYTES];
        frame[..8].copy_from_slice(REQUEST_MAGIC);
        frame[9] = self.engine() as u8;
        match self {
            Self::Acquire { .. } => frame[8] = 1,
            Self::Release { lease, .. } => { frame[8] = 2; frame[16..24].copy_from_slice(&lease.to_le_bytes()); }
        }
        frame
    }
    pub fn decode(frame: &[u8]) -> Option<Self> {
        if frame.len() < FRAME_BYTES || &frame[..8] != REQUEST_MAGIC
            || frame[10..16].iter().any(|byte| *byte != 0)
            || frame[24..32].iter().any(|byte| *byte != 0) { return None; }
        let engine = Engine::decode(frame[9])?;
        let lease = u64::from_le_bytes(frame[16..24].try_into().ok()?);
        match (frame[8], lease) {
            (1, 0) => Some(Self::Acquire { engine }),
            (2, lease) if lease != 0 => Some(Self::Release { engine, lease }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Failure { Unavailable = 2, Denied = 3, Busy = 4, Failed = 5 }
impl Failure {
    fn decode(value: u8) -> Option<Self> {
        match value { 2 => Some(Self::Unavailable), 3 => Some(Self::Denied), 4 => Some(Self::Busy), 5 => Some(Self::Failed), _ => None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Ready { engine: Engine, lease: u64, tid: u64 },
    Released { engine: Engine },
    Error { engine: Engine, failure: Failure },
}
impl Response {
    pub const fn engine(self) -> Engine {
        match self { Self::Ready { engine, .. } | Self::Released { engine } | Self::Error { engine, .. } => engine }
    }
    pub fn encode(self) -> [u8; FRAME_BYTES] {
        let mut frame = [0; FRAME_BYTES];
        frame[..8].copy_from_slice(RESPONSE_MAGIC);
        frame[9] = self.engine() as u8;
        match self {
            Self::Ready { lease, tid, .. } => {
                frame[16..24].copy_from_slice(&lease.to_le_bytes());
                frame[24..32].copy_from_slice(&tid.to_le_bytes());
            }
            Self::Released { .. } => frame[8] = 1,
            Self::Error { failure, .. } => frame[8] = failure as u8,
        }
        frame
    }
    pub fn decode(frame: &[u8]) -> Option<Self> {
        if frame.len() < FRAME_BYTES || &frame[..8] != RESPONSE_MAGIC
            || frame[10..16].iter().any(|byte| *byte != 0) { return None; }
        let engine = Engine::decode(frame[9])?;
        let lease = u64::from_le_bytes(frame[16..24].try_into().ok()?);
        let tid = u64::from_le_bytes(frame[24..32].try_into().ok()?);
        match (frame[8], lease, tid) {
            (0, lease, tid) if lease != 0 && tid != 0 => Some(Self::Ready { engine, lease, tid }),
            (1, 0, 0) => Some(Self::Released { engine }),
            (status, 0, 0) => Some(Self::Error { engine, failure: Failure::decode(status)? }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_selectors_and_lease_shapes_fail_closed() {
        let mut acquire = Request::Acquire { engine: Engine::Pdf }.encode();
        for selector in [0, 3, 255] { acquire[9] = selector; assert!(Request::decode(&acquire).is_none()); }
        acquire[9] = Engine::Pdf as u8;
        acquire[16] = 1;
        assert!(Request::decode(&acquire).is_none());
        let release = Request::Release { engine: Engine::Pdf, lease: 0 }.encode();
        assert!(Request::decode(&release).is_none());
        let mut valid = Request::Acquire { engine: Engine::JavaScript }.encode();
        valid[10] = 1;
        assert!(Request::decode(&valid).is_none());
        assert!(Request::decode(&valid[..31]).is_none());
    }
    #[test]
    fn provider_response_cannot_hide_invalid_handles_or_status() {
        for (lease, tid) in [(0, 1), (1, 0), (0, 0)] {
            assert!(Response::decode(&Response::Ready { engine: Engine::Pdf, lease, tid }.encode()).is_none());
        }
        let mut error = Response::Error { engine: Engine::Pdf, failure: Failure::Unavailable }.encode();
        error[24] = 1;
        assert!(Response::decode(&error).is_none());
        error[24] = 0; error[8] = 255;
        assert!(Response::decode(&error).is_none());
    }
}
