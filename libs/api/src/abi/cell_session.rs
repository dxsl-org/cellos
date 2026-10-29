// SPDX-License-Identifier: Apache-2.0
//! Version-one owner-authenticated child/session transport. References are selectors,
//! never capabilities; the resource owner must authenticate every operation.

use crate::service_binding::{ServiceBinding, SERVICE_BINDING_LEN};

pub mod frontend;
pub mod trust;

pub const VERSION: u32 = 1;
pub const HEADER_LEN: usize = 24;
pub const MAX_REQUEST_LEN: usize = 4152;
pub const MAX_SERVICE_RECORD_LEN: usize = 4000;
pub const TRANSFER_MAX: usize = 512;
pub const OBJECT_REF_LEN: usize = 16;
pub const PRINCIPAL_LEN: usize = 16;
pub const CHILD_REF_LEN: usize = 32;
pub const CONTEXT_HEADER_LEN: usize = 48;
/// Grant records one child context may carry (slots 0..=8).
pub const CONTEXT_MAX_GRANTS: usize = 9;
pub const GRANT_LEN: usize = 40;
/// Header + argv 512 + cwd 4096 + environment 4096 + every grant record.
pub const CONTEXT_MAX: usize = CONTEXT_HEADER_LEN + 512 + 4096 + 4096 + CONTEXT_MAX_GRANTS * GRANT_LEN;
/// Child-context grant kind 6 `InputFrontend`: slot 8, rights 0, owner INPUT,
/// object = the input-issued frontend Ref; admitted only with a bound kernel receipt.
pub const GRANT_KIND_INPUT_FRONTEND: u16 = 6;
pub const SLOT_FRONTEND: u16 = 8;
/// `RegisterBindReceipt` seal of a kind-6 receipt: frontend Ref + exact ChildRefV1.
pub const FRONTEND_SEAL_LEN: usize = OBJECT_REF_LEN + CHILD_REF_LEN;

macro_rules! wire_enum {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        #[repr(u16)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant = $value),+ }
        impl TryFrom<u16> for $name {
            type Error = Status;
            fn try_from(value: u16) -> Result<Self, Status> {
                match value { $($value => Ok(Self::$variant),)+ _ => Err(Status::UnknownOperation) }
            }
        }
    };
}

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0, Eof = 1, UnsupportedVersion = 2, Malformed = 3,
    InvalidFlags = 4, UnknownOperation = 5, Denied = 6, Stale = 7,
    WrongOwner = 8, WrongDirection = 9, WrongSession = 10, WrongEpoch = 11,
    InvalidChild = 12, QuotaExceeded = 13, WouldBlock = 14, TimedOut = 15,
    Cancelled = 16, Revoked = 17, BrokenPipe = 18, NotFound = 19,
    Unsupported = 20, UnsupportedInput = 21, Io = 22, Indeterminate = 23,
    BufferTooSmall = 24, UnknownRequest = 25,
}
impl TryFrom<u16> for Status {
    type Error = Status;
    fn try_from(value: u16) -> Result<Self, Status> {
        match value {
            0 => Ok(Self::Ok), 1 => Ok(Self::Eof), 2 => Ok(Self::UnsupportedVersion),
            3 => Ok(Self::Malformed), 4 => Ok(Self::InvalidFlags),
            5 => Ok(Self::UnknownOperation), 6 => Ok(Self::Denied), 7 => Ok(Self::Stale),
            8 => Ok(Self::WrongOwner), 9 => Ok(Self::WrongDirection),
            10 => Ok(Self::WrongSession), 11 => Ok(Self::WrongEpoch),
            12 => Ok(Self::InvalidChild), 13 => Ok(Self::QuotaExceeded),
            14 => Ok(Self::WouldBlock), 15 => Ok(Self::TimedOut),
            16 => Ok(Self::Cancelled), 17 => Ok(Self::Revoked),
            18 => Ok(Self::BrokenPipe), 19 => Ok(Self::NotFound),
            20 => Ok(Self::Unsupported), 21 => Ok(Self::UnsupportedInput),
            22 => Ok(Self::Io), 23 => Ok(Self::Indeterminate),
            24 => Ok(Self::BufferTooSmall), 25 => Ok(Self::UnknownRequest),
            _ => Err(Self::Malformed),
        }
    }
}
wire_enum!(KernelOp {
    CreateOperation = 1, CreateStream = 2, DuplicateEndpoint = 3,
    DelegateEndpoint = 4, ReadStream = 5, WriteStream = 6, CloseEndpoint = 7,
    CancelOperation = 8, QueryOperation = 9, PrepareChild = 10,
    WriteChildContext = 11, BindOwnerReceipt = 12, CommitChild = 13,
    AbortChild = 14, RegisterBindReceipt = 15, ResolveBindReceipt = 16,
    ReadOwnContext = 17, InspectPreparedGrant = 18, InspectEndpoint = 19,
    CloseOperation = 20, BootstrapSession = 21, ResolveBootstrap = 22,
    InspectPreparedIdentity = 23, QueryReceiptChild = 24,
    AcknowledgeRequests = 25, ReadOwnSessionBinding = 26,
    AuthorizeInputBind = 27, QueryRequest = 28, RestoreSessionInput = 29,
    // QueryChild: preparer-only read of a committed child's recorded terminal
    // state. Request body: ChildRefV1. Result body (24 bytes): state:u32
    // (1 live, 2 retiring, 3 terminal), reason:u32, exit_code:i32,
    // reserved u32 = 0, terminal_sequence:u64.
    QueryChild = 30,
});
wire_enum!(InputOp {
    Create = 1, AttachFrontend = 2, PrepareForeground = 3, CommitForeground = 4,
    RestoreForeground = 5, Resize = 6, SetMode = 7, ReleaseMode = 8,
    PollEvents = 9, Detach = 10, CloseSession = 11, QueryCapabilities = 12,
    SubmitEvents = 13, PreparePaste = 14, WritePaste = 15, CommitPaste = 16,
    AbortPaste = 17, AcknowledgeRequests = 18, QueryRequest = 19,
    SubmitRaw = 20, EndInput = 21, AcknowledgeControl = 22,
    // Frontend attachment selectors; records in `frontend`.
    RegisterFrontend = 23, BindFrontendChild = 24, CreateForFrontend = 25,
    QueryFrontend = 26, ReleaseFrontend = 27, ReattachFrontend = 28,
    // First-trust confirmation selectors (D79-11/D79-13); records in `trust`.
    // RegisterTrustChallenge: live registered VFS only. Body `trust::TrustChallengeV1`
    // (challenge:Ref, target:Principal, expiry_ns:u64, port:u16, u16-prefixed host
    // 1..=253 / algorithm 1..=64 / fingerprint 1..=64 graphic ASCII); result
    // confirmation:Ref. Lane (fence operation) = the challenge Ref.
    // BeginTrustConfirmation: the descriptor's target, as the live committed
    // foreground child with exact lease and epoch. Body
    // `trust::BeginTrustConfirmationV1` (session, lease, epoch, confirmation); empty result.
    // CollectTrustDecision: the registering principal only. Body confirmation:Ref;
    // result `trust::TrustDecisionV1` (decision:u32, challenge, target, session,
    // epoch, descriptor SHA-256). Lane = the confirmation Ref.
    RegisterTrustChallenge = 29, BeginTrustConfirmation = 30, CollectTrustDecision = 31,
});
wire_enum!(VfsOp {
    ReserveCwd = 1, FinalizeCwd = 2, ResolveCwd = 3,
    AcknowledgeRequests = 4, QueryRequest = 5,
});

/// Also carried in typed service IPC (postcard: `id`, `generation` varints).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectRefV1 { pub id: u64, pub generation: u64 }
impl ObjectRefV1 {
    pub const ZERO: Self = Self { id: 0, generation: 0 };
    pub const fn is_zero(self) -> bool { self.id == 0 && self.generation == 0 }
    pub fn to_bytes(self) -> [u8; OBJECT_REF_LEN] {
        let mut out = [0; OBJECT_REF_LEN];
        out[..8].copy_from_slice(&self.id.to_le_bytes());
        out[8..].copy_from_slice(&self.generation.to_le_bytes());
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(bytes);
        let value = r.reference()?;
        r.finish()?;
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrincipalV1 { pub cell_id: u64, pub generation: u64 }
impl PrincipalV1 {
    pub fn to_bytes(self) -> [u8; PRINCIPAL_LEN] {
        let mut out = [0; PRINCIPAL_LEN];
        out[..8].copy_from_slice(&self.cell_id.to_le_bytes());
        out[8..].copy_from_slice(&self.generation.to_le_bytes());
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(bytes);
        let value = r.principal()?;
        r.finish()?;
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChildRefV1 {
    pub principal: PrincipalV1,
    pub root_tid: u64,
    pub publication_generation: u64,
}
impl ChildRefV1 {
    pub fn to_bytes(self) -> [u8; CHILD_REF_LEN] {
        let mut out = [0; CHILD_REF_LEN];
        out[..16].copy_from_slice(&self.principal.to_bytes());
        out[16..24].copy_from_slice(&self.root_tid.to_le_bytes());
        out[24..].copy_from_slice(&self.publication_generation.to_le_bytes());
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(bytes);
        let value = r.child()?;
        r.finish()?;
        Ok(value)
    }
    /// Select the kernel's prepared reservation. This conveys no authority;
    /// owners still verify the full child principal, lineage and bind intent.
    pub const fn prepared_ref(self) -> ObjectRefV1 {
        ObjectRefV1 { id: self.root_tid, generation: self.publication_generation }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestHeaderV1 { pub selector: u16, pub total_len: u32, pub request_id: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultHeaderV1 {
    pub selector: u16, pub status: Status, pub total_len: u32, pub request_id: u64,
}
fn decode_header(record: &[u8]) -> Result<(u16, u16, u32, u64), Status> {
    let mut r = Reader::new(record.get(..HEADER_LEN).ok_or(Status::Malformed)?);
    if r.u32()? != VERSION { return Err(Status::UnsupportedVersion); }
    let selector = r.u16()?;
    let field = r.u16()?;
    let len = r.u32()?;
    if r.u32()? != 0 { return Err(Status::Malformed); }
    let id = r.u64()?;
    if len < HEADER_LEN as u32 { return Err(Status::Malformed); }
    Ok((selector, field, len, id))
}
fn encode_header(selector: u16, field: u16, len: u32, id: u64, out: &mut [u8]) -> Result<(), Status> {
    if len < HEADER_LEN as u32 { return Err(Status::Malformed); }
    let mut w = Writer::new(out.get_mut(..HEADER_LEN).ok_or(Status::BufferTooSmall)?);
    w.u32(VERSION)?; w.u16(selector)?; w.u16(field)?; w.u32(len)?;
    w.u32(0)?; w.u64(id)
}
impl RequestHeaderV1 {
    pub fn decode(record: &[u8]) -> Result<Self, Status> {
        let (selector, flags, total_len, request_id) = decode_header(record)?;
        if flags != 0 { return Err(Status::InvalidFlags); }
        if request_id == 0 || total_len as usize != record.len() || record.len() > MAX_REQUEST_LEN {
            return Err(Status::Malformed);
        }
        Ok(Self { selector, total_len, request_id })
    }
    /// Writes exactly the fixed header; the caller separately encodes the body.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), Status> {
        if self.request_id == 0 || self.total_len as usize > MAX_REQUEST_LEN { return Err(Status::Malformed); }
        encode_header(self.selector, 0, self.total_len, self.request_id, out)
    }
}
impl ResultHeaderV1 {
    pub fn decode(record: &[u8]) -> Result<Self, Status> {
        let (selector, status, total_len, request_id) = decode_header(record)?;
        let status = Status::try_from(status)?;
        if request_id == 0 && !matches!(status, Status::Malformed | Status::UnsupportedVersion | Status::InvalidFlags) {
            return Err(Status::Malformed);
        }
        if status == Status::BufferTooSmall {
            if record.len() != HEADER_LEN || total_len as usize <= HEADER_LEN {
                return Err(Status::Malformed);
            }
        } else if total_len as usize != record.len()
            || (status != Status::Ok && record.len() != HEADER_LEN) {
            return Err(Status::Malformed);
        }
        Ok(Self { selector, status, total_len, request_id })
    }
    /// BufferTooSmall is a header-only response advertising the required length.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), Status> {
        // A malformed request may itself contain correlation zero. Preserve
        // that echo for syntactic rejection, never a successful operation.
        if self.request_id == 0 && !matches!(self.status, Status::Malformed | Status::UnsupportedVersion | Status::InvalidFlags) {
            return Err(Status::Malformed);
        }
        if (self.status == Status::BufferTooSmall && self.total_len as usize <= HEADER_LEN)
            || (self.status != Status::Ok && self.status != Status::BufferTooSmall
                && self.total_len as usize != HEADER_LEN) {
            return Err(Status::Malformed);
        }
        encode_header(self.selector, self.status as u16, self.total_len, self.request_id, out)
    }
}

/// Bounded borrowed little-endian decoder; no native-size fields or allocation.
pub struct Reader<'a> { bytes: &'a [u8], position: usize }
impl<'a> Reader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self { Self { bytes, position: 0 } }
    pub fn take(&mut self, len: usize) -> Result<&'a [u8], Status> {
        let end = self.position.checked_add(len).ok_or(Status::Malformed)?;
        let bytes = self.bytes.get(self.position..end).ok_or(Status::Malformed)?;
        self.position = end;
        Ok(bytes)
    }
    pub fn u8(&mut self) -> Result<u8, Status> { Ok(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Result<u16, Status> { Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| Status::Malformed)?)) }
    pub fn u32(&mut self) -> Result<u32, Status> { Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| Status::Malformed)?)) }
    pub fn u64(&mut self) -> Result<u64, Status> { Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| Status::Malformed)?)) }
    pub fn i32(&mut self) -> Result<i32, Status> { Ok(i32::from_le_bytes(self.take(4)?.try_into().map_err(|_| Status::Malformed)?)) }
    pub fn reference(&mut self) -> Result<ObjectRefV1, Status> { Ok(ObjectRefV1 { id: self.u64()?, generation: self.u64()? }) }
    pub fn principal(&mut self) -> Result<PrincipalV1, Status> { Ok(PrincipalV1 { cell_id: self.u64()?, generation: self.u64()? }) }
    pub fn child(&mut self) -> Result<ChildRefV1, Status> { Ok(ChildRefV1 { principal: self.principal()?, root_tid: self.u64()?, publication_generation: self.u64()? }) }
    pub fn binding(&mut self) -> Result<ServiceBinding, Status> { ServiceBinding::from_bytes(self.take(SERVICE_BINDING_LEN)?).ok_or(Status::Malformed) }
    pub fn finish(self) -> Result<(), Status> { if self.position == self.bytes.len() { Ok(()) } else { Err(Status::Malformed) } }
}
/// Bounded caller-buffer encoder. Failed writes never advance the cursor.
pub struct Writer<'a> { bytes: &'a mut [u8], position: usize }
impl<'a> Writer<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self { Self { bytes, position: 0 } }
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), Status> {
        let end = self.position.checked_add(bytes.len()).ok_or(Status::BufferTooSmall)?;
        self.bytes.get_mut(self.position..end).ok_or(Status::BufferTooSmall)?.copy_from_slice(bytes);
        self.position = end;
        Ok(())
    }
    pub fn zero(&mut self, count: usize) -> Result<(), Status> {
        let end = self.position.checked_add(count).ok_or(Status::BufferTooSmall)?;
        self.bytes.get_mut(self.position..end).ok_or(Status::BufferTooSmall)?.fill(0);
        self.position = end;
        Ok(())
    }
    pub fn position(&self) -> usize { self.position }
    pub fn u8(&mut self, value: u8) -> Result<(), Status> { self.bytes(&[value]) }
    pub fn u16(&mut self, value: u16) -> Result<(), Status> { self.bytes(&value.to_le_bytes()) }
    pub fn u32(&mut self, value: u32) -> Result<(), Status> { self.bytes(&value.to_le_bytes()) }
    pub fn u64(&mut self, value: u64) -> Result<(), Status> { self.bytes(&value.to_le_bytes()) }
    pub fn i32(&mut self, value: i32) -> Result<(), Status> { self.bytes(&value.to_le_bytes()) }
    pub fn reference(&mut self, value: ObjectRefV1) -> Result<(), Status> { self.bytes(&value.to_bytes()) }
    pub fn principal(&mut self, value: PrincipalV1) -> Result<(), Status> { self.bytes(&value.to_bytes()) }
    pub fn child(&mut self, value: ChildRefV1) -> Result<(), Status> { self.bytes(&value.to_bytes()) }
    pub fn binding(&mut self, value: ServiceBinding) -> Result<(), Status> { self.bytes(&value.to_bytes()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_have_fixed_little_endian_bytes_and_exact_lengths() {
        let value = ChildRefV1 { principal: PrincipalV1 { cell_id: 0x102, generation: 3 }, root_tid: 4, publication_generation: 5 };
        let bytes = value.to_bytes();
        assert_eq!(&bytes[..8], &[2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(ChildRefV1::from_bytes(&bytes), Ok(value));
        assert_eq!(ChildRefV1::from_bytes(&bytes[..31]), Err(Status::Malformed));
        assert_eq!(ObjectRefV1::from_bytes(&bytes[..17]), Err(Status::Malformed));
        assert!(ObjectRefV1::ZERO.is_zero());
        assert_eq!(value.prepared_ref(), ObjectRefV1 { id: 4, generation: 5 });
    }
    #[test]
    fn context_bounds_and_frontend_grant_numbers_are_pinned() {
        assert_eq!(CONTEXT_MAX_GRANTS, 9);
        assert_eq!(CONTEXT_MAX, 9112);
        assert_eq!((GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND, FRONTEND_SEAL_LEN), (6, 8, 48));
    }
    #[test]
    fn request_header_rejects_short_trailing_reserved_flags_and_zero_id() {
        let header = RequestHeaderV1 { selector: KernelOp::CreateOperation as u16, total_len: 24, request_id: 1 };
        let mut bytes = [0; 25];
        header.encode(&mut bytes).unwrap();
        assert_eq!(RequestHeaderV1::decode(&bytes[..24]), Ok(header));
        assert_eq!(RequestHeaderV1::decode(&bytes), Err(Status::Malformed));
        for len in 0..24 { assert!(RequestHeaderV1::decode(&bytes[..len]).is_err()); }
        bytes[6] = 1;
        assert_eq!(RequestHeaderV1::decode(&bytes[..24]), Err(Status::InvalidFlags));
        bytes[6] = 0; bytes[12] = 1;
        assert_eq!(RequestHeaderV1::decode(&bytes[..24]), Err(Status::Malformed));
        bytes[12] = 0; bytes[16] = 0;
        assert_eq!(RequestHeaderV1::decode(&bytes[..24]), Err(Status::Malformed));
    }
    #[test]
    fn header_only_encoding_allows_the_bounded_body_to_follow() {
        let mut record = [0; MAX_REQUEST_LEN];
        let header = RequestHeaderV1 { selector: 10, total_len: MAX_REQUEST_LEN as u32, request_id: 1 };
        header.encode(&mut record[..HEADER_LEN]).unwrap();
        assert_eq!(RequestHeaderV1::decode(&record), Ok(header));
        assert_eq!(RequestHeaderV1::decode(&record[..HEADER_LEN]), Err(Status::Malformed));
        let oversized = RequestHeaderV1 { total_len: MAX_REQUEST_LEN as u32 + 1, ..header };
        assert_eq!(oversized.encode(&mut record), Err(Status::Malformed));
        let mut small = [0xaa; HEADER_LEN - 1];
        assert_eq!(header.encode(&mut small), Err(Status::BufferTooSmall));
        assert_eq!(small, [0xaa; HEADER_LEN - 1]);
    }
    #[test]
    fn short_result_advertises_required_size_without_payload() {
        let header = ResultHeaderV1 { selector: 5, status: Status::BufferTooSmall, total_len: 544, request_id: 7 };
        let mut bytes = [0; 24];
        header.encode(&mut bytes).unwrap();
        assert_eq!(ResultHeaderV1::decode(&bytes), Ok(header));
        bytes[6..8].copy_from_slice(&(Status::Ok as u16).to_le_bytes());
        assert_eq!(ResultHeaderV1::decode(&bytes), Err(Status::Malformed));
        bytes[6..8].copy_from_slice(&26u16.to_le_bytes());
        assert_eq!(ResultHeaderV1::decode(&bytes), Err(Status::Malformed));
        assert_eq!(Status::try_from(25), Ok(Status::UnknownRequest));
    }
    #[test]
    fn malformed_zero_correlation_is_echoed_but_never_admitted_as_a_request() {
        let mut bytes = [0; HEADER_LEN];
        let malformed = ResultHeaderV1 { selector: 1, status: Status::Malformed, total_len: HEADER_LEN as u32, request_id: 0 };
        malformed.encode(&mut bytes).unwrap();
        assert_eq!(ResultHeaderV1::decode(&bytes), Ok(malformed));
        bytes[6..8].fill(0);
        assert_eq!(RequestHeaderV1::decode(&bytes), Err(Status::Malformed));
        assert_eq!(ResultHeaderV1::decode(&bytes), Err(Status::Malformed));
        assert_eq!(ResultHeaderV1 { status: Status::Ok, ..malformed }.encode(&mut bytes), Err(Status::Malformed));
    }
    #[test]
    fn cursors_reject_overflow_and_leave_failed_write_position_unchanged() {
        let mut bytes = [0; 12];
        let mut w = Writer::new(&mut bytes);
        w.u64(u64::MAX).unwrap();
        assert_eq!(w.zero(usize::MAX), Err(Status::BufferTooSmall));
        assert_eq!(w.position(), 8);
        w.i32(-17).unwrap();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.take(usize::MAX), Err(Status::Malformed));
        assert_eq!(r.u64(), Ok(u64::MAX));
        assert_eq!(r.i32(), Ok(-17));
        assert_eq!(r.finish(), Ok(()));
    }
    #[test]
    fn query_child_selector_and_24_byte_result_body_codec() {
        assert_eq!(KernelOp::try_from(30), Ok(KernelOp::QueryChild));
        assert_eq!(KernelOp::QueryChild as u16, 30);
        assert!(KernelOp::try_from(31).is_err());
        let mut body = [0xaa; 24];
        let mut w = Writer::new(&mut body);
        w.u32(3).unwrap(); w.u32(0).unwrap(); w.i32(-7).unwrap(); w.u32(0).unwrap(); w.u64(0x0102_0304_0506_0708).unwrap();
        assert_eq!(w.position(), 24);
        assert_eq!(&body[..4], &3u32.to_le_bytes());
        assert_eq!(&body[8..12], &(-7i32).to_le_bytes());
        assert_eq!(&body[16..], &0x0102_0304_0506_0708u64.to_le_bytes());
        let mut r = Reader::new(&body);
        assert_eq!((r.u32(), r.u32(), r.i32(), r.u32(), r.u64()), (Ok(3), Ok(0), Ok(-7), Ok(0), Ok(0x0102_0304_0506_0708)));
        assert_eq!(r.finish(), Ok(()));
        // The request body is exactly one ChildRefV1 (32 bytes).
        let child = ChildRefV1 { principal: PrincipalV1 { cell_id: 1, generation: 2 }, root_tid: 3, publication_generation: 4 };
        let bytes = child.to_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.child(), Ok(child));
        assert_eq!(r.finish(), Ok(()));
        assert_eq!(Reader::new(&bytes[..31]).child(), Err(Status::Malformed));
    }
}
