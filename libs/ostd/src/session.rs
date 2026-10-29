// SPDX-License-Identifier: MPL-2.0
//! Authenticated CellSessionV1 transport and operation-scoped request correlation.
use api::cell_session::{KernelOp, ObjectRefV1, RequestHeaderV1, ResultHeaderV1, Status, HEADER_LEN, MAX_REQUEST_LEN, MAX_SERVICE_RECORD_LEN};
use api::service_binding::ServiceBinding;

/// Execute exactly one bounded kernel request. Errors remain in `header.status`.
pub fn kernel_request(op: KernelOp, request_id: u64, body: &[u8], reply: &mut [u8]) -> Result<ResultHeaderV1, Status> {
    let len = HEADER_LEN.checked_add(body.len()).ok_or(Status::Malformed)?;
    if len > MAX_REQUEST_LEN || reply.len() < HEADER_LEN || request_id == 0 { return Err(Status::Malformed); }
    let mut request = Scrub::<MAX_REQUEST_LEN>::new();
    RequestHeaderV1 { selector: op as u16, total_len: len as u32, request_id }.encode(&mut request[..HEADER_LEN])?;
    request[HEADER_LEN..len].copy_from_slice(body);
    match crate::syscall::sys_cell_session_v1(&request[..len], reply) {
        crate::syscall::SyscallResult::Ok(0) => {},
        _ => return Err(Status::Indeterminate),
    }
    let required = u32::from_le_bytes(reply[8..12].try_into().map_err(|_| Status::Malformed)?) as usize;
    let actual = if required > reply.len() { HEADER_LEN } else { required };
    if actual < HEADER_LEN { return Err(Status::Malformed); }
    let header = ResultHeaderV1::decode(&reply[..actual])?;
    if op != KernelOp::QueryRequest && (header.selector != op as u16 || header.request_id != request_id) { return Err(Status::Malformed); }
    Ok(header)
}

/// An accepted-or-not mutation whose outcome could not yet be observed. The next call
/// on the lane first resolves it by query (never by replay); an identical call returns
/// the original retained outcome.
#[derive(Clone, Copy)]
struct Pending { selector: u16, digest: u64 }
#[derive(Clone, Copy)]
struct Lane { issuer: u16, incarnation: Option<ServiceBinding>, operation: ObjectRefV1, next: u64, blocked: bool, pending: Option<Pending> }
// This is only correlation bookkeeping, never a resource/authority registry. Shared
// across handles/threads so constructing another SDK client cannot reuse ID 1.
static LANES: spin::Mutex<alloc::vec::Vec<Lane>> = spin::Mutex::new(alloc::vec::Vec::new());

/// Drop every correlation lane for `operation`. Call once the operation is closed,
/// acknowledged or otherwise retired so the bounded registry cannot be exhausted.
pub fn retire_operation(operation: ObjectRefV1) {
    LANES.lock().retain(|lane| lane.operation != operation);
}

/// Retained-outcome polling budget for a still-pending original: 20 queries, 50 ms
/// apart (about 5 scheduler ticks at the 10 ms x86 LAPIC tick).
const PENDING_QUERIES: u32 = 20;
const PENDING_WAIT_TICKS: usize = 5;

fn body_digest(body: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in body { hash ^= *byte as u64; hash = hash.wrapping_mul(0x0000_0100_0000_01b3); }
    hash
}

enum Answer { Outcome(ResultHeaderV1), Unknown, Pending, Failed(Status) }

/// Ask the owner for the retained outcome of `id`. A `WouldBlock` answer carrying the
/// query's own selector means the original is still pending: re-query with a bounded
/// delay rather than treating it as corruption.
fn query_outcome<F>(call: &mut F, query_selector: u16, selector: u16, operation: ObjectRefV1, id: u64, reply: &mut [u8]) -> Answer
where F: FnMut(u16,u64,&[u8],&mut[u8])->Result<ResultHeaderV1,Status> {
    let mut query = [0u8;24]; query[..16].copy_from_slice(&operation.to_bytes()); query[16..].copy_from_slice(&id.to_le_bytes());
    for attempt in 0..=PENDING_QUERIES {
        if attempt != 0 { let _ = crate::syscall::sys_set_timer(PENDING_WAIT_TICKS); }
        match call(query_selector, id, &query, reply) {
            Ok(h) if h.selector == selector && h.request_id == id => return Answer::Outcome(h),
            Ok(h) if h.status == Status::UnknownRequest => return Answer::Unknown,
            Ok(h) if h.status == Status::WouldBlock && h.selector == query_selector => {}
            Ok(_) => return Answer::Failed(Status::Indeterminate),
            // Queries are read-only: a transient transport failure is retried too.
            Err(Status::WouldBlock | Status::TimedOut | Status::Indeterminate) => {}
            Err(e) => return Answer::Failed(e),
        }
    }
    Answer::Pending
}

fn correlated<F>(issuer: u16, incarnation: Option<ServiceBinding>, operation: ObjectRefV1, selector: u16, mutating: bool, query_selector: u16, ack_selector: u16, body: &[u8], reply: &mut [u8], mut call: F) -> Result<ResultHeaderV1, Status>
where F: FnMut(u16,u64,&[u8],&mut[u8])->Result<ResultHeaderV1,Status> {
    let mut lanes = LANES.lock();
    let index = match lanes.iter().position(|l| l.issuer == issuer && l.incarnation == incarnation && l.operation == operation) {
        Some(i) => i,
        None => {
            if lanes.len() == 192 { return Err(Status::QuotaExceeded); }
            lanes.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
            lanes.push(Lane { issuer, incarnation, operation, next: 1, blocked: false, pending: None });
            lanes.len()-1
        }
    };
    let lane = &mut lanes[index];
    if lane.blocked { return Err(Status::Indeterminate); }
    let digest = body_digest(body);
    let mut resumed = None;
    if let Some(p) = lane.pending {
        // Resolve the earlier uncertain mutation by query before anything else.
        match query_outcome(&mut call, query_selector, p.selector, operation, lane.next, reply) {
            Answer::Outcome(h) => {
                lane.pending = None;
                if mutating && p.selector == selector && p.digest == digest {
                    // The caller is retrying the identical operation: hand back the
                    // original retained outcome; the request is never re-sent.
                    resumed = Some(h);
                } else {
                    // A different operation: retire the observed one, then proceed.
                    let mut ack = [0u8;24]; ack[..16].copy_from_slice(&operation.to_bytes()); ack[16..].copy_from_slice(&lane.next.to_le_bytes());
                    lane.next += 1;
                    let _ = call(ack_selector, lane.next - 1, &ack, &mut [0u8;HEADER_LEN]);
                }
            }
            // Still pending (the lane keeps the in-flight record and is not blocked).
            Answer::Pending => return Err(Status::Indeterminate),
            Answer::Unknown => { lane.blocked = true; return Err(Status::Indeterminate); }
            Answer::Failed(e) => { lane.blocked = true; return Err(e); }
        }
    }
    let id = lane.next;
    if id == u64::MAX { return Err(Status::QuotaExceeded); }
    let header = if let Some(h) = resumed { h } else {
        let result = call(selector,id,body,reply);
        match result {
            Ok(h) => h,
            Err(Status::Indeterminate) if mutating => {
                match query_outcome(&mut call, query_selector, selector, operation, id, reply) {
                    Answer::Outcome(h) => h,
                    Answer::Pending => {
                        lane.pending = Some(Pending { selector, digest });
                        return Err(Status::Indeterminate);
                    }
                    Answer::Unknown => { lane.blocked = true; return Err(Status::Indeterminate); }
                    Answer::Failed(e) => { lane.blocked = true; return Err(e); }
                }
            }
            Err(e) => {
                // Definite pre-dispatch rejection has no retained outcome or fence.
                if mutating && !matches!(e,Status::WouldBlock|Status::TimedOut|Status::Malformed|Status::Stale|Status::NotFound) {lane.blocked=true;}
                return Err(e);
            }
        }
    };
    if header.selector != selector || header.request_id != id { lane.blocked = true; return Err(Status::Malformed); }
    if mutating && header.status != Status::BufferTooSmall {
        if header.status!=Status::Ok{
            // Admission errors can precede outcome reservation. Ask the owner
            // whether this ID crossed its fence rather than guessing from an
            // error code (QuotaExceeded can occur on either side).
            let mut retained=[0u8;HEADER_LEN];
            match query_outcome(&mut call, query_selector, selector, operation, id, &mut retained) {
                Answer::Outcome(_) => {}
                Answer::Unknown => return Ok(header),
                Answer::Pending => {
                    lane.pending = Some(Pending { selector, digest });
                    return Err(Status::Indeterminate);
                }
                Answer::Failed(_) => { lane.blocked = true; return Err(Status::Indeterminate); }
            }
        }
        lane.next += 1;
        // A complete observed outcome can now be retired. ACK failure does not
        // turn the already observed operation into an uncertain one.
        let mut ack = [0u8;24]; ack[..16].copy_from_slice(&operation.to_bytes()); ack[16..].copy_from_slice(&id.to_le_bytes());
        let mut ack_reply = [0u8;HEADER_LEN];
        let _ = call(ack_selector,id,&ack,&mut ack_reply);
    }
    Ok(header)
}

/// Operation-scoped kernel request. No accepted mutation is automatically replayed.
pub fn kernel_call(op: KernelOp, operation: ObjectRefV1, mutating: bool, body: &[u8], reply: &mut [u8]) -> Result<ResultHeaderV1,Status> {
    let outcome=correlated(0,None,operation,op as u16,mutating,KernelOp::QueryRequest as u16,KernelOp::AcknowledgeRequests as u16,body,reply, |selector,id,body,reply| {
        let opcode = KernelOp::try_from(selector).map_err(|_| Status::UnknownOperation)?;
        if opcode == KernelOp::QueryRequest {
            // Query returns the original header, not a nested query result.
            return kernel_query(id,body,reply);
        }
        kernel_request(opcode,id,body,reply)
    });
    if op==KernelOp::CloseOperation&&outcome.as_ref().is_ok_and(|h|h.status==Status::Ok){retire_operation(operation);}
    outcome
}
fn kernel_query(id:u64, body:&[u8], reply:&mut[u8])->Result<ResultHeaderV1,Status> {
    // The raw helper enforces echo for normal requests; query's successful record
    // intentionally echoes the *original* selector instead.
    let mut request=[0u8;48];
    RequestHeaderV1{selector:KernelOp::QueryRequest as u16,total_len:48,request_id:id}.encode(&mut request[..24])?;
    if body.len()!=24 {return Err(Status::Malformed)} request[24..].copy_from_slice(body);
    // The kernel reserves its maximum retained-record size before executing a
    // query, so the scratch must be at least that large (kernel RESULT_MAX).
    let mut scratch=Scrub::<768>::new();
    if !matches!(crate::syscall::sys_cell_session_v1(&request,&mut scratch),crate::syscall::SyscallResult::Ok(0)) { return Err(Status::Indeterminate); }
    let n=u32::from_le_bytes(scratch[8..12].try_into().map_err(|_|Status::Malformed)?) as usize;
    if n>scratch.len()||n<HEADER_LEN{return Err(Status::Malformed)}
    let header=ResultHeaderV1::decode(&scratch[..n])?;
    if n<=reply.len(){reply[..n].copy_from_slice(&scratch[..n]);return Ok(header)}
    // A header-only probe (the post-status check) needs only the outcome header.
    if reply.len()==HEADER_LEN{reply.copy_from_slice(&scratch[..HEADER_LEN]);return Ok(header)}
    Err(Status::BufferTooSmall)
}

/// One correlated endpoint mutation; callers may advance only by acknowledged
/// positive progress. The deadline is absolute and reused across partial writes.
pub(crate) fn write_stream(operation: ObjectRefV1, endpoint: ObjectRefV1, bytes: &[u8], deadline_ns: u64) -> Result<usize, Status> {
    write_stream_with(operation, endpoint, bytes, deadline_ns, kernel_call)
}

fn write_stream_with(
    operation: ObjectRefV1,
    endpoint: ObjectRefV1,
    bytes: &[u8],
    deadline_ns: u64,
    call: impl FnOnce(KernelOp, ObjectRefV1, bool, &[u8], &mut [u8]) -> Result<ResultHeaderV1, Status>,
) -> Result<usize, Status> {
    use api::cell_session::{Reader, Writer, TRANSFER_MAX};

    if bytes.is_empty() || bytes.len() > TRANSFER_MAX || deadline_ns == 0 || deadline_ns == u64::MAX {
        return Err(Status::Malformed);
    }
    let mut body = Scrub::<{ 32 + TRANSFER_MAX }>::new();
    let mut writer = Writer::new(&mut body);
    writer.reference(endpoint)?;
    writer.u32(bytes.len() as u32)?;
    writer.u32(0)?;
    writer.u64(deadline_ns)?;
    writer.bytes(bytes)?;
    let length = writer.position();
    let mut reply = [0u8; HEADER_LEN + 8];
    let header = call(KernelOp::WriteStream, operation, true, &body[..length], &mut reply)?;
    if header.status != Status::Ok {
        return Err(header.status);
    }
    if header.total_len as usize != reply.len() {
        return Err(Status::Malformed);
    }
    let mut reader = Reader::new(&reply[HEADER_LEN..]);
    let actual = reader.u32()? as usize;
    if reader.u32()? != 0 || actual == 0 || actual > bytes.len() {
        return Err(Status::Malformed);
    }
    reader.finish()?;
    Ok(actual)
}

/// Resolve the current full issuer binding; a TID alone is never sufficient.
pub fn service_binding(service:u16)->Result<ServiceBinding,Status> {
    let mut record=[0u8;24]; crate::syscall::sys_lookup_service_bound(service,&mut record).ok_or(Status::NotFound)
}

/// Borrowed, exact-length authenticated SessionV1 / PreparedChildV1 RPC.
/// `service` is restricted to the two admitted issuers. Each IPC wait is <=100 ticks.
pub fn service_call(binding:ServiceBinding, service:u16, operation:ObjectRefV1, selector:u16, mutating:bool, body:&[u8], reply:&mut[u8])->Result<ResultHeaderV1,Status> {
    let (query,ack)=if service==api::syscall::service::INPUT {(19,18)} else if service==api::syscall::service::VFS {(5,4)} else {return Err(Status::Denied)};
    correlated(service,Some(binding),operation,selector,mutating,query,ack,body,reply,|selector,id,body,reply| {
        if service_binding(service)? != binding {return Err(Status::Stale)}
        let n=HEADER_LEN.checked_add(body.len()).ok_or(Status::Malformed)?;
        if n>MAX_SERVICE_RECORD_LEN {return Err(Status::Malformed)}
        let mut record=Scrub::<MAX_SERVICE_RECORD_LEN>::new();
        RequestHeaderV1{selector,total_len:n as u32,request_id:id}.encode(&mut record[..HEADER_LEN])?;
        record[HEADER_LEN..n].copy_from_slice(body);
        let mut send=Scrub::<{api::ipc::IPC_BUF_SIZE}>::new(); let mut recv=Scrub::<{api::ipc::IPC_BUF_SIZE}>::new();
        let inner = if service==api::syscall::service::INPUT {
            let req=api::ipc::InputRequest::SessionV1(&record[..n]);
            let response=crate::ipc::service_call_typed_bounded::<_,api::ipc::InputResponse<'_>>(binding.tid as usize,&req,&mut send,&mut recv,100).map_err(ipc_status)?;
            match response { api::ipc::InputResponse::SessionV1(bytes)=>bytes,_=>return Err(Status::Malformed) }
        } else {
            let req=api::ipc::VfsRequest::PreparedChildV1(&record[..n]);
            let response=crate::ipc::service_call_typed_bounded::<_,api::ipc::VfsResponse<'_>>(binding.tid as usize,&req,&mut send,&mut recv,100).map_err(ipc_status)?;
            match response { api::ipc::VfsResponse::PreparedChildV1(bytes)=>bytes,_=>return Err(Status::Malformed) }
        };
        if service_binding(service)? != binding {return Err(Status::Stale)}
        if inner.len()>reply.len(){return Err(Status::BufferTooSmall)}
        let h=ResultHeaderV1::decode(inner)?;
        if selector!=query && (h.selector!=selector || h.request_id!=id) {return Err(Status::Malformed)}
        reply[..inner.len()].copy_from_slice(inner); Ok(h)
    })
}

/// VFS → input: register one first-trust descriptor (`InputOp::RegisterTrustChallenge`);
/// returns the input-issued confirmation Ref the confirming client presents to
/// `BeginTrustConfirmation`. Input admits only the live registered VFS task. The request
/// lane is the single-use challenge Ref; it is kept only while the outcome is
/// indeterminate (retry the identical descriptor to resolve it).
pub fn register_trust_challenge(descriptor:&api::cell_session::trust::TrustChallengeV1<'_>)->Result<ObjectRefV1,Status> {
    use api::cell_session::trust::TrustChallengeV1;
    let owner=service_binding(api::syscall::service::INPUT)?;
    let mut body=[0u8;TrustChallengeV1::MAX_LEN]; let len=descriptor.encode(&mut body)?;
    let mut reply=[0u8;HEADER_LEN+16];
    let result=service_call(owner,api::syscall::service::INPUT,descriptor.challenge,api::cell_session::InputOp::RegisterTrustChallenge as u16,true,&body[..len],&mut reply);
    if result!=Err(Status::Indeterminate) { retire_operation(descriptor.challenge); }
    let h=result?;
    if h.status!=Status::Ok { return Err(h.status); }
    ObjectRefV1::from_bytes(&reply[HEADER_LEN..h.total_len as usize])
}

/// VFS → input: the decision of `confirmation` (`InputOp::CollectTrustDecision`), answered
/// only to the registering principal. `DECISION_PENDING` is not consumed; a final decision
/// is returned once (a later collect is `Stale`), and its lane (the confirmation Ref) is
/// then retired. Compare every binding, including `descriptor_sha256` against
/// [`trust_descriptor_sha256`] of the own record, before acting on it.
pub fn collect_trust_decision(confirmation:ObjectRefV1)->Result<api::cell_session::trust::TrustDecisionV1,Status> {
    use api::cell_session::trust::TrustDecisionV1;
    let owner=service_binding(api::syscall::service::INPUT)?;
    let mut reply=[0u8;HEADER_LEN+TrustDecisionV1::LEN];
    let result=service_call(owner,api::syscall::service::INPUT,confirmation,api::cell_session::InputOp::CollectTrustDecision as u16,true,&confirmation.to_bytes(),&mut reply);
    let decision=match result {
        Ok(h) if h.status==Status::Ok => TrustDecisionV1::decode(&reply[HEADER_LEN..h.total_len as usize]),
        Ok(h) => Err(h.status),
        Err(error) => Err(error),
    };
    if decision.as_ref().map_or_else(|e| *e!=Status::Indeterminate, TrustDecisionV1::is_final) { retire_operation(confirmation); }
    decision
}

/// SHA-256 of a descriptor's canonical bytes, as `TrustDecisionV1::descriptor_sha256` binds it.
pub fn trust_descriptor_sha256(descriptor:&api::cell_session::trust::TrustChallengeV1<'_>)->Result<[u8;32],Status> {
    let mut body=[0u8;api::cell_session::trust::TrustChallengeV1::MAX_LEN]; let len=descriptor.encode(&mut body)?;
    Ok(sha256(&body[..len]))
}
fn ipc_status(error:crate::ipc::IpcError)->Status {
    use crate::ipc::IpcError;
    match error {
        IpcError::Busy=>Status::WouldBlock, IpcError::PreDispatchTimeout=>Status::TimedOut,
        IpcError::Encode=>Status::Malformed,
        IpcError::Decode|IpcError::BufferTooSmall=>Status::Indeterminate,
        IpcError::Cancelled=>Status::Cancelled,
        _=>Status::Indeterminate,
    }
}
/// Receiver-monotonic nanoseconds, checked rather than architecture-guessed.
pub fn monotonic_ns()->Result<u64,Status> {
    let frequency=crate::syscall::sys_get_timer_freq().ok_or(Status::Unsupported)?;
    let ticks=crate::syscall::sys_get_time();
    let ns=(ticks as u128).checked_mul(1_000_000_000).ok_or(Status::Malformed)?/(frequency as u128);
    u64::try_from(ns).map_err(|_|Status::Malformed)
}

/// Overwrite `bytes` with zeros using volatile stores, so the optimizer cannot treat the
/// write as a dead store before the memory is released or reused. Secret-bearing buffers
/// (typed text, raw stdin bytes, paste staging, retained request bodies) are wiped with
/// this, never with a plain `fill(0)`/`clear()`.
pub fn zeroize(bytes:&mut [u8]) { zeroize_raw(bytes.as_mut_ptr(),bytes.len()); }

fn zeroize_raw(pointer:*mut u8, length:usize) {
    for offset in 0..length {
        // SAFETY: callers pass a pointer valid for `length` exclusive byte writes.
        unsafe { core::ptr::write_volatile(pointer.add(offset),0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Zero the vector's whole allocation (including spare capacity that earlier
/// contents may have occupied), then truncate it. The allocation is retained.
pub fn zeroize_vec(bytes:&mut alloc::vec::Vec<u8>) {
    zeroize_raw(bytes.as_mut_ptr(),bytes.capacity());
    bytes.clear();
}

/// Zero a string's whole allocation, then truncate it (still valid UTF-8: empty).
pub fn zeroize_string(text:&mut alloc::string::String) {
    // SAFETY: the vector is cleared before the borrow ends, so the string stays valid UTF-8.
    zeroize_vec(unsafe { text.as_mut_vec() });
}

/// Zero every byte of a ring (both halves), then empty it.
pub fn zeroize_deque(ring:&mut alloc::collections::VecDeque<u8>) {
    let (first,second)=ring.as_mut_slices();
    zeroize(first); zeroize(second);
    ring.clear();
}

/// Zero and remove the oldest `count` bytes of a ring, so a consumed prefix never
/// lingers in a vacated slot.
pub fn zeroize_deque_front(ring:&mut alloc::collections::VecDeque<u8>, count:usize) {
    let count=count.min(ring.len());
    let (first,second)=ring.as_mut_slices();
    let head=count.min(first.len());
    zeroize(&mut first[..head]); zeroize(&mut second[..count-head]);
    ring.drain(..count);
}

/// Fixed-size stack buffer that is wiped (volatile) when it goes out of scope, on every
/// exit path including `?` returns. Dereferences to a byte slice.
pub struct Scrub<const N:usize>([u8;N]);
impl<const N:usize> Scrub<N> { pub const fn new()->Self { Self([0;N]) } }
impl<const N:usize> Default for Scrub<N> { fn default()->Self { Self::new() } }
impl<const N:usize> core::ops::Deref for Scrub<N> { type Target=[u8]; fn deref(&self)->&[u8] { &self.0 } }
impl<const N:usize> core::ops::DerefMut for Scrub<N> { fn deref_mut(&mut self)->&mut [u8] { &mut self.0 } }
impl<const N:usize> Drop for Scrub<N> { fn drop(&mut self) { zeroize(&mut self.0); } }

/// SHA-256 (FIPS 180-4). Used only to bind a retained request identity without keeping the
/// request's plaintext (see the input service's sealed outcomes).
pub fn sha256(data:&[u8])->[u8;32] {
    const K:[u32;64]=[
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
    ];
    let mut state:[u32;8]=[0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    fn compress(state:&mut [u32;8], block:&[u8])  {
        let mut w=[0u32;64];
        for (i,word) in block.chunks_exact(4).enumerate() { w[i]=u32::from_be_bytes([word[0],word[1],word[2],word[3]]); }
        for i in 16..64 {
            let s0=w[i-15].rotate_right(7)^w[i-15].rotate_right(18)^(w[i-15]>>3);
            let s1=w[i-2].rotate_right(17)^w[i-2].rotate_right(19)^(w[i-2]>>10);
            w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let [mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut h]=*state;
        for i in 0..64 {
            let s1=e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25);
            let ch=(e&f)^(!e&g);
            let t1=h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0=a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22);
            let maj=(a&b)^(a&c)^(b&c);
            let t2=s0.wrapping_add(maj);
            h=g; g=f; f=e; e=d.wrapping_add(t1); d=c; c=b; b=a; a=t1.wrapping_add(t2);
        }
        for (slot,value) in state.iter_mut().zip([a,b,c,d,e,f,g,h]) { *slot=slot.wrapping_add(value); }
    }
    let mut blocks=data.chunks_exact(64);
    for block in &mut blocks { compress(&mut state,block); }
    // Final one or two blocks hold the remainder, 0x80, zero padding and the bit length.
    let remainder=blocks.remainder();
    let mut tail=Scrub::<128>::new();
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()]=0x80;
    let total=if remainder.len()<56 {64} else {128};
    tail[total-8..total].copy_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in tail[..total].chunks_exact(64) { compress(&mut state,block); }
    let mut digest=[0u8;32];
    for (chunk,word) in digest.chunks_exact_mut(4).zip(state) { chunk.copy_from_slice(&word.to_be_bytes()); }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_stream_encodes_exact_correlated_mutation_and_partial_progress() {
        let operation = ObjectRefV1 { id: 91, generation: 3 };
        let endpoint = ObjectRefV1 { id: 92, generation: 4 };
        let bytes = [0xFF; api::cell_session::TRANSFER_MAX];
        let result = write_stream_with(operation, endpoint, &bytes, 123456, |op, lineage, mutating, body, reply| {
            assert_eq!(op, KernelOp::WriteStream);
            assert_eq!(lineage, operation);
            assert!(mutating);
            assert_eq!(body.len(), 32 + bytes.len());
            let mut reader = api::cell_session::Reader::new(body);
            assert_eq!(reader.reference()?, endpoint);
            assert_eq!(reader.u32()? as usize, bytes.len());
            assert_eq!(reader.u32()?, 0);
            assert_eq!(reader.u64()?, 123456);
            assert_eq!(reader.take(bytes.len())?, &bytes);
            reader.finish()?;
            reply[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&37u32.to_le_bytes());
            Ok(ResultHeaderV1 {
                selector: op as u16, status: Status::Ok,
                total_len: (HEADER_LEN + 8) as u32, request_id: 1,
            })
        });
        assert_eq!(result, Ok(37));
    }

    #[test]
    fn write_stream_rejects_invalid_inputs_before_calling_owner() {
        let endpoint = ObjectRefV1 { id: 12, generation: 2 };
        let oversized = [0; api::cell_session::TRANSFER_MAX + 1];
        for (bytes, deadline) in [(&b""[..], 1), (&oversized[..], 1), (&b"x"[..], 0), (&b"x"[..], u64::MAX)] {
            assert_eq!(write_stream_with(endpoint, endpoint, bytes, deadline, |_, _, _, _, _| {
                panic!("invalid write reached owner");
            }), Err(Status::Malformed));
        }
    }

    #[test]
    fn write_stream_requires_exact_reply_and_positive_bounded_progress() {
        let endpoint = ObjectRefV1 { id: 12, generation: 2 };
        for (length, actual, reserved) in [
            (24, 1, 0), (28, 1, 0), (33, 1, 0), (32, 0, 0),
            (32, 4, 0), (32, 1, 1),
        ] {
            assert_eq!(write_stream_with(endpoint, endpoint, b"abc", 1, |op, _, _, _, reply| {
                reply[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&(actual as u32).to_le_bytes());
                reply[HEADER_LEN + 4..].copy_from_slice(&(reserved as u32).to_le_bytes());
                Ok(ResultHeaderV1 {
                    selector: op as u16, status: Status::Ok,
                    total_len: length, request_id: 1,
                })
            }), Err(Status::Malformed));
        }
    }

    #[test]
    fn write_stream_preserves_owner_and_transport_errors() {
        let endpoint = ObjectRefV1 { id: 12, generation: 2 };
        for status in [Status::WouldBlock, Status::TimedOut, Status::Cancelled,
            Status::Revoked, Status::BrokenPipe, Status::WrongOwner,
            Status::WrongDirection, Status::Stale, Status::Indeterminate]
        {
            assert_eq!(write_stream_with(endpoint, endpoint, b"abc", 1, |op, _, _, _, _| {
                Ok(ResultHeaderV1 {
                    selector: op as u16, status, total_len: HEADER_LEN as u32, request_id: 1,
                })
            }), Err(status));
            assert_eq!(write_stream_with(endpoint, endpoint, b"abc", 1, |_, _, _, _, _| {
                Err(status)
            }), Err(status));
        }
    }

    #[test]
    fn zeroize_overwrites_every_byte() {
        let mut bytes=[0xA5u8;37]; zeroize(&mut bytes);
        assert!(bytes.iter().all(|b|*b==0));
        zeroize(&mut []);
    }

    #[test]
    fn zeroize_vec_wipes_spare_capacity_and_truncates() {
        let mut vector=alloc::vec::Vec::with_capacity(32);
        vector.extend_from_slice(b"hunter2 and more secret bytes!!");
        vector.truncate(7); // the tail now lives only in spare capacity
        let capacity=vector.capacity(); let pointer=vector.as_ptr();
        zeroize_vec(&mut vector);
        assert!(vector.is_empty()); assert_eq!(vector.capacity(),capacity);
        // SAFETY: the allocation is still owned by `vector`; every byte was just written.
        let whole=unsafe { core::slice::from_raw_parts(pointer,capacity) };
        assert!(whole.iter().all(|b|*b==0));
    }

    #[test]
    fn zeroize_string_wipes_text_and_stays_valid() {
        let mut text=alloc::string::String::from("correct horse battery staple");
        let capacity=text.capacity(); let pointer=text.as_ptr();
        zeroize_string(&mut text);
        assert!(text.is_empty()); assert_eq!(text.as_str(),"");
        // SAFETY: allocation retained by `text`; every byte was just written.
        assert!(unsafe { core::slice::from_raw_parts(pointer,capacity) }.iter().all(|b|*b==0));
        text.push_str("reusable");
        assert_eq!(text,"reusable");
    }

    #[test]
    fn zeroize_deque_wipes_both_halves() {
        let mut ring=alloc::collections::VecDeque::with_capacity(8);
        for byte in 0..8u8 { ring.push_back(0xF0|byte); }
        for _ in 0..5 { ring.pop_front(); }
        for byte in 0..4u8 { ring.push_back(0xE0|byte); } // wraps when capacity is 8: two slices
        let (first,second)=ring.as_slices();
        let halves=[(first.as_ptr(),first.len()),(second.as_ptr(),second.len())];
        zeroize_deque(&mut ring);
        assert!(ring.is_empty());
        for (pointer,length) in halves {
            // SAFETY: the allocation is still owned by `ring`; these slots held live bytes
            // until the wipe wrote zeros to them.
            assert!(unsafe { core::slice::from_raw_parts(pointer,length) }.iter().all(|b|*b==0));
        }
    }

    #[test]
    fn zeroize_deque_front_removes_and_wipes_only_the_prefix() {
        let mut ring=alloc::collections::VecDeque::with_capacity(8);
        for byte in 1..=6u8 { ring.push_back(byte); }
        for _ in 0..4 { ring.pop_front(); }
        for byte in 7..=10u8 { ring.push_back(byte); } // wrapped: [5,6,7,8,9,10]
        let pointer=ring.as_slices().0.as_ptr();
        zeroize_deque_front(&mut ring,3);
        assert_eq!(ring.iter().copied().collect::<alloc::vec::Vec<u8>>(),[8,9,10]);
        // SAFETY: still-owned allocation; the vacated head slots were written to zero.
        let head=unsafe { core::slice::from_raw_parts(pointer,2) };
        assert!(head.iter().all(|b|*b==0));
        zeroize_deque_front(&mut ring,99); // over-long count is clamped
        assert!(ring.is_empty());
    }

    #[test]
    fn scrub_wipes_on_drop_and_derefs_as_slice() {
        let mut slot=core::mem::MaybeUninit::new(Scrub::<24>::new());
        // SAFETY: `slot` was just initialized and is only dropped once, below.
        let buffer=unsafe { slot.assume_init_mut() };
        buffer[..6].copy_from_slice(b"secret"); assert_eq!(buffer.len(),24); assert_eq!(&buffer[..6],b"secret");
        // SAFETY: drop_in_place runs `Drop` exactly once; the storage stays valid (u8 array, no heap).
        unsafe { core::ptr::drop_in_place(slot.as_mut_ptr()) };
        let bytes=unsafe { &*(slot.as_ptr() as *const [u8;24]) };
        assert!(bytes.iter().all(|b|*b==0));
    }

    #[test]
    fn sha256_matches_fips_vectors() {
        fn hex(digest:[u8;32])->alloc::string::String { digest.iter().map(|b|alloc::format!("{b:02x}")).collect() }
        assert_eq!(hex(sha256(b"")),"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(sha256(b"abc")),"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        // 55/56/64-byte inputs straddle the padding boundary.
        assert_eq!(hex(sha256(&[b'a';55])),"9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318");
        assert_eq!(hex(sha256(&[b'a';56])),"b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a");
        assert_eq!(hex(sha256(&[b'a';64])),"ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb");
    }
}
