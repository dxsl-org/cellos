// SPDX-License-Identifier: MPL-2.0
//! SessionV1 input authority. All identities enter through the attested receive
//! trailer; kernel endpoints, prepared children and publication remain kernel owned.
use alloc::{vec::Vec, collections::VecDeque};
use api::cell_session::{ChildRefV1, KernelOp, ObjectRefV1 as Ref, PrincipalV1 as Principal, Reader, RequestHeaderV1, ResultHeaderV1, Status, Writer, HEADER_LEN, MAX_SERVICE_RECORD_LEN};
use api::cell_session::{frontend as fe, FRONTEND_SEAL_LEN, GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND};
use api::{caller_identity::CallerIdentity, service_binding::ServiceBinding};
use sys::{sys_get_time_ms, sys_ipc_current, sys_ipc_reply, sys_lookup_service, sys_lookup_service_bound, sys_try_send};

/// Kernel/IPC seam. Production uses the real syscalls; host tests run the same
/// dispatch against an in-process kernel model (`session_tests.rs`).
#[cfg(not(test))]
mod sys {
    pub use ostd::syscall::{sys_get_time_ms, sys_ipc_current, sys_ipc_reply, sys_lookup_service, sys_lookup_service_bound, sys_try_send};
    pub use ostd::session::{kernel_call, kernel_request, monotonic_ns};
}
#[cfg(test)]
use frontend_tests::fake as sys;
#[cfg(test)]
#[path = "session_tests.rs"]
mod frontend_tests;
#[path = "session_trust.rs"]
mod trust;
use ostd::session::{zeroize_deque, zeroize_deque_front, zeroize_vec, Scrub};

const INPUT_LIMIT: usize = 256;
const CONTROL_LIMIT: usize = 64;
const RAW_LIMIT: usize = 65548;
const MODE_MASK: u32 = 1 | 2 | 16;
const ZERO: Ref = Ref { id: 0, generation: 0 };
fn now() -> u64 { sys_get_time_ms().unwrap_or(0) }
/// A registered compositor owns keyboard routing and focus. While one exists no session
/// may bind or hold the physical serial/keyboard owner role, so a session can never gate
/// legacy `SetFocus` or global key capture.
///
/// Residual limitation (headless images): with no compositor registered, the first
/// attached session whose frontend is its own attested controller still binds the physical
/// serial owner, because the kernel offers no console-principal attestation to distinguish
/// the designated serial shell from another cell. Images that run untrusted cells without a
/// compositor therefore must not expose SessionV1 to them.
fn compositor_registered() -> bool { sys_lookup_service(api::syscall::service::COMPOSITOR).is_some() }
fn principal(c: CallerIdentity) -> Principal { Principal { cell_id: c.cell_id, generation: c.generation } }
fn reserved(v: u32) -> Result<(), Status> { if v == 0 { Ok(()) } else { Err(Status::Malformed) } }
fn dimensions(rows: u16, cols: u16) -> Result<(), Status> { if rows == 0 || rows > 256 || cols == 0 || cols > 512 { Err(Status::Malformed) } else { Ok(()) } }
fn reserve<T>(v: &mut Vec<T>, count: usize) -> Result<(), Status> { v.try_reserve(count).map_err(|_| Status::QuotaExceeded) }
/// Receiver-monotonic nanoseconds on the same base the SDK uses for absolute deadlines.
fn now_ns() -> u64 { sys::monotonic_ns().unwrap_or(0) }

struct Event { sequence: u64, bytes: Vec<u8>, sent: usize }
impl Event { fn wipe(&mut self) { zeroize_vec(&mut self.bytes); } }
// Typed text rides in event bodies: wipe on every release (ack, queue clear, close, drop).
impl Drop for Event { fn drop(&mut self) { self.wipe(); } }
struct Mode { reference: Ref, holder: Principal, epoch: u64, mask: u32, value: u32 }
struct Attachment { reference: Ref, frontend: Principal }
struct Foreground {
    lease: Ref, receipt: Ref, child: ChildRefV1, operation: Ref, expected: u64, next: u64,
    endpoints: [Ref; 4], writers: [Ref; 2], committed: bool, restored: bool, acknowledged: bool, cancelled: bool,
}
struct Paste { reference: Ref, frontend: Principal, attachment: Ref, epoch: u64, expires: u64, total: usize, bytes: Vec<u8> }
impl Paste { fn wipe(&mut self) { zeroize_vec(&mut self.bytes); } }
impl Drop for Paste { fn drop(&mut self) { self.wipe(); } }
/// Physical-UART paste staging: wiped whenever it is dropped, taken-and-dropped or replaced.
struct Secret(Vec<u8>);
impl Secret { fn wipe(&mut self) { zeroize_vec(&mut self.0); } }
impl core::ops::Deref for Secret { type Target = Vec<u8>; fn deref(&self) -> &Vec<u8> { &self.0 } }
impl core::ops::DerefMut for Secret { fn deref_mut(&mut self) -> &mut Vec<u8> { &mut self.0 } }
impl Drop for Secret { fn drop(&mut self) { self.wipe(); } }
struct PendingPoll { outcome: usize, token: usize, epoch: u64, lease: Ref, cursor: u64, maximum: usize, expires: u64, deadline: u64 }
struct Session {
    reference: Ref, bootstrap: Ref, controller: Principal, operation: Ref,
    baseline: [Ref; 4], baseline_writers: [Ref; 2], endpoints: [Ref; 4], writers: [Ref; 2],
    baseline_lease: Ref, lease: Ref, reader: Principal, epoch: u64, sequence: u64,
    rows: u16, cols: u16, attachment: Option<Attachment>, foreground: Option<Foreground>,
    modes: Vec<Mode>, mode: u32, typed: VecDeque<Event>, controls: VecDeque<Event>,
    baseline_modes: Vec<Mode>,
    raw: VecDeque<u8>, paste: Option<Paste>, poll: Option<PendingPoll>,
    draining: bool, eof: bool, baseline_eof: bool, frozen: bool, closed: bool,
    input_failure: Option<Status>,
    decoder: api::services::session::SerialDecoder,
    physical_paste: Option<Secret>,
    physical_paste_expires: u64,
    physical_paste_complete: bool,
    physical_paste_discard: bool,
    delivered_cursor: u64,
    prepare_deadline: u64,
    /// Frontend kind (`fe::FRONTEND_KIND_*`), reported by QueryCapabilities.
    kind: u8,
    /// Sel-25 sessions only: the frontend holder that owns `resume`, and the registered
    /// operation of its current (or last) frontend: holder liveness and request lane.
    holder: Option<Principal>,
    holder_operation: Ref,
    /// The frontend record currently attached (sel 25/28).
    frontend: Option<Ref>,
    /// Holder-bound re-attach capability; ZERO once revoked.
    resume: Ref,
    /// The frontend detached: the reader keeps its lease and input waits (no EOF).
    detached: bool,
    /// The holder died: no frontend can return; the reader gets Lost(4), then EOF.
    orphaned: bool,
    /// Owner-run first-trust capture (sel 30): while set, the session's typed, raw and
    /// paste input is consumed by input and never routed to the reader.
    capture: Option<trust::Capture>,
    /// Owner notices (`TrustPrompt`/`TrustPromptEnd`) for the controller's own control ring.
    notices: VecDeque<Event>,
}
// The raw ring holds unchanged application stdin bytes (possibly secrets): never leave them behind.
impl Drop for Session { fn drop(&mut self) { zeroize_deque(&mut self.raw); } }
/// An input-issued frontend capability (annex S5-3c §3). Only `holder` may use it.
struct Frontend {
    reference: Ref, holder: Principal, operation: Ref, kind: u8, rows: u16, cols: u16,
    state: u32, child: Option<ChildRefV1>, receipt: Ref, session: Ref,
}
struct Outcome { caller: Principal, operation: Ref, id: u64, selector: u16, request: Vec<u8>, sealed: bool, result: Vec<u8>, observed: bool, pending: bool, poll_session: Ref, poll_epoch: u64, cursor: u64 }
struct Fence { caller: Principal, operation: Ref, next: u64, acknowledged: u64 }
/// Selectors whose request body can carry typed or raw secrets (SubmitEvents, WritePaste, SubmitRaw).
/// Their results are only a sequence/count, so nothing beyond the status is ever replayable.
fn secret_bearing(selector: u16) -> bool { matches!(selector, 13 | 15 | 20) }
fn wipe(bytes: &mut Vec<u8>) { zeroize_vec(bytes); *bytes = Vec::new(); }
/// The record an outcome retains for `request`. A secret-bearing request is sealed at the
/// moment it is consumed: only the SHA-256 of the exact record is kept, never its plaintext,
/// not even while the request executes (the body is read from the receive buffer, which the
/// caller wipes). The digest still lets an identical re-send resolve to the retained (or
/// pending) outcome and a changed body under the same ID be refused (annex: Malformed).
fn retained(selector: u16, request: &[u8]) -> Result<(Vec<u8>, bool), Status> {
    let sealed = secret_bearing(selector);
    let mut copy = Vec::new();
    if sealed {
        copy.try_reserve_exact(32).map_err(|_| Status::QuotaExceeded)?;
        copy.extend_from_slice(&ostd::session::sha256(request));
    } else {
        reserve(&mut copy, request.len())?; copy.extend_from_slice(request);
    }
    Ok((copy, sealed))
}
impl Outcome {
    /// Wipe and release the retained request and result bodies (the fence stays elsewhere).
    fn scrub(&mut self) { wipe(&mut self.request); wipe(&mut self.result); self.sealed = false; }
    /// Does `bytes` exactly equal the record this outcome retained? `digest` caches the hash of
    /// `bytes` across candidates. Selector and request ID are header fields (not secret).
    fn matches(&self, bytes: &[u8], digest: &mut Option<[u8; 32]>) -> bool {
        if self.request.is_empty() { return false; }
        if !self.sealed { return self.request == bytes; }
        if bytes.len() < HEADER_LEN || bytes[4..6] != self.selector.to_le_bytes() || bytes[16..24] != self.id.to_le_bytes() { return false; }
        let digest = digest.get_or_insert_with(|| ostd::session::sha256(bytes));
        self.request[..] == digest[..]
    }
}
impl Drop for Outcome { fn drop(&mut self) { self.scrub(); } }

pub struct Sessions { sessions: Vec<Session>, frontends: Vec<Frontend>, outcomes: Vec<Outcome>, fences: Vec<Fence>, serial: Option<Ref>, issuer: Option<ServiceBinding>, next_ref: u64, kernel_operation: Ref, challenges: Vec<trust::Challenge> }
impl Sessions {
    pub fn new() -> Self { Self { sessions: Vec::new(), frontends: Vec::new(), outcomes: Vec::new(), fences: Vec::new(), serial: None, issuer: None, next_ref: 1, kernel_operation: ZERO, challenges: Vec::new() } }
    fn reference(&mut self) -> Result<Ref, Status> { let id = self.next_ref; self.next_ref = id.checked_add(1).ok_or(Status::QuotaExceeded)?; Ok(Ref { id, generation: 1 }) }
    fn binding(&mut self) -> Result<ServiceBinding, Status> {
        let mut bytes = [0; 24];
        let binding = sys_lookup_service_bound(api::syscall::service::INPUT as u16, &mut bytes).ok_or(Status::Revoked)?;
        if let Some(old) = self.issuer { if old != binding { return Err(Status::Revoked); } } else { self.issuer = Some(binding); }
        Ok(binding)
    }
    fn kernel(&mut self, op: KernelOp, body: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
        self.binding()?;
        let query = matches!(op, KernelOp::InspectPreparedGrant | KernelOp::InspectEndpoint | KernelOp::QueryReceiptChild | KernelOp::QueryOperation)
            || (matches!(op, KernelOp::ResolveBindReceipt | KernelOp::ResolveBootstrap)
                && body.get(16..20).is_some_and(|v| v == [0; 4]));
        let header = if query {
            // Nonmutating selectors retain no outcome and consume no request ID; they must
            // not mint SDK correlation lanes (bounded, never retired for foreign operations).
            sys::kernel_request(op, 1, body, reply)?
        } else {
            sys::kernel_call(op, self.kernel_operation, true, body, reply)?
        };
        if header.status != Status::Ok { return Err(header.status); }
        Ok(header.total_len as usize)
    }
    fn resolve_bootstrap(&mut self, bootstrap: Ref, action: u32, session: Ref) -> Result<(u32, Principal, Ref, [Ref; 4], [Ref; 2], [Ref; 2]), Status> {
        let mut body = [0; 40]; let mut w = Writer::new(&mut body); w.reference(bootstrap)?; w.u32(action)?; w.u32(0)?; w.reference(session)?;
        let mut reply = [0; 224]; let len = self.kernel(KernelOp::ResolveBootstrap, &body, &mut reply)?;
        let mut r = Reader::new(&reply[HEADER_LEN..len]); let state = r.u32()?; reserved(r.u32()?)?; let controller = r.principal()?; let operation = r.reference()?;
        let endpoints = [r.reference()?, r.reference()?, r.reference()?, r.reference()?]; let outputs = [r.reference()?, r.reference()?]; let writers = [r.reference()?, r.reference()?]; r.finish()?;
        Ok((state, controller, operation, endpoints, outputs, writers))
    }
    fn index(&self, reference: Ref) -> Result<usize, Status> { self.sessions.iter().position(|s| s.reference == reference && !s.closed).ok_or(Status::Stale) }
    fn controller(&self, index: usize, p: Principal) -> Result<(), Status> { if self.sessions[index].controller == p { Ok(()) } else { Err(Status::Denied) } }
    fn reader(&self, index: usize, p: Principal, lease: Ref) -> Result<(), Status> { let s = &self.sessions[index]; if s.frozen { return Err(Status::WouldBlock); } if (s.attachment.is_none() && !s.detached) || s.reader != p || s.lease != lease { Err(Status::Denied) } else { Ok(()) } }
    fn frontend(&self, index: usize, p: Principal, attachment: Ref, epoch: u64) -> Result<(), Status> {
        let s = &self.sessions[index]; if s.frozen { return Err(Status::WouldBlock); } if s.epoch != epoch { return Err(Status::WrongEpoch); }
        match s.attachment.as_ref() {
            Some(a) if a.reference == attachment && a.frontend == p => Ok(()),
            // A sel-25 holder presenting a detached or superseded attachment holds a stale selector.
            _ if s.holder == Some(p) => Err(Status::Stale),
            _ => Err(Status::Denied),
        }
    }
    /// Sel-25 frontends cannot observe the controller's foreground churn: they send
    /// epoch 0 and act on the current epoch. Kind-0 self-attachments keep the exact rule.
    fn wire_epoch(&self, index: usize, epoch: u64) -> Result<u64, Status> {
        let s = &self.sessions[index];
        match (s.holder.is_some(), epoch) { (false, epoch) => Ok(epoch), (true, 0) => Ok(s.epoch), (true, _) => Err(Status::WrongEpoch) }
    }
    /// The request lane of a sel-25 frontend holder is its frontend's registered operation.
    fn holder_lane(s: &Session, caller: Principal) -> Option<Ref> { (s.holder == Some(caller) && s.controller != caller && !s.orphaned).then_some(s.holder_operation) }
    /// Does a live sel-25 holder or frontend record still use `operation`?
    fn frontend_lane(&self, operation: Ref) -> bool {
        self.sessions.iter().any(|s| !s.closed && s.holder.is_some() && !s.orphaned && s.holder_operation == operation)
            || self.frontends.iter().any(|f| f.operation == operation)
    }
    fn frontend_index(&self, reference: Ref) -> Result<usize, Status> { self.frontends.iter().position(|f| f.reference == reference).ok_or(Status::Stale) }
    fn owned_frontend(&self, reference: Ref, p: Principal) -> Result<usize, Status> { let i = self.frontend_index(reference)?; if self.frontends[i].holder == p { Ok(i) } else { Err(Status::Denied) } }
    fn epoch(&self, index: usize, epoch: u64) -> Result<(), Status> { if self.sessions[index].epoch == epoch { Ok(()) } else { Err(Status::WrongEpoch) } }
    fn operation_for(&self, selector: u16, body: &[u8], caller: Principal) -> Result<Ref, Status> {
        let mut r = Reader::new(body); let reference = r.reference()?;
        // Trust lanes: Register is fenced on its challenge Ref, Collect on its confirmation Ref.
        if matches!(selector, 1 | 18 | 19 | 23 | 25 | 29 | 31) { return Ok(reference); }
        if matches!(selector, 24 | 26..=28) { return self.frontends.iter().find(|f| f.reference == reference).map(|f| f.operation).ok_or(Status::Stale); }
        if (15..=17).contains(&selector) {
            let s = self.sessions.iter().find(|s| s.paste.as_ref().is_some_and(|p| p.reference == reference)).ok_or(Status::Stale)?;
            return Ok(Self::holder_lane(s, caller).unwrap_or(s.operation));
        }
        let s = &self.sessions[self.index(reference)?];
        if let Some(lane) = Self::holder_lane(s, caller) { return Ok(lane); }
        Ok(s.foreground.as_ref().filter(|f| f.child.principal == caller && !f.restored).map(|f| f.operation).unwrap_or(s.operation))
    }
    fn participant(&self, index: usize, p: Principal) -> bool {
        let s = &self.sessions[index];
        s.controller == p || s.reader == p || s.holder == Some(p)
            || s.attachment.as_ref().is_some_and(|a| a.frontend == p)
            || s.foreground.as_ref().is_some_and(|f| f.child.principal == p)
            || s.modes.iter().chain(s.baseline_modes.iter()).any(|m| m.holder == p)
    }
    /// Only principals already tied to the target session may allocate retained
    /// outcome/fence capacity; a guessed session reference confers no bookkeeping budget.
    fn admit(&mut self, selector: u16, body: &[u8], p: Principal, sender: usize) -> Result<(), Status> {
        let mut r = Reader::new(body);
        match selector {
            29 | 31 => self.admit_trust(selector, body, p, sender),
            1 => {
                let operation = r.reference()?; let bootstrap = r.reference()?;
                self.kernel_operation = operation;
                let (state, controller, recorded, ..) = self.resolve_bootstrap(bootstrap, 0, ZERO)?;
                if state > 1 || controller != p || recorded != operation { return Err(Status::Denied); }
                Ok(())
            }
            15..=17 => {
                let reference = r.reference()?;
                let i = self.sessions.iter().position(|s| !s.closed && s.paste.as_ref().is_some_and(|x| x.reference == reference)).ok_or(Status::Stale)?;
                if self.participant(i, p) { Ok(()) } else { Err(Status::Denied) }
            }
            // Any attested Cell may register a frontend on a live operation of its own lane.
            23 => { let operation = r.reference()?; self.kernel_operation = operation; if self.query_operation(operation)? != 0 { return Err(Status::Cancelled); } Ok(()) }
            24 | 26..=28 => { self.owned_frontend(r.reference()?, p)?; Ok(()) }
            // Only the exact child bound by sel 24 may create the frontend's session.
            25 => {
                r.reference()?; r.reference()?; let f = &self.frontends[self.frontend_index(r.reference()?)?];
                if f.child.is_some_and(|c| c.principal == p) { Ok(()) } else { Err(Status::Denied) }
            }
            18 | 19 => Ok(()),
            _ => { let i = self.index(r.reference()?)?; if self.participant(i, p) { Ok(()) } else { Err(Status::Denied) } }
        }
    }
    /// Drop request fences whose operation no live session or live foreground child uses
    /// and that retain no outcome. A dead principal/operation cannot present an old ID.
    fn prune(&mut self) {
        let sessions = &self.sessions; let outcomes = &self.outcomes; let frontends = &self.frontends; let challenges = &self.challenges;
        self.fences.retain(|f| sessions.iter().any(|s| !s.closed && (s.operation == f.operation
                || (s.holder.is_some() && !s.orphaned && s.holder_operation == f.operation)
                || s.foreground.as_ref().is_some_and(|g| !g.restored && g.operation == f.operation)))
            || frontends.iter().any(|x| x.operation == f.operation)
            || trust::challenge_lane(challenges, f.caller, f.operation)
            || outcomes.iter().any(|o| !o.request.is_empty() && o.caller == f.caller && o.operation == f.operation));
    }
    fn drop_outcomes(&mut self, operation: Ref) {
        for o in self.outcomes.iter_mut().filter(|o| o.operation == operation) { o.scrub(); o.pending = false; }
    }
    /// An explicitly closed session's retained outcomes die with it, except the outcome of the
    /// close request itself (kept so its replay and acknowledgement still resolve). Dropped
    /// outcomes are marked observed so they can never block that acknowledgement.
    fn release_closed(&mut self, operations: &[Option<Ref>], keep: usize) {
        for operation in operations.iter().flatten().copied() {
            if self.frontend_lane(operation) || self.sessions.iter().any(|s| !s.closed && (s.operation == operation || s.foreground.as_ref().is_some_and(|f| !(f.restored && f.acknowledged) && f.operation == operation))) { continue; }
            for (n, o) in self.outcomes.iter_mut().enumerate() {
                if n == keep || o.operation != operation { continue; }
                o.scrub(); o.pending = false; o.observed = true;
            }
        }
        self.prune();
    }
    /// Retire the SDK correlation lane of an operation this service no longer calls on, unless
    /// another live session or unfinished foreground still uses it.
    fn retire_lane(&self, operation: Ref) {
        let busy = self.frontend_lane(operation) || self.sessions.iter().any(|s| !s.closed && (s.operation == operation
            || s.foreground.as_ref().is_some_and(|f| !(f.restored && f.acknowledged) && f.operation == operation)));
        if !busy { ostd::session::retire_operation(operation); }
    }
    /// The foreground child is terminal and restored: its retained outcomes/fences die with it.
    fn retire_foreground(&mut self, i: usize) {
        let Some(f) = self.sessions[i].foreground.as_ref() else { return; };
        let (principal, operation) = (f.child.principal, f.operation);
        for o in self.outcomes.iter_mut().filter(|o| o.caller == principal && o.operation == operation) {
            o.scrub(); o.pending = false;
        }
        self.prune();
        self.retire_lane(operation);
    }
    fn record(&mut self, header: RequestHeaderV1, request: &[u8], p: Principal, operation: Ref) -> Result<(usize, bool), Status> {
        if let Some(i) = self.outcomes.iter().position(|o| !o.request.is_empty() && o.caller == p && o.operation == operation && o.id == header.request_id) {
            if !self.outcomes[i].matches(request, &mut None) { return Err(Status::Malformed); }
            return Ok((i, true));
        }
        let fence = if let Some(i) = self.fences.iter().position(|f| f.caller == p && f.operation == operation) { i } else {
            if self.fences.len() >= 512 { self.prune(); } if self.fences.len() >= 512 { return Err(Status::QuotaExceeded); } reserve(&mut self.fences, 1)?;
            self.fences.push(Fence { caller: p, operation, next: 1, acknowledged: 0 }); self.fences.len() - 1
        };
        if header.request_id != self.fences[fence].next { return Err(Status::UnknownRequest); }
        if self.outcomes.iter().filter(|o| o.caller == p && o.operation == operation && !o.request.is_empty()).count() >= 64
            || self.outcomes.iter().filter(|o| o.caller == p && !o.request.is_empty()).count() >= 128 { return Err(Status::QuotaExceeded); }
        let next = header.request_id.checked_add(1).ok_or(Status::QuotaExceeded)?;
        let (copy, sealed) = retained(header.selector, request)?;
        let mut result = Vec::new(); reserve(&mut result, MAX_SERVICE_RECORD_LEN)?;
        let slot = self.outcomes.iter().position(|o| o.request.is_empty());
        if slot.is_none() && self.outcomes.len() >= 1024 { return Err(Status::QuotaExceeded); }
        if slot.is_none() { reserve(&mut self.outcomes, 1)?; }
        let record = Outcome { caller: p, operation, id: header.request_id, selector: header.selector, request: copy, sealed, result, observed: false, pending: true, poll_session: ZERO, poll_epoch: 0, cursor: 0 };
        let index = if let Some(slot) = slot { self.outcomes[slot] = record; slot } else { self.outcomes.push(record); self.outcomes.len() - 1 };
        self.fences[fence].next = next;
        Ok((index, false))
    }
    fn finish(&mut self, index: usize, status: Status, payload: &[u8]) {
        let o = &mut self.outcomes[index]; o.result.resize(HEADER_LEN + payload.len(), 0);
        let h = ResultHeaderV1 { selector: o.selector, status, total_len: o.result.len() as u32, request_id: o.id };
        let _ = h.encode(&mut o.result); o.result[HEADER_LEN..].copy_from_slice(payload); o.pending = false;
    }
    fn send(bytes: &[u8], sender: usize, token: usize) -> bool {
        let mut envelope = Scrub::<{ api::ipc::IPC_BUF_SIZE }>::new();
        let Ok(encoded) = api::ipc::encode(&api::ipc::InputResponse::SessionV1(bytes), &mut envelope) else { return false; };
        if token != 0 { sys_ipc_reply(token, encoded) == 0 } else { matches!(sys_try_send(sender, encoded), ostd::syscall::SyscallResult::Ok(0)) }
    }
    fn error(header: RequestHeaderV1, status: Status, sender: usize, token: usize) {
        let mut bytes = [0; HEADER_LEN]; let _ = ResultHeaderV1 { selector: header.selector, status, total_len: HEADER_LEN as u32, request_id: header.request_id }.encode(&mut bytes); Self::send(&bytes, sender, token);
    }
    fn reply_outcome(&mut self, index: usize, header: RequestHeaderV1, caller: Principal, sender: usize, token: usize) {
        let outcome = &self.outcomes[index];
        let denial = if outcome.pending { Some(Status::WouldBlock) }
        else if outcome.poll_session != ZERO {
            match self.sessions.iter().find(|s| !s.closed && s.reference == outcome.poll_session && s.epoch == outcome.poll_epoch && s.reader == caller) {
                Some(session) if session.frozen => Some(Status::WouldBlock),
                Some(_) => None,
                None => Some(Status::WrongEpoch),
            }
        } else { None };
        if let Some(status) = denial { Self::error(header, status, sender, token); return; }
        let outcome = &mut self.outcomes[index];
        outcome.observed |= Self::send(&outcome.result, sender, token);
    }
    pub fn request(&mut self, bytes: &[u8], caller: Option<CallerIdentity>, sender: usize) {
        let token = sys_ipc_current(); let token = if token > 0 { token as usize } else { 0 };
        let header = match RequestHeaderV1::decode(bytes) {
            Ok(header) => header,
            Err(status) => {
                if bytes.len() >= HEADER_LEN {
                    let header = RequestHeaderV1 {
                        selector: u16::from_le_bytes([bytes[4], bytes[5]]),
                        total_len: bytes.len() as u32,
                        request_id: u64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8])),
                    };
                    Self::error(header, status, sender, token);
                }
                return;
            }
        };
        if !(1..=31).contains(&header.selector) { Self::error(header, Status::UnknownOperation, sender, token); return; }
        let Some(caller) = caller.filter(|c| c.sender_tid == sender as u64 && c.generation != 0) else { Self::error(header, Status::Denied, sender, token); return; };
        let p = principal(caller);
        if bytes.len() > MAX_SERVICE_RECORD_LEN { Self::error(header, Status::Malformed, sender, token); return; }
        if let Err(status) = self.binding() { Self::error(header, status, sender, token); return; }
        self.maintain();
        // Resolve exact retained bytes before resource lookup: CommitPaste,
        // AbortPaste and CloseSession remove the live selector they mutate.
        let mut digest = None;
        if let Some(index) = self.outcomes.iter().position(|o| o.caller == p && o.matches(bytes, &mut digest)) {
            self.reply_outcome(index, header, p, sender, token);
            return;
        }
        let body = &bytes[HEADER_LEN..];
        let operation = match self.operation_for(header.selector, body, p) { Ok(v) => v, Err(status) => { Self::error(header, status, sender, token); return; } };
        if let Err(status) = self.admit(header.selector, body, p, sender) { Self::error(header, status, sender, token); return; }
        if header.selector == 18 || header.selector == 19 { self.query_ack(header, body, p, sender, token); return; }
        // Nonmutating reads (QueryCapabilities, QueryFrontend) retain no outcome and consume no ID.
        if header.selector == 12 || header.selector == 26 {
            let mut reply = [0; MAX_SERVICE_RECORD_LEN - HEADER_LEN];
            let result = self.execute(header.selector, body, p, usize::MAX, token, &mut reply);
            let mut record = [0; MAX_SERVICE_RECORD_LEN];
            let (status, length) = match result { Ok(Some(n)) => (Status::Ok, n), Ok(None) => (Status::Malformed, 0), Err(s) => (s, 0) };
            let _ = ResultHeaderV1 { selector: header.selector, status, total_len: (HEADER_LEN + length) as u32, request_id: header.request_id }.encode(&mut record[..HEADER_LEN]);
            record[HEADER_LEN..HEADER_LEN + length].copy_from_slice(&reply[..length]);
            Self::send(&record[..HEADER_LEN + length], sender, token);
            return;
        }
        let (outcome, replay) = match self.record(header, bytes, p, operation) { Ok(v) => v, Err(status) => { Self::error(header, status, sender, token); return; } };
        if replay {
            self.reply_outcome(outcome, header, p, sender, token);
            return;
        }
        let mut reply = Scrub::<{ MAX_SERVICE_RECORD_LEN - HEADER_LEN }>::new();
        match self.execute(header.selector, body, p, outcome, token, &mut reply) {
            Ok(Some(len)) => self.finish(outcome, Status::Ok, &reply[..len]),
            Ok(None) => return,
            Err(status) => self.finish(outcome, status, &[]),
        }
        let o = &mut self.outcomes[outcome]; o.observed |= Self::send(&o.result, sender, token);
    }
    fn query_ack(&mut self, h: RequestHeaderV1, body: &[u8], p: Principal, sender: usize, token: usize) {
        let mut r = Reader::new(body); let parsed = (|| { let op = r.reference()?; let through = r.u64()?; r.finish()?; Ok::<_, Status>((op, through)) })();
        let (op, id) = match parsed { Ok(v) => v, Err(s) => { Self::error(h, s, sender, token); return; } };
        if h.selector == 19 {
            if let Some(index) = self.outcomes.iter().position(|o| !o.request.is_empty() && o.caller == p && o.operation == op && o.id == id) {
                self.reply_outcome(index, h, p, sender, token);
            } else { Self::error(h, Status::UnknownRequest, sender, token); }
            return;
        }
        let Some(fence) = self.fences.iter().position(|f| f.caller == p && f.operation == op) else { Self::error(h, Status::UnknownRequest, sender, token); return; };
        if id <= self.fences[fence].acknowledged { Self::error(h, Status::Ok, sender, token); return; }
        if id >= self.fences[fence].next || self.outcomes.iter().any(|o| o.caller == p && o.operation == op && o.id <= id && (!o.observed || o.pending)) { Self::error(h, Status::UnknownRequest, sender, token); return; }
        for o in self.outcomes.iter_mut().filter(|o| o.caller == p && o.operation == op && o.id <= id) {
            if o.poll_session != ZERO { if let Some(s) = self.sessions.iter_mut().find(|s| s.reference == o.poll_session && s.epoch == o.poll_epoch) { while s.typed.front().is_some_and(|e| e.sequence <= o.cursor) { s.typed.pop_front(); } s.delivered_cursor = s.delivered_cursor.max(o.cursor); } }
            o.scrub();
        }
        // Stable outcome indices are used by saved poll tokens. Tombstones retain
        // their ID fence until the operation/principal is retired.
        self.fences[fence].acknowledged = self.fences[fence].acknowledged.max(id);
        Self::error(h, Status::Ok, sender, token);
    }
    fn execute(&mut self, selector: u16, body: &[u8], p: Principal, outcome: usize, token: usize, out: &mut [u8]) -> Result<Option<usize>, Status> {
        let mut r = Reader::new(body); let mut w = Writer::new(out);
        if selector == 1 {
            let operation = r.reference()?; let bootstrap = r.reference()?; let kind = r.u8()?; let flags = r.u8()?; let rows = r.u16()?; let cols = r.u16()?; reserved(r.u16()? as u32)?; r.finish()?;
            self.kernel_operation = operation;
            // GUI sessions are admitted only through RegisterFrontend/CreateForFrontend.
            if flags != 0 { return Err(Status::InvalidFlags); } if kind > 2 { return Err(Status::Malformed); } if kind != 0 { return Err(Status::Unsupported); } dimensions(rows, cols)?;
            let (i, endpoints) = self.create_session(operation, bootstrap, p, fe::FRONTEND_KIND_SERIAL, rows, cols, None)?;
            let s = &self.sessions[i];
            w.reference(s.reference)?; w.principal(p)?; w.reference(s.lease)?; w.u64(1)?; w.u32(2)?; w.u32(0)?; for endpoint in endpoints { w.reference(endpoint)?; }
            return Ok(Some(w.position()));
        }
        if (23..=28).contains(&selector) { return self.frontend_operation(selector, body, p, &mut w).map(|_| Some(w.position())); }
        if (15..=17).contains(&selector) { return self.paste_operation(selector, r, p, &mut w).map(|_| Some(w.position())); }
        if (29..=31).contains(&selector) { return self.trust_operation(selector, body, p, &mut w).map(|_| Some(w.position())); }
        let reference = r.reference()?; let i = self.index(reference)?;
        self.kernel_operation = self.sessions[i].operation;
        match selector {
            2 => {
                let frontend = r.principal()?; let operation = r.reference()?; let input = r.reference()?; let output = r.reference()?; r.finish()?; self.controller(i, p)?;
                if self.sessions[i].attachment.is_some() { return Err(Status::WouldBlock); }
                // A sel-25 session's frontend returns only through ReattachFrontend.
                if self.sessions[i].holder.is_some() { return Err(Status::Denied); }
                if operation != self.sessions[i].operation || frontend != p { return Err(Status::Denied); }
                let (_, _, _, endpoints, outputs, _) = self.resolve_bootstrap(self.sessions[i].bootstrap, 0, ZERO)?;
                if input != endpoints[0] || output != outputs[0] { return Err(Status::WrongSession); }
                // The admitted headless Serial frontend is the attested
                // controller, not an arbitrary payload-named process. GUI/SSH
                // and foreign-process forwarding require their own live bridge.
                let attachment = self.reference()?;
                self.sessions[i].attachment = Some(Attachment { reference: attachment, frontend });
                if frontend == p && self.serial.is_none() && !compositor_registered() { self.serial = Some(reference); }
                w.reference(attachment)?;
            }
            3 => {
                let expected = r.u64()?; let child = r.child()?; let endpoints = [r.reference()?, r.reference()?, r.reference()?, r.reference()?]; let writers = [r.reference()?, r.reference()?]; r.finish()?; self.controller(i, p)?; self.epoch(i, expected)?;
                self.prepare_foreground(i, child, expected, endpoints, writers, &mut w)?;
            }
            4 => {
                let lease = r.reference()?; let child = r.child()?; let receipt = r.reference()?; r.finish()?; self.controller(i, p)?;
                let s = &self.sessions[i]; let f = s.foreground.as_ref().ok_or(Status::Stale)?;
                if f.lease != lease || f.child != child || f.receipt != receipt { return Err(Status::Denied); }
                if !f.committed { return Err(if f.restored { Status::Cancelled } else { Status::WouldBlock }); } w.u64(s.epoch)?; w.principal(s.reader)?;
            }
            5 => {
                let lease = r.reference()?; let expected = r.u64()?; let child = r.child()?; r.finish()?; self.controller(i, p)?;
                let f = self.sessions[i].foreground.as_ref().ok_or(Status::Stale)?; if f.lease != lease || f.child != child || f.next != expected { return Err(Status::WrongEpoch); }
                if !f.restored { self.restore(i)?; } w.u64(self.sessions[i].epoch)?; w.principal(self.sessions[i].reader)?;
            }
            6 => {
                let attachment = r.reference()?; let rows = r.u16()?; let cols = r.u16()?; reserved(r.u32()?)?; r.finish()?; dimensions(rows, cols)?;
                self.frontend(i, p, attachment, self.sessions[i].epoch)?;
                let mut payload = [0; 4]; payload[..2].copy_from_slice(&rows.to_le_bytes()); payload[2..].copy_from_slice(&cols.to_le_bytes());
                let sequence = self.control(i, 5, &payload)?; self.sessions[i].rows = rows; self.sessions[i].cols = cols; w.u64(sequence)?;
            }
            7 => {
                let lease = r.reference()?; let epoch = r.u64()?; let mask = r.u32()?; let value = r.u32()?; r.finish()?; self.reader(i, p, lease)?; self.epoch(i, epoch)?;
                if mask & !31 != 0 || value & !mask != 0 { return Err(Status::InvalidFlags); } if mask & !MODE_MASK != 0 { return Err(Status::Unsupported); }
                // A physical UART's local echo is outside this owner's control: echo-off cannot be honored there.
                if mask & 2 != 0 && value & 2 == 0 && self.serial == Some(reference) { return Err(Status::Unsupported); }
                if self.sessions[i].modes.len() + self.sessions[i].baseline_modes.len() >= 64 { return Err(Status::QuotaExceeded); }
                if !self.sessions[i].raw.is_empty() || !self.sessions[i].typed.is_empty() || self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() || self.sessions[i].poll.is_some() { return Err(Status::WouldBlock); }
                let mode_lease = self.reference()?; let s = &mut self.sessions[i]; s.modes.push(Mode { reference: mode_lease, holder: p, epoch, mask, value }); s.mode = (s.mode & !mask) | value;
                w.reference(mode_lease)?; w.u32(s.mode)?; w.u32(0)?;
            }
            8 => {
                let lease = r.reference()?; r.finish()?;
                if self.sessions[i].frozen { return Err(Status::WouldBlock); }
                let before = self.sessions[i].mode;
                {
                    let s = &mut self.sessions[i];
                    if let Some(mode) = s.modes.iter().position(|m| m.reference == lease) {
                        if s.modes[mode].holder != p && s.controller != p { return Err(Status::Denied); }
                        if s.modes[mode].epoch != s.epoch { return Err(Status::WrongEpoch); }
                        s.modes.remove(mode); Self::recompute_mode(s);
                    } else if let Some(mode) = s.baseline_modes.iter().position(|m| m.reference == lease) {
                        if s.baseline_modes[mode].holder != p && s.controller != p { return Err(Status::Denied); }
                        s.baseline_modes.remove(mode);
                    } else { return Err(Status::Stale); }
                }
                if (before ^ self.sessions[i].mode) & 1 != 0 {
                    // Routing flips between typed and raw: input queued for the old route is revoked.
                    self.invalidate_poll(i, Status::WrongDirection);
                    let s = &mut self.sessions[i]; zeroize_deque(&mut s.raw); s.typed.clear();
                }
                let s = &self.sessions[i]; w.u32(s.mode)?; w.u32(0)?;
            }
            9 => {
                let lease = r.reference()?; let cursor = r.u64()?; let maximum = r.u16()? as usize; reserved(r.u16()? as u32)?; reserved(r.u32()?)?; let deadline = r.u64()?; r.finish()?; self.reader(i, p, lease)?;
                if maximum == 0 || maximum > 64 { return Err(Status::Malformed); } if self.sessions[i].mode & 1 != 0 { return Err(Status::WrongDirection); }
                if self.sessions[i].poll.is_some() || self.outcomes.iter().enumerate().any(|(n,o)| n != outcome && o.poll_session == reference && o.poll_epoch == self.sessions[i].epoch && !o.result.is_empty()) { return Err(Status::WouldBlock); }
                if cursor != self.sessions[i].delivered_cursor { return Err(Status::Malformed); }
                self.outcomes[outcome].poll_session = reference; self.outcomes[outcome].poll_epoch = self.sessions[i].epoch;
                if let Some(len) = self.poll_batch(i, outcome, cursor, maximum, out)? { return Ok(Some(len)); }
                if deadline == 0 { return Err(Status::WouldBlock); } if token == 0 { return Err(Status::Unsupported); }
                let timestamp = now(); if deadline != u64::MAX && deadline <= now_ns() { return Err(Status::TimedOut); }
                // Reply before the SDK's 100-tick bounded wait lapses so a timeout result is observed, not stranded.
                self.sessions[i].poll = Some(PendingPoll { outcome, token, epoch: self.sessions[i].epoch, lease, cursor, maximum, expires: timestamp.saturating_add(900), deadline });
                return Ok(None);
            }
            10 => {
                let attachment = r.reference()?; r.finish()?;
                if self.sessions[i].holder.is_none() {
                    self.controller(i, p)?; if !self.sessions[i].attachment.as_ref().is_some_and(|a| a.reference == attachment) { return Err(Status::Stale); } self.close_session(i, outcome)?;
                } else {
                    // A sel-25 session outlives its frontend: holder or controller only detach it.
                    let s = &self.sessions[i];
                    if s.controller != p && s.holder != Some(p) { return Err(Status::Denied); }
                    if !s.attachment.as_ref().is_some_and(|a| a.reference == attachment) { return Err(Status::Stale); }
                    if let Some(fi) = s.frontend.and_then(|f| self.frontend_index(f).ok()) { let f = &mut self.frontends[fi]; f.state = fe::FRONTEND_STATE_LOST; f.session = ZERO; }
                    self.detach(i);
                }
            }
            11 => { r.finish()?; self.controller(i, p)?; self.close_session(i, outcome)?; }
            12 => {
                r.finish()?; if self.sessions[i].controller != p && self.sessions[i].reader != p { return Err(Status::Denied); }
                let physical = self.serial == Some(reference); let s = &self.sessions[i];
                w.u8(s.kind)?; w.u8(0)?; w.u16(s.rows)?; w.u16(s.cols)?; w.u16(0)?;
                // Echo suppression is unverifiable on the physical UART; virtual Serial submissions honor it.
                w.u32(if physical { MODE_MASK & !2 } else { MODE_MASK })?; w.u32(0xff)?; w.u32(if physical { 0x5f } else { 0x7f })?; w.u32(0)?;
            }
            13 => {
                let attachment = r.reference()?; let epoch = r.u64()?; let count = r.u16()? as usize; reserved(r.u16()? as u32)?; let len = r.u32()? as usize; let events = r.take(len)?; r.finish()?; let epoch = self.wire_epoch(i, epoch)?; self.frontend(i, p, attachment, epoch)?;
                if count == 0 || count > 64 { return Err(Status::Malformed); } self.admit_events(i, events, count)?; w.u64(self.sessions[i].sequence)?;
            }
            14 => {
                let attachment = r.reference()?; let epoch = r.u64()?; let total = r.u32()? as usize; reserved(r.u32()?)?; r.finish()?; let epoch = self.wire_epoch(i, epoch)?; self.frontend(i, p, attachment, epoch)?;
                if total == 0 || total > 65536 { return Err(Status::Malformed); }
                // A paste never answers a trust prompt: it rejects the capture and is refused.
                if self.refuse_paste(i) { return Err(Status::Cancelled); }
                if self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() || self.sessions[i].physical_paste_discard || self.sessions[i].draining { return Err(Status::WouldBlock); }
                let mut bytes = Vec::new(); reserve(&mut bytes, total)?; let paste = self.reference()?;
                self.sessions[i].paste = Some(Paste { reference: paste, frontend: p, attachment, epoch, expires: now().saturating_add(5000), total, bytes }); w.reference(paste)?;
            }
            20 => {
                let attachment = r.reference()?; let epoch = r.u64()?; let count = r.u32()? as usize; reserved(r.u32()?)?; let bytes = r.take(count)?; r.finish()?; let epoch = self.wire_epoch(i, epoch)?; self.frontend(i, p, attachment, epoch)?;
                if count == 0 || count > 512 { return Err(Status::Malformed); } if self.sessions[i].mode & 1 == 0 { return Err(Status::WrongDirection); }
                // Raw bytes carry no key/paste provenance: they never answer a trust prompt.
                if self.sessions[i].capture.is_some() { self.capture_raw(i); w.u32(count as u32)?; w.u32(0)?; return Ok(Some(w.position())); }
                if let Some(status) = self.sessions[i].input_failure { return Err(status); }
                if self.sessions[i].draining { return Err(Status::BrokenPipe); }
                if self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() || !self.sessions[i].raw.is_empty()
                    || self.sessions[i].controls.iter().any(|e| e.sent != e.bytes.len()) { return Err(Status::WouldBlock); }
                let count = self.write_stream(self.sessions[i].writers[0], bytes)?; w.u32(count as u32)?; w.u32(0)?;
            }
            21 => {
                let attachment = r.reference()?; let epoch = r.u64()?; r.finish()?; let epoch = self.wire_epoch(i, epoch)?; self.frontend(i, p, attachment, epoch)?;
                if let Some(status) = self.sessions[i].input_failure { return Err(status); }
                if self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() { return Err(Status::WouldBlock); }
                if !self.sessions[i].eof && self.sessions[i].controls.len() >= CONTROL_LIMIT - 1 { return Err(Status::QuotaExceeded); }
                // Input ends: no answer to a trust prompt can arrive any more.
                self.end_capture(i, api::cell_session::trust::DECISION_CANCELLED, trust::End::Release);
                self.sessions[i].draining = true; self.drain(i); w.u32(if self.sessions[i].eof { 1 } else { 0 })?; w.u32(0)?;
                if let Some(status) = self.sessions[i].input_failure { return Err(status); }
            }
            22 => {
                let lease = r.reference()?; let epoch = r.u64()?; let through = r.u64()?; r.finish()?;
                // The controller, while a foreground child is the reader, acknowledges the owner
                // notices (trust prompt / prompt end) it has drawn on its own control ring.
                if self.sessions[i].controller == p && self.sessions[i].reader != p { self.acknowledge_notices(i, lease, epoch, through)?; return Ok(Some(w.position())); }
                self.reader(i, p, lease)?; self.epoch(i, epoch)?;
                let s = &mut self.sessions[i]; if s.controls.iter().any(|e| e.sequence <= through && e.sent != e.bytes.len()) || through > s.sequence { return Err(Status::Malformed); }
                while s.controls.front().is_some_and(|e| e.sequence <= through) { s.controls.pop_front(); }
            }
            _ => return Err(Status::UnknownOperation),
        }
        Ok(Some(w.position()))
    }
    fn recompute_mode(s: &mut Session) { s.mode = 2; for m in &s.modes { if m.epoch == s.epoch { s.mode = (s.mode & !m.mask) | m.value; } } }
    /// Shared tail of Create (1) and CreateForFrontend (25): limits, bootstrap proof,
    /// then activation; nothing fails after the kernel activates the bootstrap.
    /// `frontend` = (holder, holder's registered operation, frontend record).
    fn create_session(&mut self, operation: Ref, bootstrap: Ref, p: Principal, kind: u8, rows: u16, cols: u16, frontend: Option<(Principal, Ref, Ref)>) -> Result<(usize, [Ref; 4]), Status> {
        if self.sessions.iter().filter(|s| !s.closed).count() >= 64 || self.sessions.iter().filter(|s| !s.closed && s.controller == p).count() >= 8 { return Err(Status::QuotaExceeded); }
        let (state, controller, op, endpoints, _, _) = self.resolve_bootstrap(bootstrap, 0, ZERO)?;
        if state != 0 || controller != p || op != operation { return Err(Status::Denied); }
        let reference = self.reference()?; let lease = self.reference()?;
        let (attachment, resume) = match frontend {
            Some((holder, ..)) => (Some(Attachment { reference: self.reference()?, frontend: holder }), self.reference()?),
            None => (None, ZERO),
        };
        let mut typed = VecDeque::new(); typed.try_reserve(INPUT_LIMIT).map_err(|_| Status::QuotaExceeded)?;
        let mut controls = VecDeque::new(); controls.try_reserve(CONTROL_LIMIT).map_err(|_| Status::QuotaExceeded)?;
        let mut raw = VecDeque::new(); raw.try_reserve(RAW_LIMIT).map_err(|_| Status::QuotaExceeded)?;
        let mut modes = Vec::new(); reserve(&mut modes, 64)?; reserve(&mut self.sessions, 1)?;
        let mut baseline_modes = Vec::new(); reserve(&mut baseline_modes, 64)?;
        let (_, _, _, _, _, writers) = self.resolve_bootstrap(bootstrap, 1, ZERO)?;
        let session = Session {
            reference, bootstrap, controller, operation, baseline: endpoints,
            baseline_writers: writers, endpoints, writers, baseline_lease: lease,
            lease, reader: p, epoch: 1, sequence: 0, rows, cols,
            attachment, foreground: None, modes, baseline_modes, mode: 2,
            typed, controls, raw, paste: None, poll: None, draining: false,
            eof: false, baseline_eof: false, frozen: false, closed: false, input_failure: None,
            decoder: api::services::session::SerialDecoder::new(),
            physical_paste: None, physical_paste_expires: 0,
            physical_paste_complete: false, physical_paste_discard: false,
            delivered_cursor: 0, prepare_deadline: 0,
            kind, holder: frontend.map(|f| f.0), holder_operation: frontend.map_or(ZERO, |f| f.1),
            frontend: frontend.map(|f| f.2), resume, detached: false, orphaned: false,
            capture: None, notices: VecDeque::new(),
        };
        if let Err(error) = self.resolve_bootstrap(bootstrap, 3, reference) { let _ = self.resolve_bootstrap(bootstrap, 2, ZERO); return Err(error); }
        let index = if let Some(slot) = self.sessions.iter().position(|s| s.closed) { self.sessions[slot] = session; slot } else { self.sessions.push(session); self.sessions.len() - 1 };
        Ok((index, endpoints))
    }
    /// Kernel `QueryOperation` (issuer-gated read): 0 = live, otherwise cancelled.
    fn query_operation(&mut self, operation: Ref) -> Result<u32, Status> {
        let mut reply = [0; HEADER_LEN + 16]; let len = self.kernel(KernelOp::QueryOperation, &operation.to_bytes(), &mut reply)?;
        let mut r = Reader::new(&reply[HEADER_LEN..len]); let state = r.u32()?; r.u32()?; r.u32()?; reserved(r.u32()?)?; r.finish()?;
        Ok(state)
    }
    /// Frontend selectors 23–28 (annex S5-3c §3, §3a).
    fn frontend_operation(&mut self, selector: u16, body: &[u8], p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        match selector {
            23 => {
                let request = fe::RegisterFrontendV1::decode(body)?;
                self.kernel_operation = request.operation;
                if request.kind > fe::FRONTEND_KIND_SSH { return Err(Status::Malformed); }
                // Serial needs no registration; Ssh stays Unsupported until its frontend is admitted.
                if request.kind != fe::FRONTEND_KIND_GUI { return Err(Status::Unsupported); }
                dimensions(request.rows, request.cols)?;
                if self.frontends.len() >= 64 || self.frontends.iter().filter(|f| f.holder == p).count() >= 8 { return Err(Status::QuotaExceeded); }
                if self.query_operation(request.operation)? != 0 { return Err(Status::Cancelled); }
                reserve(&mut self.frontends, 1)?; let reference = self.reference()?;
                self.frontends.push(Frontend {
                    reference, holder: p, operation: request.operation, kind: request.kind, rows: request.rows, cols: request.cols,
                    state: fe::FRONTEND_STATE_PENDING, child: None, receipt: ZERO, session: ZERO,
                });
                w.reference(reference)
            }
            24 => { let request = fe::BindFrontendChildV1::decode(body)?; let fi = self.owned_frontend(request.frontend, p)?; self.bind_frontend(fi, request.child, w) }
            25 => self.create_for_frontend(fe::CreateForFrontendV1::decode(body)?, p, w),
            26 => {
                let fi = self.owned_frontend(Ref::from_bytes(body)?, p)?; let f = &self.frontends[fi];
                let mut status = fe::FrontendStatusV1 { state: f.state, session: ZERO, attachment: ZERO, resume: ZERO };
                if f.state == fe::FRONTEND_STATE_ATTACHED {
                    let s = &self.sessions[self.index(f.session)?];
                    status.session = s.reference; status.attachment = s.attachment.as_ref().map_or(ZERO, |a| a.reference); status.resume = s.resume;
                }
                let mut bytes = [0; fe::FrontendStatusV1::LEN]; status.encode(&mut bytes)?; w.bytes(&bytes)
            }
            27 => {
                let fi = self.owned_frontend(Ref::from_bytes(body)?, p)?;
                if self.frontends[fi].state == fe::FRONTEND_STATE_ATTACHED { let i = self.index(self.frontends[fi].session)?; self.detach(i); }
                self.remove_frontend(fi, false); Ok(())
            }
            28 => self.reattach(fe::ReattachFrontendV1::decode(body)?, p, w),
            _ => Err(Status::UnknownOperation),
        }
    }
    /// Sel 24: prove the child was prepared by the holder under its registered operation,
    /// carrying exactly this frontend in slot 8, then register the kernel kind-6 receipt.
    fn bind_frontend(&mut self, fi: usize, child: ChildRefV1, w: &mut Writer<'_>) -> Result<(), Status> {
        let (frontend, holder, operation) = { let f = &self.frontends[fi]; if f.state != fe::FRONTEND_STATE_PENDING || f.child.is_some() { return Err(Status::Denied); } (f.reference, f.holder, f.operation) };
        self.kernel_operation = operation;
        let prepared = child.prepared_ref();
        let mut body = [0; 32]; let mut b = Writer::new(&mut body); b.reference(prepared)?; b.u16(GRANT_KIND_INPUT_FRONTEND)?; b.u16(SLOT_FRONTEND)?; b.u32(0)?; b.u32(512)?; b.u32(0)?;
        let mut reply = [0; 704]; let len = self.kernel(KernelOp::InspectPreparedGrant, &body, &mut reply)?;
        let mut r = Reader::new(&reply[HEADER_LEN..len]); let actual = r.child()?; let child_operation = r.reference()?; let digest: [u8; 32] = r.take(32)?.try_into().map_err(|_| Status::Malformed)?;
        let mut grant = Reader::new(r.take(40)?); let kind = grant.u16()?; let rights = grant.u16()?; let slot = grant.u16()?; reserved(grant.u16()? as u32)?; let owner_service = grant.u64()?; let object = grant.reference()?; let tail = grant.u64()?; grant.finish()?;
        let projection_len = r.u32()? as usize; reserved(r.u32()?)?; let count = r.u32()? as usize; reserved(r.u32()?)?; let projection = r.take(count)?; r.finish()?;
        if actual != child || count != projection_len || projection.len() != 32 { return Err(Status::InvalidChild); }
        if kind != GRANT_KIND_INPUT_FRONTEND || slot != SLOT_FRONTEND || rights != 0 || tail != 0
            || owner_service != api::syscall::service::INPUT as u64 || object != frontend { return Err(Status::InvalidChild); }
        let mut pr = Reader::new(projection); let preparer = pr.principal()?; let owner = pr.principal()?; pr.finish()?;
        if child_operation != operation || preparer != holder || owner != holder { return Err(Status::Denied); }
        let mut seal = [0; FRONTEND_SEAL_LEN]; let mut b = Writer::new(&mut seal); b.reference(frontend)?; b.child(child)?;
        let mut registration = [0; 96 + FRONTEND_SEAL_LEN]; let mut b = Writer::new(&mut registration);
        b.reference(prepared)?; b.u16(GRANT_KIND_INPUT_FRONTEND)?; b.u16(SLOT_FRONTEND)?; b.u32(0)?; b.reference(frontend)?; b.u64(0)?; b.u64(0)?; b.bytes(&digest)?; b.u32(FRONTEND_SEAL_LEN as u32)?; b.u32(0)?; b.bytes(&seal)?;
        let mut result = [0; HEADER_LEN + 16]; let length = self.kernel(KernelOp::RegisterBindReceipt, &registration, &mut result)?;
        let receipt = Ref::from_bytes(&result[HEADER_LEN..length])?;
        let f = &mut self.frontends[fi]; f.child = Some(child); f.receipt = receipt; f.state = fe::FRONTEND_STATE_BOUND;
        w.reference(receipt)
    }
    /// Sel 25: the committed bound child becomes the controller of a session of the
    /// frontend's kind, attached to the frontend holder. One-shot per frontend.
    fn create_for_frontend(&mut self, request: fe::CreateForFrontendV1, p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        self.kernel_operation = request.operation;
        let fi = self.frontend_index(request.frontend)?;
        let (holder, holder_operation, kind, child, receipt, registered) = {
            let f = &self.frontends[fi];
            let child = f.child.filter(|c| c.principal == p).ok_or(Status::Denied)?;
            if f.state != fe::FRONTEND_STATE_BOUND { return Err(Status::Denied); }
            (f.holder, f.operation, f.kind, child, f.receipt, (f.rows, f.cols))
        };
        // Zero size: the frontend's registered size (the shell cannot know the window).
        let (rows, cols) = if request.rows == 0 && request.cols == 0 { registered } else { dimensions(request.rows, request.cols)?; (request.rows, request.cols) };
        let (state, actual, _) = self.receipt(receipt, 0)?;
        if actual != child || state != 1 { return Err(Status::InvalidChild); }
        self.kernel_operation = request.operation;
        let (i, endpoints) = self.create_session(request.operation, request.bootstrap, p, kind, rows, cols, Some((holder, holder_operation, request.frontend)))?;
        let s = &self.sessions[i];
        let result = fe::CreateForFrontendResultV1 {
            session: s.reference, controller: p, lease: s.lease, epoch: s.epoch, mode: s.mode, endpoints,
            attachment: s.attachment.as_ref().map_or(ZERO, |a| a.reference),
        };
        let session = s.reference;
        let f = &mut self.frontends[fi]; f.state = fe::FRONTEND_STATE_ATTACHED; f.session = session;
        let mut bytes = [0; fe::CreateForFrontendResultV1::LEN]; result.encode(&mut bytes)?; w.bytes(&bytes)
    }
    /// Sel 28: the resume holder binds a new Pending frontend to its Detached session.
    /// The reader gets a Resize with the new size, then Lost(FRONTEND_ATTACHED); `resume` is reissued.
    fn reattach(&mut self, request: fe::ReattachFrontendV1, p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        let fi = self.owned_frontend(request.frontend, p)?;
        let (operation, kind, rows, cols) = { let f = &self.frontends[fi]; if f.state != fe::FRONTEND_STATE_PENDING { return Err(Status::Denied); } (f.operation, f.kind, f.rows, f.cols) };
        if request.resume == ZERO { return Err(Status::Stale); }
        let i = self.sessions.iter().position(|s| !s.closed && s.resume == request.resume).ok_or(Status::Stale)?;
        let s = &self.sessions[i];
        // `resume` is holder-bound, not a bearer token.
        if s.holder != Some(p) { return Err(Status::WrongOwner); }
        if s.attachment.is_some() || s.frozen { return Err(Status::WouldBlock); }
        if s.kind != kind { return Err(Status::WrongSession); }
        if s.controls.len() >= CONTROL_LIMIT - 2 { return Err(Status::QuotaExceeded); }
        let attachment = self.reference()?; let resume = self.reference()?;
        let mut size = [0; 4]; size[..2].copy_from_slice(&rows.to_le_bytes()); size[2..].copy_from_slice(&cols.to_le_bytes());
        self.control(i, 5, &size)?;
        let s = &mut self.sessions[i];
        s.attachment = Some(Attachment { reference: attachment, frontend: p }); s.detached = false;
        s.frontend = Some(request.frontend); s.resume = resume; s.holder_operation = operation; s.rows = rows; s.cols = cols;
        let session = s.reference;
        self.control(i, 8, &fe::FRONTEND_ATTACHED.to_le_bytes())?;
        let f = &mut self.frontends[fi]; f.state = fe::FRONTEND_STATE_ATTACHED; f.session = session;
        let mut bytes = [0; fe::ReattachResultV1::LEN]; fe::ReattachResultV1 { session, attachment }.encode(&mut bytes)?; w.bytes(&bytes)
    }
    /// The frontend is gone; the controller keeps the session (annex §3a). Its pending
    /// paste dies with the attachment; queued input still drains to the reader.
    fn detach(&mut self, i: usize) {
        let s = &mut self.sessions[i];
        if s.attachment.take().is_none() { return; }
        s.detached = true; s.frontend = None; s.paste = None;
        // No frontend can type the answer any more: the trust capture ends cancelled.
        self.end_capture(i, api::cell_session::trust::DECISION_CANCELLED, trust::End::Release);
        let _ = self.control(i, 8, &fe::LOST_FRONTEND.to_le_bytes());
    }
    /// The holder died: `resume` is revoked and no frontend can return.
    fn orphan(&mut self, i: usize) {
        let frontend = self.sessions[i].frontend;
        self.detach(i);
        let s = &mut self.sessions[i]; s.orphaned = true; s.detached = true; s.resume = ZERO;
        if let Some(fi) = frontend.and_then(|f| self.frontend_index(f).ok()) { self.remove_frontend(fi, true); }
    }
    /// Forget a frontend record. Its kind-6 receipt is revoked if still unconsumed and
    /// then acknowledged (best effort; the kernel also reaps it with the holder). The
    /// holder's request lane survives a release; a dead holder's lane is retired.
    fn remove_frontend(&mut self, fi: usize, dead: bool) {
        let f = self.frontends.swap_remove(fi);
        if f.receipt != ZERO {
            self.kernel_operation = f.operation; let _ = self.receipt(f.receipt, 1);
            self.kernel_operation = f.operation; let _ = self.receipt(f.receipt, 2);
        }
        if dead { self.prune(); self.retire_lane(f.operation); }
    }
    /// Holder liveness of frontends not attached to a session (attached ones are
    /// checked with their session): a proven-dead operation releases the record.
    fn maintain_frontends(&mut self) {
        let mut n = 0;
        while n < self.frontends.len() {
            if self.frontends[n].state == fe::FRONTEND_STATE_ATTACHED { n += 1; continue; }
            match self.query_operation(self.frontends[n].operation) {
                Ok(0) => n += 1,
                Ok(_) | Err(Status::Stale | Status::Revoked | Status::Cancelled | Status::NotFound) => self.remove_frontend(n, true),
                Err(_) => n += 1,
            }
        }
    }
    fn prepare_foreground(&mut self, i: usize, child: ChildRefV1, expected: u64, endpoints: [Ref; 4], writers: [Ref; 2], w: &mut Writer<'_>) -> Result<(), Status> {
        if self.sessions[i].baseline_eof { return Err(Status::BrokenPipe); }
        if self.sessions[i].frozen || self.sessions[i].foreground.as_ref().is_some_and(|f| !f.restored || !f.acknowledged) || self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() { return Err(Status::WouldBlock); }
        let next = expected.checked_add(1).ok_or(Status::QuotaExceeded)?; let lease = self.reference()?;
        let prepared = child.prepared_ref();
        let mut body = [0; 32]; let mut b = Writer::new(&mut body); b.reference(prepared)?; b.u16(1)?; b.u16(0)?; b.u32(0)?; b.u32(512)?; b.u32(0)?;
        let mut reply = [0; 704]; let len = self.kernel(KernelOp::InspectPreparedGrant, &body, &mut reply)?;
        let mut r = Reader::new(&reply[HEADER_LEN..len]); let actual = r.child()?; let operation = r.reference()?; let digest: [u8;32] = r.take(32)?.try_into().map_err(|_| Status::Malformed)?;
        r.take(40)?; let projection_len = r.u32()? as usize; reserved(r.u32()?)?; let count = r.u32()? as usize; reserved(r.u32()?)?; let projection = r.take(count)?; r.finish()?;
        if actual != child || count != projection_len { return Err(Status::InvalidChild); }
        self.kernel_operation = operation;
        // The issuer may see only the immutable completed stdio projection. Its
        // endpoint refs must exactly match the parent-intent-selected grant set.
        if projection.len() != 160 { return Err(Status::Malformed); }
        for slot in 0..4 { let mut grant = Reader::new(&projection[slot*40..(slot+1)*40]); if grant.u16()? != 1 { return Err(Status::Malformed); } let rights = grant.u16()?; if grant.u16()? != slot as u16 { return Err(Status::Malformed); } reserved(grant.u16()? as u32)?; if grant.u64()? != 0 || grant.reference()? != endpoints[slot] || grant.u64()? != 0 { return Err(Status::InvalidChild); } if rights as u32 & if slot == 0 || slot == 3 { 1 } else { 2 } == 0 { return Err(Status::WrongDirection); } }
        let mut seal = [0;192]; let mut b = Writer::new(&mut seal); b.reference(self.sessions[i].reference)?; b.reference(lease)?; b.reference(operation)?; b.child(child)?; b.u64(expected)?; b.u64(next)?; for endpoint in endpoints { b.reference(endpoint)?; } for writer in writers { b.reference(writer)?; }
        let mut registration = [0;288]; let mut b = Writer::new(&mut registration); b.reference(prepared)?; b.u16(1)?; b.u16(0)?; b.u32(0)?; b.reference(lease)?; b.u64(expected)?; b.u64(next)?; b.bytes(&digest)?; b.u32(192)?; b.u32(0)?; b.bytes(&seal)?;
        self.sessions[i].frozen = true; self.sessions[i].prepare_deadline = now().saturating_add(6000); self.invalidate_poll(i, Status::WouldBlock);
        let mut result = [0;40]; let length = match self.kernel(KernelOp::RegisterBindReceipt, &registration, &mut result) { Ok(v) => v, Err(s) => { if s != Status::Indeterminate { self.sessions[i].frozen = false; self.sessions[i].prepare_deadline = 0; } return Err(s); } };
        let mut r = Reader::new(&result[HEADER_LEN..length]); let receipt = r.reference()?; r.finish()?;
        self.sessions[i].prepare_deadline = 0;
        self.sessions[i].foreground = Some(Foreground { lease, receipt, child, operation, expected, next, endpoints, writers, committed: false, restored: false, acknowledged: false, cancelled: false });
        w.reference(lease)?; w.reference(receipt)?; w.u64(next)?; Ok(())
    }
    fn receipt(&mut self, receipt: Ref, action: u32) -> Result<(u32, ChildRefV1, [Ref; 2]), Status> {
        let mut body = [0;24]; let mut b = Writer::new(&mut body); b.reference(receipt)?; b.u32(action)?; b.u32(0)?;
        if let Some(f) = self.sessions.iter().filter_map(|s| s.foreground.as_ref()).find(|f| f.receipt == receipt) { self.kernel_operation = f.operation; }
        let mut result = [0;96]; let len = self.kernel(KernelOp::ResolveBindReceipt, &body, &mut result)?; let mut r = Reader::new(&result[HEADER_LEN..len]); let state = r.u32()?; reserved(r.u32()?)?; let child = r.child()?; let writers = [r.reference()?, r.reference()?]; r.finish()?; Ok((state, child, writers))
    }
    fn reconcile(&mut self, i: usize) -> Result<(), Status> {
        let Some(f) = self.sessions[i].foreground.as_ref() else { return Ok(()); };
        let receipt = f.receipt; let child = f.child; let committed = f.committed;
        if f.restored {
            if !f.acknowledged { self.receipt(receipt, 2)?; self.sessions[i].foreground.as_mut().ok_or(Status::Stale)?.acknowledged = true; self.retire_foreground(i); }
            return Ok(());
        }
        let (state, actual, writers) = self.receipt(receipt, 0)?; if actual != child { return Err(Status::InvalidChild); }
        match state {
            0 => { if committed { return Err(Status::Stale); } }
            1 if !committed => {
                if writers.contains(&ZERO) { return Err(Status::Malformed); }
                self.invalidate_poll(i, Status::WrongEpoch); let s = &mut self.sessions[i]; let f = s.foreground.as_mut().ok_or(Status::Stale)?;
                s.typed.clear(); s.controls.clear(); zeroize_deque(&mut s.raw); s.paste = None; s.physical_paste = None; s.decoder = api::services::session::SerialDecoder::new(); s.delivered_cursor = 0;
                s.physical_paste_complete = false; s.physical_paste_discard = false;
                core::mem::swap(&mut s.modes, &mut s.baseline_modes); s.modes.clear(); s.mode = 2; s.draining = false; s.eof = false; s.input_failure = None;
                s.reader = f.child.principal; s.lease = f.lease; s.epoch = f.next; s.endpoints = f.endpoints; s.writers = writers; f.writers = writers; f.committed = true; s.frozen = false;
            }
            1 => { let _ = self.restore(i); }
            2 | 3 if !committed => {
                self.sessions[i].frozen = false; self.sessions[i].foreground.as_mut().ok_or(Status::Stale)?.restored = true;
                self.receipt(receipt, 2)?; self.sessions[i].foreground.as_mut().ok_or(Status::Stale)?.acknowledged = true; self.retire_foreground(i);
            }
            _ => return Err(Status::Stale),
        }
        Ok(())
    }
    fn restore(&mut self, i: usize) -> Result<(), Status> {
        let f = self.sessions[i].foreground.as_ref().ok_or(Status::Stale)?; if !f.committed { return Err(Status::WouldBlock); } if f.restored { return Ok(()); } let receipt = f.receipt; let child = f.child; let expected = f.next;
        self.kernel_operation = f.operation;
        let mut result = [0;88]; let len = self.kernel(KernelOp::QueryReceiptChild, &receipt.to_bytes(), &mut result)?;
        let mut r = Reader::new(&result[HEADER_LEN..len]); let actual = r.child()?; let state = r.u32()?; r.u32()?; r.i32()?; reserved(r.u32()?)?; let terminal = r.u64()?; r.finish()?;
        if actual != child || terminal == 0 || !(3..=5).contains(&state) { return Err(Status::WouldBlock); } if self.sessions[i].epoch != expected { return Err(Status::WrongEpoch); }
        let epoch = expected.checked_add(1).ok_or(Status::QuotaExceeded)?;
        let mut body = [0;32]; let mut b = Writer::new(&mut body); b.reference(receipt)?; b.u64(terminal)?; b.u64(expected)?;
        let mut result = [0;56]; let len = self.kernel(KernelOp::RestoreSessionInput, &body, &mut result)?;
        let mut r = Reader::new(&result[HEADER_LEN..len]); let _readers = [r.reference()?, r.reference()?]; r.finish()?;
        // The kernel returns the controller's baseline reader refs; input's own producer peers are unchanged.
        let writers = self.sessions[i].baseline_writers;
        // The kernel just flushed the controller's control ring: a running capture ends
        // cancelled and unsent owner notices are dropped (the controller ends its prompt).
        self.end_capture(i, api::cell_session::trust::DECISION_CANCELLED, trust::End::Silent); self.sessions[i].notices.clear();
        self.invalidate_poll(i, Status::WrongEpoch); let s = &mut self.sessions[i]; s.typed.clear(); s.controls.clear(); zeroize_deque(&mut s.raw); s.paste = None; s.physical_paste = None; s.decoder = api::services::session::SerialDecoder::new(); s.delivered_cursor = 0; s.modes.clear(); s.mode = 2; s.draining = false; s.eof = false; s.epoch = epoch; s.reader = s.controller; s.lease = s.baseline_lease; s.endpoints = s.baseline; s.writers = writers; s.baseline_writers = writers; s.frozen = false; s.foreground.as_mut().ok_or(Status::Stale)?.restored = true;
        s.input_failure = None;
        s.physical_paste_complete = false; s.physical_paste_discard = false;
        s.modes.clear(); core::mem::swap(&mut s.modes, &mut s.baseline_modes);
        for mode in &mut s.modes { mode.epoch = epoch; }
        Self::recompute_mode(s);
        self.receipt(receipt, 2)?; self.sessions[i].foreground.as_mut().ok_or(Status::Stale)?.acknowledged = true; self.retire_foreground(i); Ok(())
    }
    /// Explicit close: tear the session down, then release its retained outcomes (but not the
    /// close request's own, `keep`).
    fn close_session(&mut self, i: usize, keep: usize) -> Result<(), Status> {
        let sop = self.sessions[i].operation; let fop = self.sessions[i].foreground.as_ref().map(|f| f.operation);
        self.close(i)?;
        self.release_closed(&[Some(sop), fop], keep);
        Ok(())
    }
    fn close(&mut self, i: usize) -> Result<(), Status> {
        let bootstrap = self.sessions[i].bootstrap;
        self.kernel_operation = self.sessions[i].operation;
        if let Some(f) = self.sessions[i].foreground.as_ref() { let receipt = f.receipt; if !f.restored { let _ = self.receipt(receipt, 1); } }
        self.kernel_operation = self.sessions[i].operation;
        match self.resolve_bootstrap(bootstrap, 2, ZERO) {
            Ok(_) | Err(Status::Stale | Status::Revoked | Status::Cancelled | Status::NotFound) => {}
            Err(status) => { self.sessions[i].frozen = true; return Err(status); }
        }
        self.end_capture(i, api::cell_session::trust::DECISION_CANCELLED, trust::End::Silent); self.sessions[i].notices.clear();
        self.invalidate_poll(i, Status::Revoked); let s = &mut self.sessions[i]; s.closed = true; s.typed.clear(); s.controls.clear(); zeroize_deque(&mut s.raw); s.paste = None; s.physical_paste = None; s.decoder = api::services::session::SerialDecoder::new(); s.modes.clear(); s.attachment = None;
        if self.serial == Some(s.reference) { self.serial = None; }
        s.detached = false; s.resume = ZERO;
        if let Some(reference) = s.frontend.take() {
            if let Some(f) = self.frontends.iter_mut().find(|f| f.reference == reference) { f.state = fe::FRONTEND_STATE_CLOSED; f.session = ZERO; }
        }
        let (sop, fop) = (self.sessions[i].operation, self.sessions[i].foreground.as_ref().map(|f| f.operation));
        self.retire_lane(sop); if let Some(fop) = fop { self.retire_lane(fop); }
        Ok(())
    }
    fn event(&mut self, i: usize, kind: u16, payload: &[u8]) -> Result<Event, Status> {
        let sequence = self.sessions[i].sequence.checked_add(1).ok_or(Status::QuotaExceeded)?;
        let mut bytes = Vec::new(); reserve(&mut bytes, 24 + payload.len())?; bytes.resize(24, 0);
        bytes[..8].copy_from_slice(&sequence.to_le_bytes()); bytes[8..16].copy_from_slice(&self.sessions[i].epoch.to_le_bytes()); bytes[16..18].copy_from_slice(&kind.to_le_bytes()); bytes[18..20].copy_from_slice(&(payload.len() as u16).to_le_bytes()); bytes.extend_from_slice(payload);
        Ok(Event { sequence, bytes, sent: 0 })
    }
    fn control(&mut self, i: usize, kind: u16, payload: &[u8]) -> Result<u64, Status> {
        if kind == 5 {
            if let Some(event) = self.sessions[i].controls.iter_mut().rev().find(|e| e.sent == 0 && e.bytes[16..18] == 5u16.to_le_bytes()) {
                event.bytes[24..].copy_from_slice(payload);
                return Ok(event.sequence);
            }
        }
        if self.sessions[i].controls.len() >= if kind == 8 { CONTROL_LIMIT } else { CONTROL_LIMIT - 1 } { return Err(Status::QuotaExceeded); }
        let event = self.event(i, kind, payload)?; let sequence = event.sequence; self.sessions[i].controls.push_back(event); self.sessions[i].sequence = sequence; Ok(sequence)
    }
    fn write_stream(&mut self, endpoint: Ref, bytes: &[u8]) -> Result<usize, Status> {
        let count = bytes.len().min(512); if count == 0 { return Err(Status::Malformed); }
        if let Some(s) = self.sessions.iter().find(|s| s.writers.contains(&endpoint)) {
            self.kernel_operation = s.foreground.as_ref().filter(|f| f.committed && !f.restored).map(|f| f.operation).unwrap_or(s.operation);
        }
        let mut body = [0;544]; let mut b = Writer::new(&mut body); b.reference(endpoint)?; b.u32(count as u32)?; b.u32(0)?; b.u64(0)?; b.bytes(&bytes[..count])?; let length = b.position();
        let mut result = [0;32]; let len = self.kernel(KernelOp::WriteStream, &body[..length], &mut result)?; let mut r = Reader::new(&result[HEADER_LEN..len]); let written = r.u32()? as usize; reserved(r.u32()?)?; r.finish()?; if written == 0 || written > count { return Err(Status::Malformed); } Ok(written)
    }
    fn admit_events(&mut self, i: usize, bytes: &[u8], count: usize) -> Result<(), Status> {
        if self.sessions[i].capture.is_some() { return self.capture_events(i, bytes, count); }
        if let Some(status) = self.sessions[i].input_failure { return Err(status); }
        if self.sessions[i].draining { return Err(Status::BrokenPipe); }
        if self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() || self.sessions[i].physical_paste_discard { return Err(Status::WouldBlock); }
        let raw_mode = self.sessions[i].mode & 1 != 0;
        let mut prepared = Vec::new(); reserve(&mut prepared, count)?;
        let mut raw = Scrub::<MAX_SERVICE_RECORD_LEN>::new();
        let mut raw_len = 0; let mut offset = 0; let mut interrupts = 0; let mut interrupt_frames = 0;
        let child_op = self.sessions[i].foreground.as_ref().filter(|f| f.committed && !f.restored).map(|f| (f.operation, f.cancelled));
        for _ in 0..count {
            let frame = bytes.get(offset..offset + 24).ok_or(Status::Malformed)?;
            let kind = u16::from_le_bytes([frame[16], frame[17]]);
            let length = u16::from_le_bytes([frame[18], frame[19]]) as usize;
            if frame[..16] != [0; 16] || frame[20..24] != [0; 4] || !(1..=2).contains(&kind) { return Err(Status::Malformed); }
            let payload = bytes.get(offset + 24..offset + 24 + length).ok_or(Status::Malformed)?;
            api::services::session::validate_event(kind, payload)?;
            if kind == 2 { api::services::session::validate_text(payload)?; }
            let ctrl_c = kind == 1 && payload[4..8] == 2u32.to_le_bytes()
                && matches!(u32::from_le_bytes(payload[..4].try_into().map_err(|_| Status::Malformed)?), 67 | 99) && !raw_mode;
            // Canonical Ctrl-C is one cancellation per press; release/repeat are consumed without bytes.
            if ctrl_c && payload[8] != 1 { offset += 24 + length; continue; }
            let interrupt = ctrl_c;
            let mut encoded = Scrub::<512>::new();
            let encoded_len = if kind == 1 {
                api::services::session::encode_key(u32::from_le_bytes(payload[..4].try_into().map_err(|_| Status::Malformed)?),
                    u32::from_le_bytes(payload[4..8].try_into().map_err(|_| Status::Malformed)?), payload[8], &mut encoded)?
            } else { encoded[..payload.len()].copy_from_slice(payload); payload.len() };
            if interrupt { interrupts += 1; if child_op.is_none() { interrupt_frames += 1; prepared.push(self.event(i, 6, &[])?); } }
            else if raw_mode {
                if raw_len + encoded_len > raw.len() { return Err(Status::QuotaExceeded); }
                raw[raw_len..raw_len + encoded_len].copy_from_slice(&encoded[..encoded_len]); raw_len += encoded_len;
            } else { prepared.push(self.event(i, kind, payload)?); }
            offset += 24 + length;
        }
        if offset != bytes.len() { return Err(Status::Malformed); }
        let s = &self.sessions[i];
        s.sequence.checked_add(count as u64).ok_or(Status::QuotaExceeded)?;
        if s.controls.len() + interrupt_frames > CONTROL_LIMIT - 1 || s.typed.len() + prepared.len() - interrupt_frames > INPUT_LIMIT
            || s.raw.len() + raw_len > RAW_LIMIT { return Err(Status::QuotaExceeded); }
        // Every bounded frame and queue slot is reserved before the first effect.
        // One cancellation of the committed child's operation per accepted batch,
        // never a controller-root cancel; its revoked control endpoint carries no frame.
        if let (true, Some((operation, false))) = (interrupts != 0, child_op) {
            self.kernel_operation = operation;
            let mut body = [0; 24]; let mut b = Writer::new(&mut body); b.reference(operation)?; b.u32(2)?; b.u32(0)?;
            let mut result = [0; 24];
            match self.kernel(KernelOp::CancelOperation, &body, &mut result) {
                Ok(_) | Err(Status::Cancelled) => { if let Some(f) = self.sessions[i].foreground.as_mut() { f.cancelled = true; } }
                // The child is already gone (or the operation retired): nothing to cancel, nothing to latch.
                Err(Status::WrongOwner | Status::Stale | Status::InvalidChild | Status::Revoked) => {}
                Err(status) => return Err(status),
            }
        }
        let s = &mut self.sessions[i];
        for mut event in prepared {
            s.sequence += 1; event.sequence = s.sequence; event.bytes[..8].copy_from_slice(&s.sequence.to_le_bytes());
            if event.bytes[16..18] == 6u16.to_le_bytes() { s.controls.push_back(event); } else { s.typed.push_back(event); }
        }
        s.raw.extend(&raw[..raw_len]);
        if raw_mode { s.sequence += count as u64; }
        Ok(())
    }
    fn paste_operation(&mut self, selector: u16, mut r: Reader<'_>, p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        let reference = r.reference()?; let i = self.sessions.iter().position(|s| s.paste.as_ref().is_some_and(|p| p.reference == reference)).ok_or(Status::Stale)?;
        let paste = self.sessions[i].paste.as_ref().ok_or(Status::Stale)?;
        self.kernel_operation = self.sessions[i].operation;
        if paste.frontend != p { return Err(Status::Denied); } self.frontend(i,p,paste.attachment,paste.epoch)?;
        if selector == 15 { let offset = r.u32()? as usize; let count = r.u32()? as usize; let bytes = r.take(count)?; r.finish()?; let paste = self.sessions[i].paste.as_mut().ok_or(Status::Stale)?;
            if count == 0 || count > 512 || offset != paste.bytes.len() || offset.checked_add(count).is_none_or(|end| end > paste.total) { return Err(Status::Malformed); } paste.bytes.extend_from_slice(bytes); w.u32(paste.bytes.len() as u32)?; w.u32(0)?; return Ok(()); }
        r.finish()?; if selector == 17 { self.sessions[i].paste = None; return Ok(()); }
        let paste = self.sessions[i].paste.take().ok_or(Status::Stale)?;
        if paste.bytes.len() != paste.total { self.sessions[i].paste = Some(paste); return Err(Status::Malformed); }
        match self.admit_paste(i, &paste.bytes) {
            Ok(sequence) => { w.u64(sequence)?; Ok(()) }
            Err(status) => { self.sessions[i].paste = Some(paste); Err(status) }
        }
    }
    fn admit_paste(&mut self, i: usize, bytes: &[u8]) -> Result<u64, Status> {
        api::services::session::validate_paste(bytes)?;
        let text = core::str::from_utf8(bytes).map_err(|_| Status::UnsupportedInput)?;
        let mut chunks = [(0usize, 0usize); 130]; let mut count = 0; let mut start = 0;
        while start < text.len() {
            let mut end = (start + 512).min(text.len()); while !text.is_char_boundary(end) { end -= 1; }
            if count == chunks.len() { return Err(Status::QuotaExceeded); }
            chunks[count] = (start, end); count += 1; start = end;
        }
        let s = &self.sessions[i]; let raw_mode = s.mode & 1 != 0;
        s.sequence.checked_add((count + 2) as u64).ok_or(Status::QuotaExceeded)?;
        if raw_mode {
            let extra = if s.mode & 16 != 0 { 12 } else { 0 };
            if s.raw.len() + bytes.len() + extra > RAW_LIMIT { return Err(Status::WouldBlock); }
        } else if s.typed.len() + count + 2 > INPUT_LIMIT { return Err(Status::WouldBlock); }
        let mut events = Vec::new();
        if !raw_mode {
            reserve(&mut events, count + 2)?; events.push(self.event(i, 3, &[])?);
            for &(start, end) in &chunks[..count] { events.push(self.event(i, 2, &bytes[start..end])?); }
            events.push(self.event(i, 4, &[])?);
        }
        let s = &mut self.sessions[i];
        if raw_mode {
            s.sequence += (count + 2) as u64;
            if s.mode & 16 != 0 { s.raw.extend(b"\x1b[200~"); }
            s.raw.extend(bytes);
            if s.mode & 16 != 0 { s.raw.extend(b"\x1b[201~"); }
        } else {
            for mut event in events { s.sequence += 1; event.sequence = s.sequence; event.bytes[..8].copy_from_slice(&s.sequence.to_le_bytes()); s.typed.push_back(event); }
        }
        Ok(s.sequence)
    }
    fn poll_batch(&mut self, i: usize, outcome: usize, cursor: u64, maximum: usize, out: &mut [u8]) -> Result<Option<usize>, Status> {
        let s = &self.sessions[i]; if s.frozen { return Err(Status::WouldBlock); }
        let mut length = 16; let mut count = 0; let mut next = cursor;
        for e in &s.typed { if e.sequence <= cursor { continue; } if count == maximum || length+e.bytes.len()>out.len() { break; }
            if s.controls.iter().any(|c| c.sequence < e.sequence && c.sent != c.bytes.len()) { break; }
            out[length..length+e.bytes.len()].copy_from_slice(&e.bytes); length+=e.bytes.len(); count+=1; next=e.sequence;
        }
        if count==0 { return Ok(None); } out[..8].copy_from_slice(&next.to_le_bytes()); out[8..10].copy_from_slice(&(count as u16).to_le_bytes()); out[10..12].fill(0); out[12..16].copy_from_slice(&((length-16) as u32).to_le_bytes()); self.outcomes[outcome].cursor=next; Ok(Some(length))
    }
    fn invalidate_poll(&mut self, i: usize, status: Status) { if let Some(poll)=self.sessions[i].poll.take() { self.finish(poll.outcome,status,&[]); let o=&mut self.outcomes[poll.outcome]; o.observed|=Self::send(&o.result,0,poll.token); } }
    fn drain(&mut self, i: usize) {
        self.drain_notices(i);
        if self.sessions[i].closed || self.sessions[i].frozen || (self.sessions[i].attachment.is_none() && !self.sessions[i].detached) || self.sessions[i].input_failure.is_some() { return; }
        // One bounded write per ring per turn; no full ring can stall ingress.
        if let Some(n) = self.sessions[i].controls.iter().position(|e| e.sent < e.bytes.len()) {
            let mut bytes = Scrub::<512>::new(); let event = &self.sessions[i].controls[n]; let count = (event.bytes.len() - event.sent).min(512);
            bytes[..count].copy_from_slice(&event.bytes[event.sent..event.sent + count]);
            match self.write_stream(self.sessions[i].writers[1], &bytes[..count]) {
                Ok(written) => self.sessions[i].controls[n].sent += written,
                Err(Status::WouldBlock) => {}
                Err(status) => { self.sessions[i].input_failure = Some(status); self.invalidate_poll(i, status); return; }
            }
        }
        if self.sessions[i].controls.iter().any(|e| e.sent != e.bytes.len()) { return; }
        if !self.sessions[i].raw.is_empty() {
            let mut bytes = Scrub::<512>::new(); let count = self.sessions[i].raw.len().min(512);
            for (destination, byte) in bytes.iter_mut().zip(self.sessions[i].raw.iter()).take(count) { *destination = *byte; }
            match self.write_stream(self.sessions[i].writers[0], &bytes[..count]) {
                Ok(written) => zeroize_deque_front(&mut self.sessions[i].raw, written),
                Err(Status::WouldBlock) => {}
                Err(status) => { self.sessions[i].input_failure = Some(status); self.invalidate_poll(i, status); return; }
            }
        }
        if self.sessions[i].draining && !self.sessions[i].eof && self.sessions[i].raw.is_empty() && self.sessions[i].typed.is_empty() {
            if self.sessions[i].controls.len() >= CONTROL_LIMIT - 1 { return; }
            let Ok(event) = self.event(i, 7, &[]) else { return; };
            let endpoint = self.sessions[i].writers[0];
            self.kernel_operation = self.sessions[i].foreground.as_ref().filter(|f| f.committed && !f.restored).map(|f| f.operation).unwrap_or(self.sessions[i].operation);
            let mut result = [0; 24];
            match self.kernel(KernelOp::CloseEndpoint, &endpoint.to_bytes(), &mut result) {
                Ok(_) => { let s = &mut self.sessions[i]; s.sequence = event.sequence; s.controls.push_back(event); s.eof = true; if s.writers[0] == s.baseline_writers[0] { s.baseline_eof = true; } }
                Err(status) => { self.sessions[i].input_failure = Some(status); self.invalidate_poll(i, status); }
            }
        }
    }
    pub fn maintain(&mut self) {
        let timestamp=now();
        self.maintain_frontends();
        self.maintain_trust();
        for i in 0..self.sessions.len() {
            if self.sessions[i].closed {continue;}
            self.kernel_operation = self.sessions[i].operation;
            let bootstrap = self.sessions[i].bootstrap;
            match self.resolve_bootstrap(bootstrap, 0, ZERO) {
                Ok((2, controller, operation, _, _, _)) if controller == self.sessions[i].controller && operation == self.sessions[i].operation => {}
                Ok(_) | Err(Status::Stale | Status::Revoked | Status::Cancelled | Status::NotFound) => {
                    let sop = self.sessions[i].operation; let fop = self.sessions[i].foreground.as_ref().map(|f| f.operation);
                    if self.close(i).is_ok() { self.drop_outcomes(sop); if let Some(fop) = fop { self.drop_outcomes(fop); } self.prune(); }
                    continue;
                }
                // An unverifiable owner query is not proof of death: retry next turn without altering routing.
                Err(_) => { continue; }
            }
            if self.sessions[i].holder.is_some() && !self.sessions[i].orphaned {
                match self.query_operation(self.sessions[i].holder_operation) {
                    Ok(0) => {}
                    Ok(_) | Err(Status::Stale | Status::Revoked | Status::Cancelled | Status::NotFound) => self.orphan(i),
                    // Unverifiable is not proof of holder death: retry next turn.
                    Err(_) => {}
                }
            }
            // An orphaned session ends its input once Lost(4) is out: EOF to each current reader.
            if self.sessions[i].orphaned && !self.sessions[i].draining { self.sessions[i].draining = true; }
            if self.sessions[i].prepare_deadline != 0 && timestamp >= self.sessions[i].prepare_deadline
                && !self.sessions[i].foreground.as_ref().is_some_and(|f| !f.restored || !f.acknowledged) {
                // The prepared child's five-second reservation lapsed before any receipt was registered.
                self.sessions[i].prepare_deadline = 0; self.sessions[i].frozen = false;
            }
            if let Err(status) = self.reconcile(i) {
                // Only a proven identity mismatch fails closed; transient kernel errors retry next turn.
                if matches!(status, Status::InvalidChild | Status::Malformed) { self.invalidate_poll(i, Status::Revoked); self.sessions[i].frozen = true; }
                continue;
            }
            if self.sessions[i].paste.as_ref().is_some_and(|p| p.expires<=timestamp) {self.sessions[i].paste=None;}
            if self.sessions[i].physical_paste.is_some() && self.sessions[i].physical_paste_expires <= timestamp {
                self.sessions[i].physical_paste = None;
                self.sessions[i].physical_paste_discard = !self.sessions[i].physical_paste_complete;
                self.sessions[i].physical_paste_complete = false;
                self.physical_loss(i, 3);
            }
            if self.sessions[i].physical_paste_complete {
                if let Some(staging) = self.sessions[i].physical_paste.take() {
                    match self.admit_paste(i, &staging) {
                        Ok(_) => self.sessions[i].physical_paste_complete = false,
                        Err(Status::WouldBlock) => self.sessions[i].physical_paste = Some(staging),
                        Err(_) => { self.sessions[i].physical_paste_complete = false; self.physical_loss(i, 3); }
                    }
                }
            }
            if !self.sessions[i].frozen && self.sessions[i].mode & 1 == 0 {
                let mut events = Scrub::<4000>::new();
                match sys::monotonic_ns().and_then(|ns| self.sessions[i].decoder.expire(ns, &mut events)) {
                    Ok(n) if n != 0 => { if self.serial_events(i, &events[..n]).is_err() { self.abort_physical_paste(i); self.physical_loss(i, 3); } }
                    Err(_) => { self.abort_physical_paste(i); self.physical_loss(i, 3); }
                    _ => {}
                }
            }
            self.drain(i);
            if let Some(poll)=self.sessions[i].poll.take() {
                let mut bytes=Scrub::<{ MAX_SERVICE_RECORD_LEN - HEADER_LEN }>::new();
                let status=if self.sessions[i].epoch!=poll.epoch || self.sessions[i].lease!=poll.lease {Some(Err(Status::WrongEpoch))} else {
                    match self.poll_batch(i,poll.outcome,poll.cursor,poll.maximum,&mut bytes) {Ok(Some(n))=>Some(Ok(n)),Err(s)=>Some(Err(s)),Ok(None) if timestamp>=poll.expires || (poll.deadline!=u64::MAX && now_ns()>=poll.deadline)=>Some(Err(Status::TimedOut)),Ok(None)=>None}
                };
                if let Some(status)=status {match status {Ok(n)=>self.finish(poll.outcome,Status::Ok,&bytes[..n]),Err(s)=>self.finish(poll.outcome,s,&[])} let o=&mut self.outcomes[poll.outcome]; o.observed|=Self::send(&o.result,0,poll.token);}
                else {self.sessions[i].poll=Some(poll);}
            }
        }
    }
    pub fn physical_active(&self)->bool {!compositor_registered() && self.serial.is_some_and(|r| self.sessions.iter().any(|s| s.reference==r && !s.closed && s.attachment.is_some()))}
    fn physical_index(&mut self) -> Option<usize> {
        let reference = self.serial?;
        let index = self.index(reference).ok()?;
        self.kernel_operation = self.sessions[index].operation;
        if self.reconcile(index).is_err() { self.physical_loss(index, 2); return None; }
        if self.sessions[index].frozen { self.physical_loss(index, 2); return None; }
        if self.sessions[index].draining { return None; }
        Some(index)
    }
    fn physical_loss(&mut self,i:usize,reason:u32) {if !self.sessions[i].controls.iter().any(|e| e.bytes.get(16..18)==Some(&8u16.to_le_bytes()[..])) {let _=self.control(i,8,&reason.to_le_bytes());}}
    pub fn serial_byte(&mut self,byte:u8)->bool {
        if !self.physical_active(){return false;} let Some(i)=self.physical_index() else{return true;};
        if self.sessions[i].physical_paste_complete { self.physical_loss(i, 1); return true; }
        if self.sessions[i].mode&1!=0 {if self.sessions[i].raw.len()<RAW_LIMIT {self.sessions[i].raw.push_back(byte);} else {self.physical_loss(i,1);} return true;}
        // The kernel relays physical serial bytes unchanged: Enter arrives as CR (the
        // frozen Enter byte) and Ctrl-J as LF, which the decoder reports as Key(j, Control).
        let mut events = Scrub::<4000>::new();
        match sys::monotonic_ns().and_then(|ns| self.sessions[i].decoder.feed(byte, ns, &mut events)) {
            Ok(n) if n != 0 => { if self.serial_events(i, &events[..n]).is_err() { self.abort_physical_paste(i); self.physical_loss(i, 3); } }
            Err(_) => { self.abort_physical_paste(i); self.physical_loss(i, 3); }
            _ => {}
        }
        true
    }
    fn abort_physical_paste(&mut self, i: usize) {
        if self.sessions[i].physical_paste.is_some() { self.sessions[i].physical_paste_discard = true; }
        self.sessions[i].physical_paste = None; self.sessions[i].physical_paste_complete = false;
    }
    fn serial_events(&mut self, i: usize, bytes: &[u8]) -> Result<(), Status> {
        let mut offset = 0;
        while offset < bytes.len() {
            let header = bytes.get(offset..offset + 24).ok_or(Status::Malformed)?;
            let kind = u16::from_le_bytes([header[16], header[17]]);
            let length = u16::from_le_bytes([header[18], header[19]]) as usize;
            let payload = bytes.get(offset + 24..offset + 24 + length).ok_or(Status::Malformed)?;
            match kind {
                4 if self.sessions[i].physical_paste_discard => { self.sessions[i].physical_paste_discard = false; }
                _ if self.sessions[i].physical_paste_discard => {}
                3 => {
                    if self.sessions[i].paste.is_some() || self.sessions[i].physical_paste.is_some() { return Err(Status::WouldBlock); }
                    let mut staging = Vec::new(); reserve(&mut staging, 65536)?;
                    self.sessions[i].physical_paste = Some(Secret(staging)); self.sessions[i].physical_paste_expires = now().checked_add(5000).ok_or(Status::QuotaExceeded)?;
                    self.sessions[i].physical_paste_complete = false;
                }
                2 if self.sessions[i].physical_paste.is_some() => {
                    let staging = self.sessions[i].physical_paste.as_mut().ok_or(Status::Malformed)?;
                    if staging.len() + payload.len() > 65536 { return Err(Status::QuotaExceeded); }
                    staging.extend_from_slice(payload);
                }
                4 => {
                    let staging = self.sessions[i].physical_paste.take().ok_or(Status::Malformed)?;
                    match self.admit_paste(i, &staging) {
                        Ok(_) => {}
                        Err(Status::WouldBlock) => {
                            self.sessions[i].physical_paste = Some(staging);
                            self.sessions[i].physical_paste_complete = true;
                        }
                        Err(status) => return Err(status),
                    }
                }
                1 | 2 if self.sessions[i].physical_paste.is_none() => { self.admit_events(i, &bytes[offset..offset + 24 + length], 1)?; }
                _ => return Err(Status::UnsupportedInput),
            }
            offset += 24 + length;
        }
        Ok(())
    }
    pub fn keyboard(&mut self,key:&api::input::KeyEvent)->bool {
        if !self.physical_active(){return false;} let Some(i)=self.physical_index() else{return true;};
        use api::input::{KeySym as K, KeyState};
        if key.modifiers.0&8!=0 {self.physical_loss(i,3);return true;}
        let mods=(key.modifiers.0&1) as u32|((key.modifiers.0&2) as u32)|((key.modifiers.0&4) as u32);
        let code=match key.keysym {K::Printable=>key.character,K::Return=>0x110001,K::Escape=>0x110002,K::Backspace=>0x110003,K::Tab=>0x110004,K::Delete=>0x110005,K::Insert=>0x110006,K::Up=>0x110010,K::Down=>0x110011,K::Left=>0x110012,K::Right=>0x110013,K::Home=>0x110014,K::End=>0x110015,K::PageUp=>0x110016,K::PageDown=>0x110017,K::F1=>0x110020,K::F2=>0x110021,K::F3=>0x110022,K::F4=>0x110023,K::F5=>0x110024,K::F6=>0x110025,K::F7=>0x110026,K::F8=>0x110027,K::F9=>0x110028,K::F10=>0x110029,K::F11=>0x11002a,K::F12=>0x11002b,_=>0};
        let mut event=Scrub::<40>::new();let len;
        if key.keysym==K::Printable && mods&6==0 {if key.state==KeyState::Released{return true;} let Some(ch)=char::from_u32(code)else{self.physical_loss(i,3);return true;};let mut utf8=Scrub::<4>::new();let text=ch.encode_utf8(&mut utf8);event[16..18].copy_from_slice(&2u16.to_le_bytes());event[18..20].copy_from_slice(&(text.len() as u16).to_le_bytes());event[24..24+text.len()].copy_from_slice(text.as_bytes());len=24+text.len();}
        else {event[16..18].copy_from_slice(&1u16.to_le_bytes());event[18..20].copy_from_slice(&16u16.to_le_bytes());event[24..28].copy_from_slice(&code.to_le_bytes());event[28..32].copy_from_slice(&mods.to_le_bytes());event[32]=key.state as u8;len=40;}
        if let Err(status) = self.admit_events(i,&event[..len],1) { self.physical_loss(i, if matches!(status, Status::QuotaExceeded | Status::WouldBlock) { 1 } else { 3 }); }
        true
    }
}

#[cfg(test)]
mod scrub_tests {
    use super::*;

    const SECRET: &[u8] = b"hunter2-correct-horse";
    fn who() -> Principal { Principal { cell_id: 7, generation: 1 } }
    fn operation() -> Ref { Ref { id: 9, generation: 1 } }
    fn request_bytes(selector: u16, id: u64, body: &[u8]) -> (RequestHeaderV1, Vec<u8>) {
        let mut record = vec![0u8; HEADER_LEN + body.len()];
        let header = RequestHeaderV1 { selector, total_len: record.len() as u32, request_id: id };
        header.encode(&mut record[..HEADER_LEN]).unwrap();
        record[HEADER_LEN..].copy_from_slice(body);
        (header, record)
    }
    fn contains(haystack: &[u8], needle: &[u8]) -> bool { haystack.windows(needle.len()).any(|w| w == needle) }

    #[test]
    fn only_body_carrying_selectors_are_secret_bearing() {
        for selector in 1..=22u16 { assert_eq!(secret_bearing(selector), matches!(selector, 13 | 15 | 20), "selector {selector}"); }
    }

    #[test]
    fn secret_requests_are_sealed_when_consumed_and_still_resolve_replays() {
        for selector in [13u16, 15, 20] {
            let mut sessions = Sessions::new();
            let (header, record) = request_bytes(selector, 1, SECRET);
            let (index, replay) = sessions.record(header, &record, who(), operation()).unwrap();
            assert!(!replay);
            // Immediately after it is consumed, while the request is still executing, the
            // outcome holds only the digest of the exact record: no plaintext at all.
            let outcome = &sessions.outcomes[index];
            assert!(outcome.pending && outcome.sealed);
            assert_eq!(outcome.request, ostd::session::sha256(&record).to_vec());
            assert!(!contains(&outcome.request, SECRET) && !contains(&outcome.request, b"hunter"));
            // The pending request still recognises an identical re-send and refuses a changed one.
            assert_eq!(sessions.record(header, &record, who(), operation()), Ok((index, true)));
            let (_, changed) = request_bytes(selector, 1, b"hunter3-correct-horse");
            assert_eq!(sessions.record(header, &changed, who(), operation()), Err(Status::Malformed));
            sessions.finish(index, Status::Ok, &7u64.to_le_bytes());
            let outcome = &sessions.outcomes[index];
            assert!(outcome.sealed && outcome.request.len() == 32);
            assert!(!contains(&outcome.request, SECRET) && !contains(&outcome.request, b"hunter"));
            // The retained answer is a status plus a sequence/count; the body is not in it.
            assert!(!contains(&outcome.result, b"hunter") && outcome.result.len() == HEADER_LEN + 8);
            // Identical re-send resolves to the retained outcome without re-executing.
            let (again, replay) = sessions.record(header, &record, who(), operation()).unwrap();
            assert!(replay && again == index);
            let mut digest = None;
            assert!(sessions.outcomes[index].matches(&record, &mut digest) && digest.is_some());
            // Same ID, changed body (or changed selector) is Malformed, exactly as with plaintext retention.
            let (_, changed) = request_bytes(selector, 1, b"hunter3-correct-horse");
            assert_eq!(sessions.record(header, &changed, who(), operation()), Err(Status::Malformed));
            let (other_header, other_record) = request_bytes(if selector == 13 { 20 } else { 13 }, 1, SECRET);
            assert_eq!(sessions.record(other_header, &other_record, who(), operation()), Err(Status::Malformed));
            assert!(!sessions.outcomes[index].matches(&other_record, &mut None));
        }
    }

    #[test]
    fn failed_secret_requests_are_sealed_too() {
        let mut sessions = Sessions::new();
        let (header, record) = request_bytes(20, 1, SECRET);
        let (index, _) = sessions.record(header, &record, who(), operation()).unwrap();
        sessions.finish(index, Status::WrongEpoch, &[]);
        assert!(sessions.outcomes[index].sealed && !contains(&sessions.outcomes[index].request, SECRET));
        assert!(sessions.outcomes[index].matches(&record, &mut None));
    }

    #[test]
    fn nonsecret_requests_keep_their_exact_bytes() {
        let mut sessions = Sessions::new();
        let (header, record) = request_bytes(7, 1, b"mode request body");
        let (index, _) = sessions.record(header, &record, who(), operation()).unwrap();
        sessions.finish(index, Status::Ok, &[]);
        assert!(!sessions.outcomes[index].sealed && sessions.outcomes[index].request == record);
        assert!(sessions.outcomes[index].matches(&record, &mut None));
    }

    #[test]
    fn scrub_releases_both_bodies_and_the_replay_is_then_unknown() {
        let mut sessions = Sessions::new();
        let (header, record) = request_bytes(20, 1, SECRET);
        let (index, _) = sessions.record(header, &record, who(), operation()).unwrap();
        sessions.finish(index, Status::Ok, &1u64.to_le_bytes());
        sessions.outcomes[index].scrub();
        let outcome = &sessions.outcomes[index];
        assert!(outcome.request.is_empty() && outcome.result.is_empty());
        assert_eq!((outcome.request.capacity(), outcome.result.capacity()), (0, 0));
        assert!(!outcome.sealed && !outcome.matches(&record, &mut None));
        // The fence stays: the acknowledged ID can no longer be replayed or re-used.
        assert_eq!(sessions.record(header, &record, who(), operation()), Err(Status::UnknownRequest));
    }

    #[test]
    fn closing_or_retiring_an_operation_scrubs_every_retained_body() {
        let mut sessions = Sessions::new();
        for id in 1..=3u64 {
            let (header, record) = request_bytes(13, id, SECRET);
            let (index, _) = sessions.record(header, &record, who(), operation()).unwrap();
            if id != 3 { sessions.finish(index, Status::Ok, &id.to_le_bytes()); } // id 3 stays pending
        }
        // Even the pending request retains only its digest.
        assert!(sessions.outcomes[2].pending && sessions.outcomes[2].sealed && !sessions.outcomes[2].request.is_empty());
        assert!(sessions.outcomes.iter().all(|o| !contains(&o.request, SECRET)));
        sessions.release_closed(&[Some(operation())], 1);
        for (n, outcome) in sessions.outcomes.iter().enumerate() {
            // The close request's own outcome (`keep`) survives for its replay/ack; all others are wiped.
            if n == 1 { assert!(outcome.sealed && !outcome.request.is_empty() && !outcome.result.is_empty()); continue; }
            assert!(outcome.request.is_empty() && outcome.result.is_empty() && outcome.observed, "outcome {n}");
        }
        sessions.drop_outcomes(operation());
        assert!(sessions.outcomes.iter().all(|o| o.request.is_empty() && o.result.is_empty() && !o.pending));
    }

    #[test]
    fn staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped() {
        let mut secret = Secret(Vec::with_capacity(64)); secret.extend_from_slice(SECRET);
        let (pointer, capacity) = (secret.as_ptr(), secret.capacity());
        secret.wipe();
        assert!(secret.is_empty());
        // SAFETY: the allocation is still owned by `secret`; every byte was just overwritten.
        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));

        let mut event = Event { sequence: 1, bytes: SECRET.to_vec(), sent: 0 };
        let (pointer, capacity) = (event.bytes.as_ptr(), event.bytes.capacity());
        event.wipe();
        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));

        let mut paste = Paste { reference: operation(), frontend: who(), attachment: operation(), epoch: 1, expires: 0, total: SECRET.len(), bytes: SECRET.to_vec() };
        let (pointer, capacity) = (paste.bytes.as_ptr(), paste.bytes.capacity());
        paste.wipe();
        assert!(paste.bytes.is_empty());
        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
    }
}
