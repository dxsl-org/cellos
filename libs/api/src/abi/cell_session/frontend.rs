// SPDX-License-Identifier: Apache-2.0
//! SessionV1 frontend attachment records (selectors 23–28). A frontend is an
//! input-owned, holder-bound capability: the holder (for example the GUI Terminal)
//! registers it, binds it to the shell child it launches, and the shell creates its
//! own controller session for it. References are selectors, never capabilities; the
//! input service authenticates every use by the attested caller principal.
//!
//! Every record here is a request or result *body* (the 24-byte header is separate).
//! Decoders require the exact length; reserved fields must be zero.

use super::{ChildRefV1, InputOp, ObjectRefV1, PrincipalV1, Reader, Status, Writer};

pub const SEL_REGISTER_FRONTEND: u16 = InputOp::RegisterFrontend as u16;
pub const SEL_BIND_FRONTEND_CHILD: u16 = InputOp::BindFrontendChild as u16;
pub const SEL_CREATE_FOR_FRONTEND: u16 = InputOp::CreateForFrontend as u16;
pub const SEL_QUERY_FRONTEND: u16 = InputOp::QueryFrontend as u16;
pub const SEL_RELEASE_FRONTEND: u16 = InputOp::ReleaseFrontend as u16;
pub const SEL_REATTACH_FRONTEND: u16 = InputOp::ReattachFrontend as u16;

/// Session frontend kinds (the `kind` byte of `Create` and `RegisterFrontend`).
pub const FRONTEND_KIND_SERIAL: u8 = 0;
/// Admitted through `RegisterFrontend`/`CreateForFrontend` only.
pub const FRONTEND_KIND_GUI: u8 = 1;
/// Reserved; `Unsupported` until the SSH frontend is admitted.
pub const FRONTEND_KIND_SSH: u8 = 2;

/// `Lost` control-event reason: the frontend detached or its holder died. The session
/// stays with its controller (Detached); input waits rather than ending.
pub const LOST_FRONTEND: u32 = 4;
/// `Lost` reason-space notice (not a failure): a frontend re-attached; a `Resize`
/// with the new frontend's size precedes it.
pub const FRONTEND_ATTACHED: u32 = 5;

/// `QueryFrontend` states.
pub const FRONTEND_STATE_PENDING: u32 = 0;
pub const FRONTEND_STATE_BOUND: u32 = 1;
pub const FRONTEND_STATE_ATTACHED: u32 = 2;
pub const FRONTEND_STATE_CLOSED: u32 = 3;
pub const FRONTEND_STATE_LOST: u32 = 4;

fn zero8(value: u8) -> Result<(), Status> { if value == 0 { Ok(()) } else { Err(Status::Malformed) } }
fn zero16(value: u16) -> Result<(), Status> { if value == 0 { Ok(()) } else { Err(Status::Malformed) } }
fn zero32(value: u32) -> Result<(), Status> { if value == 0 { Ok(()) } else { Err(Status::Malformed) } }
fn flags(value: u8) -> Result<(), Status> { if value == 0 { Ok(()) } else { Err(Status::InvalidFlags) } }

/// Selector 23 request: `operation:Ref, kind:u8, flags:u8=0, rows:u16, cols:u16, reserved:u16=0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterFrontendV1 { pub operation: ObjectRefV1, pub kind: u8, pub rows: u16, pub cols: u16 }
impl RegisterFrontendV1 {
    pub const LEN: usize = 24;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.operation)?; w.u8(self.kind)?; w.u8(0)?; w.u16(self.rows)?; w.u16(self.cols)?; w.u16(0)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let operation = r.reference()?; let kind = r.u8()?; let f = r.u8()?;
        let rows = r.u16()?; let cols = r.u16()?; let reserved = r.u16()?; r.finish()?;
        flags(f)?; zero16(reserved)?;
        Ok(Self { operation, kind, rows, cols })
    }
}

/// Selector 24 request: `frontend:Ref, child:ChildRef`. Result: `receipt:Ref`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindFrontendChildV1 { pub frontend: ObjectRefV1, pub child: ChildRefV1 }
impl BindFrontendChildV1 {
    pub const LEN: usize = 48;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.frontend)?; w.child(self.child)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let value = Self { frontend: r.reference()?, child: r.child()? }; r.finish()?;
        Ok(value)
    }
}

/// Selector 25 request: `operation:Ref, bootstrap:Ref, frontend:Ref, flags:u8=0,
/// reserved:u8=0, rows:u16, cols:u16, reserved:u16=0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateForFrontendV1 {
    pub operation: ObjectRefV1, pub bootstrap: ObjectRefV1, pub frontend: ObjectRefV1,
    pub rows: u16, pub cols: u16,
}
impl CreateForFrontendV1 {
    pub const LEN: usize = 56;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.operation)?; w.reference(self.bootstrap)?; w.reference(self.frontend)?;
        w.u8(0)?; w.u8(0)?; w.u16(self.rows)?; w.u16(self.cols)?; w.u16(0)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let operation = r.reference()?; let bootstrap = r.reference()?; let frontend = r.reference()?;
        let f = r.u8()?; let reserved = r.u8()?; let rows = r.u16()?; let cols = r.u16()?; let tail = r.u16()?; r.finish()?;
        flags(f)?; zero8(reserved)?; zero16(tail)?;
        Ok(Self { operation, bootstrap, frontend, rows, cols })
    }
}

/// Selector 25 result: exactly the `Create` result (`session, controller, lease,
/// epoch:u64, mode:u32, reserved:u32=0, endpoints[4]`, 128 bytes) followed by the
/// frontend's `attachment:Ref`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateForFrontendResultV1 {
    pub session: ObjectRefV1, pub controller: PrincipalV1, pub lease: ObjectRefV1,
    pub epoch: u64, pub mode: u32, pub endpoints: [ObjectRefV1; 4], pub attachment: ObjectRefV1,
}
impl CreateForFrontendResultV1 {
    pub const LEN: usize = 144;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.session)?; w.principal(self.controller)?; w.reference(self.lease)?;
        w.u64(self.epoch)?; w.u32(self.mode)?; w.u32(0)?;
        for endpoint in self.endpoints { w.reference(endpoint)?; }
        w.reference(self.attachment)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let session = r.reference()?; let controller = r.principal()?; let lease = r.reference()?;
        let epoch = r.u64()?; let mode = r.u32()?; zero32(r.u32()?)?;
        let endpoints = [r.reference()?, r.reference()?, r.reference()?, r.reference()?];
        let attachment = r.reference()?; r.finish()?;
        Ok(Self { session, controller, lease, epoch, mode, endpoints, attachment })
    }
}

/// Selector 26 result: `state:u32, reserved:u32=0, session:Ref, attachment:Ref,
/// resume:Ref`. The references are zero unless `state == FRONTEND_STATE_ATTACHED`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontendStatusV1 { pub state: u32, pub session: ObjectRefV1, pub attachment: ObjectRefV1, pub resume: ObjectRefV1 }
impl FrontendStatusV1 {
    pub const LEN: usize = 56;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.u32(self.state)?; w.u32(0)?; w.reference(self.session)?; w.reference(self.attachment)?; w.reference(self.resume)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let state = r.u32()?; zero32(r.u32()?)?;
        let value = Self { state, session: r.reference()?, attachment: r.reference()?, resume: r.reference()? };
        r.finish()?;
        if state > FRONTEND_STATE_LOST { return Err(Status::Malformed); }
        if state != FRONTEND_STATE_ATTACHED && !(value.session.is_zero() && value.attachment.is_zero() && value.resume.is_zero()) {
            return Err(Status::Malformed);
        }
        Ok(value)
    }
}

/// Selector 28 request: `frontend:Ref` (new, Pending, same holder), `resume:Ref`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReattachFrontendV1 { pub frontend: ObjectRefV1, pub resume: ObjectRefV1 }
impl ReattachFrontendV1 {
    pub const LEN: usize = 32;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.frontend)?; w.reference(self.resume)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let value = Self { frontend: r.reference()?, resume: r.reference()? }; r.finish()?;
        Ok(value)
    }
}

/// Selector 28 result: `session:Ref, attachment:Ref`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReattachResultV1 { pub session: ObjectRefV1, pub attachment: ObjectRefV1 }
impl ReattachResultV1 {
    pub const LEN: usize = 32;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.session)?; w.reference(self.attachment)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let value = Self { session: r.reference()?, attachment: r.reference()? }; r.finish()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(id: u64) -> ObjectRefV1 { ObjectRefV1 { id, generation: id + 100 } }

    #[test]
    fn selectors_are_the_next_free_input_values() {
        assert_eq!([SEL_REGISTER_FRONTEND, SEL_BIND_FRONTEND_CHILD, SEL_CREATE_FOR_FRONTEND,
            SEL_QUERY_FRONTEND, SEL_RELEASE_FRONTEND, SEL_REATTACH_FRONTEND], [23, 24, 25, 26, 27, 28]);
        for selector in 23..=28u16 { assert_eq!(InputOp::try_from(selector).map(|op| op as u16), Ok(selector)); }
        // 29..=31 are the trust selectors (`super::trust`); 32 is the next free value.
        for selector in 29..=31u16 { assert_eq!(InputOp::try_from(selector).map(|op| op as u16), Ok(selector)); }
        assert_eq!(InputOp::try_from(32), Err(Status::UnknownOperation));
        assert_eq!((FRONTEND_KIND_SERIAL, FRONTEND_KIND_GUI, FRONTEND_KIND_SSH), (0, 1, 2));
        assert_eq!((LOST_FRONTEND, FRONTEND_ATTACHED), (4, 5));
        assert_eq!([FRONTEND_STATE_PENDING, FRONTEND_STATE_BOUND, FRONTEND_STATE_ATTACHED,
            FRONTEND_STATE_CLOSED, FRONTEND_STATE_LOST], [0, 1, 2, 3, 4]);
    }

    #[test]
    fn register_has_fixed_24_byte_layout_and_rejects_flags_and_reserved() {
        let value = RegisterFrontendV1 { operation: r(1), kind: FRONTEND_KIND_GUI, rows: 24, cols: 80 };
        let mut bytes = [0xaa; 24];
        assert_eq!(value.encode(&mut bytes), Ok(24));
        assert_eq!(&bytes[16..], &[1, 0, 24, 0, 80, 0, 0, 0]);
        assert_eq!(RegisterFrontendV1::decode(&bytes), Ok(value));
        assert_eq!(RegisterFrontendV1::decode(&bytes[..23]), Err(Status::Malformed));
        let mut flagged = bytes; flagged[17] = 1;
        assert_eq!(RegisterFrontendV1::decode(&flagged), Err(Status::InvalidFlags));
        let mut reserved = bytes; reserved[23] = 1;
        assert_eq!(RegisterFrontendV1::decode(&reserved), Err(Status::Malformed));
        assert_eq!(value.encode(&mut [0; 23]), Err(Status::BufferTooSmall));
    }

    #[test]
    fn bind_create_reattach_round_trip_at_exact_lengths() {
        let child = ChildRefV1 { principal: PrincipalV1 { cell_id: 5, generation: 6 }, root_tid: 7, publication_generation: 8 };
        let bind = BindFrontendChildV1 { frontend: r(2), child };
        let mut bytes = [0; 48];
        assert_eq!(bind.encode(&mut bytes), Ok(48));
        assert_eq!(BindFrontendChildV1::decode(&bytes), Ok(bind));
        assert_eq!(BindFrontendChildV1::decode(&bytes[..47]), Err(Status::Malformed));

        let create = CreateForFrontendV1 { operation: r(1), bootstrap: r(2), frontend: r(3), rows: 30, cols: 100 };
        let mut bytes = [0; 56];
        assert_eq!(create.encode(&mut bytes), Ok(56));
        assert_eq!(&bytes[48..], &[0, 0, 30, 0, 100, 0, 0, 0]);
        assert_eq!(CreateForFrontendV1::decode(&bytes), Ok(create));
        let mut flagged = bytes; flagged[48] = 1;
        assert_eq!(CreateForFrontendV1::decode(&flagged), Err(Status::InvalidFlags));
        let mut reserved = bytes; reserved[49] = 1;
        assert_eq!(CreateForFrontendV1::decode(&reserved), Err(Status::Malformed));
        let mut tail = bytes; tail[55] = 1;
        assert_eq!(CreateForFrontendV1::decode(&tail), Err(Status::Malformed));

        let reattach = ReattachFrontendV1 { frontend: r(9), resume: r(10) };
        let mut bytes = [0; 32];
        assert_eq!(reattach.encode(&mut bytes), Ok(32));
        assert_eq!(ReattachFrontendV1::decode(&bytes), Ok(reattach));
        let result = ReattachResultV1 { session: r(11), attachment: r(12) };
        assert_eq!(result.encode(&mut bytes), Ok(32));
        assert_eq!(ReattachResultV1::decode(&bytes), Ok(result));
        assert_eq!(ReattachResultV1::decode(&bytes[..31]), Err(Status::Malformed));
    }

    #[test]
    fn create_result_is_the_create_record_plus_attachment() {
        let value = CreateForFrontendResultV1 {
            session: r(1), controller: PrincipalV1 { cell_id: 2, generation: 3 }, lease: r(4), epoch: 1, mode: 2,
            endpoints: [r(5), r(6), r(7), r(8)], attachment: r(9),
        };
        let mut bytes = [0; 144];
        assert_eq!(value.encode(&mut bytes), Ok(144));
        assert_eq!(&bytes[128..], &r(9).to_bytes());
        assert_eq!(CreateForFrontendResultV1::decode(&bytes), Ok(value));
        let mut reserved = bytes; reserved[60] = 1;
        assert_eq!(CreateForFrontendResultV1::decode(&reserved), Err(Status::Malformed));
        assert_eq!(CreateForFrontendResultV1::decode(&bytes[..128]), Err(Status::Malformed));
    }

    #[test]
    fn status_refs_are_zero_unless_attached() {
        let attached = FrontendStatusV1 { state: FRONTEND_STATE_ATTACHED, session: r(1), attachment: r(2), resume: r(3) };
        let mut bytes = [0; 56];
        assert_eq!(attached.encode(&mut bytes), Ok(56));
        assert_eq!(FrontendStatusV1::decode(&bytes), Ok(attached));
        let lost = FrontendStatusV1 { state: FRONTEND_STATE_LOST, session: ObjectRefV1::ZERO, attachment: ObjectRefV1::ZERO, resume: ObjectRefV1::ZERO };
        lost.encode(&mut bytes).unwrap();
        assert_eq!(FrontendStatusV1::decode(&bytes), Ok(lost));
        let mut leaked = bytes; leaked[8] = 1;
        assert_eq!(FrontendStatusV1::decode(&leaked), Err(Status::Malformed));
        let mut unknown = bytes; unknown[0] = 5;
        assert_eq!(FrontendStatusV1::decode(&unknown), Err(Status::Malformed));
        let mut reserved = bytes; reserved[4] = 1;
        assert_eq!(FrontendStatusV1::decode(&reserved), Err(Status::Malformed));
    }
}
