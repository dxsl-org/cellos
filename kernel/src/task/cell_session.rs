//! CellSessionV1: scheduler-serialized operation, holder and launch authority.
//! Rings belong to pipe.rs; Cell identity belongs exclusively to Scheduler.
//!
//! Every transition below runs under the one SCHEDULER lock, so death, expiry,
//! abort, cancel and commit race through a single canonical owner funnel. The
//! only bookkeeping that is not allocation-free is *reservation*: the final
//! `commit_child` consumes capacity promised during `PrepareChild`.
use alloc::vec::Vec;
use api::cell_session::{ObjectRefV1 as Ref, PrincipalV1 as Principal, ChildRefV1 as Child,
    RequestHeaderV1, ResultHeaderV1, Reader, Writer, Status, HEADER_LEN,
    FRONTEND_SEAL_LEN, GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND};
use api::service_binding::ServiceBinding as Binding;
use super::{scheduler::{Scheduler, KernelAllocationContext}, pipe::{self, PipeHandle, PipeEnd}, child_context};
use types::{CellId, ViError};
const ZERO: Ref = Ref { id: 0, generation: 0 };
const RESULT_MAX: usize = 768;
const PREPARE_TICKS: u64 = 500;
const TICK_NS: u64 = 10_000_000;
/// Marks a result that already carries its own header (retained outcomes).
const RAW: usize = 1 << 31;
const READ: u32 = 1;
const WRITE: u32 = 2;
const DUPLICATE: u32 = 4;
const DELEGATE: u32 = 8;
const CLOSE: u32 = 16;
const MAX_LEDGERS: usize = 1024;
const MAX_OUTCOMES: usize = 64;
/// A principal can hold ledgers only for scopes it was authorised in; this
/// bounds how much of the shared table one principal can occupy.
const MAX_LEDGERS_PER_ACTOR: usize = 96;
/// Live (unpublished) staging children one principal may hold at once; each
/// reserves a CellId and an owner slot until commit, abort or expiry.
const MAX_STAGING_PER_PRINCIPAL: usize = 4;
/// Unpublished children one maintenance pass may abort (each abort scans the
/// receipt and holder tables); the remainder is aborted by later passes.
const ABORT_BUDGET: usize = 8;
/// Dead-owner records one maintenance pass may probe for drainability.
const DRAIN_BUDGET: usize = 4;

type Result<T, E = Status> = core::result::Result<T, E>;

/// Explicit per-principal accounting. Rings charge their creator in `pipe.rs`;
/// every other record is charged here and refunded exactly once on drop, unless
/// the principal's Cell has already retired (`forfeit`): its quota row is then
/// deregistered and its CellId may name a different generation.
struct Charge { cell: usize, bytes: usize }
impl Charge {
    fn reserve(actor: Principal, bytes: usize) -> Result<Self> {
        if !crate::memory::cell_quota::charge(actor.cell_id as usize, bytes) { return Err(Status::QuotaExceeded); }
        Ok(Self { cell: actor.cell_id as usize, bytes })
    }
    fn forfeit(&mut self, cell: usize) { if self.cell == cell { self.bytes = 0; } }
}
impl Drop for Charge { fn drop(&mut self) { crate::memory::cell_quota::refund(self.cell, self.bytes); } }

struct Operation { reference: Ref, owner: Principal, parent: Ref, state: u32, charge: Charge,
    /// Parent-chain length (roots are 0). A parent outlives every child, so the
    /// chain from any operation is intact and exactly `depth` hops long.
    depth: u32,
    /// Id of the cancellation pass that cancelled this operation; subtree
    /// membership within that pass, without any per-hop lookup.
    cancelled_by: u64,
    /// Non-zero (the reason) while a cancellation of this subtree is queued.
    pending: u32 }
#[derive(Clone, Copy)]
struct Endpoint { reference: Ref, holder: Principal, operation: Ref, parent: Ref,
    handle: PipeHandle, rights: u32, state: u32, prepared: Option<Child>, enabled: bool,
    /// Held by the resource issuer (input) as a peer of someone else's ring, not
    /// as an upstream producer stage: losing the consumer must not cancel the
    /// consumer's own operation.
    issuer_held: bool }
/// Endpoint state 4 is a reserved issuer peer: it owns no ring count until
/// CommitChild moves the sole writer count to it.
struct Holder { endpoint: Endpoint, charge: Charge }
#[derive(Clone, Copy)]
struct Intent { reference: Ref, issuer: Binding, writers: [Ref; 2] }
struct Prepared {
    reference: Ref, child: Child, preparer: Principal, operation: Ref,
    deadline: u64, written: usize, context: Vec<u8>, parsed: Option<child_context::Context>,
    publication: Option<super::launch::SessionPublication>,
    state: u32, reason: u32, exit_code: i32, terminal_sequence: u64, kill: bool,
    /// Bound owner receipts: 0 InputForeground, 1 VFS cwd, 2 InputFrontend.
    intent: Option<Intent>, receipts: [Ref; 3], charge: Charge,
}
struct Receipt { reference: Ref, prepared: Ref, child: Child, operation: Ref, issuer: Binding,
    kind: u16, slot: u16, object: Ref, expected_epoch: u64, next_epoch: u64,
    digest: [u8; 32], seal: [u8; 256], seal_len: usize, state: u32,
    bound: bool, producers: [Ref; 2], restored: bool, materialized: bool, charge: Charge }
struct Bootstrap { reference: Ref, controller: Principal, operation: Ref, issuer: Binding,
    endpoints: [Ref; 8], session: Ref, state: u32, deadline: u64, charge: Charge }
struct Outcome { request_id: u64, request: Vec<u8>, result: Vec<u8>, complete: bool,
    observed: bool, charge: Charge }
struct Ledger { actor: Principal, operation: Ref, next: u64, acknowledged: u64, outcomes: Vec<Outcome> }
enum Begin { New, Replay(usize) }
pub(crate) struct State {
    next: u64, terminal_next: u64, operations: Vec<Operation>, endpoints: Vec<Holder>,
    prepared: Vec<Prepared>, receipts: Vec<Receipt>, bootstraps: Vec<Bootstrap>, ledgers: Vec<Ledger>,
    /// Cancellation pass counter, queued cancellations, and the guard that keeps
    /// the cascade a flat worklist instead of recursion.
    pass: u64, pending_cancels: usize, cascading: bool,
    /// Rings whose final reader closed since the last producer pass (`overflow`:
    /// the list could not grow, so every ring is re-checked against its counts).
    dead_rings: Vec<usize>, dead_overflow: bool,
    /// Rotating drain positions so budgeted drains never starve later records.
    drain_operation_cursor: u64, drain_prepared_cursor: usize,
}
fn receipt_service(kind: u16) -> u16 {
    if kind == 1 || kind == GRANT_KIND_INPUT_FRONTEND { api::syscall::service::INPUT } else { api::syscall::service::VFS }
}
/// Kind-6 (InputFrontend) receipt seal: exactly the frontend Ref and the
/// prepared child. The context's slot-8 grant must name that same frontend.
fn check_frontend_seal(seal: &[u8], object: Ref, grant: Option<Ref>, child: Child) -> Result<()> {
    let mut sr = Reader::new(seal);
    let frontend = sr.reference()?; let sealed = sr.child()?; sr.finish()?;
    if sealed != child { return Err(Status::InvalidChild); }
    if frontend == ZERO || frontend != object || grant != Some(frontend) { return Err(Status::Denied); }
    Ok(())
}
fn owner_live(sched: &Scheduler, principal: Principal) -> bool {
    sched.resolve_live_cell_owner(CellId(principal.cell_id), principal.generation).is_some()
}
impl State {
    pub(crate) const fn new() -> Self { Self { next: 1, terminal_next: 1,
        operations: Vec::new(), endpoints: Vec::new(), prepared: Vec::new(), receipts: Vec::new(),
        bootstraps: Vec::new(), ledgers: Vec::new(), pass: 0, pending_cancels: 0, cascading: false,
        dead_rings: Vec::new(), dead_overflow: false, drain_operation_cursor: 0, drain_prepared_cursor: 0 } }
    fn mint(&mut self) -> Result<Ref> {
        let id = self.next;
        self.next = id.checked_add(1).ok_or(Status::QuotaExceeded)?;
        Ok(Ref { id, generation: 1 })
    }
    /// Operations, holders, receipts and bootstraps are appended in mint order
    /// and only ever removed, so each table stays sorted by reference id.
    fn op_index(&self, reference: Ref) -> Result<usize> {
        let i = self.operations.binary_search_by_key(&reference.id, |o| o.reference.id).map_err(|_| Status::Stale)?;
        if self.operations[i].reference == reference { Ok(i) } else { Err(Status::Stale) }
    }
    fn op_member(&self, actor: Principal, operation: Ref) -> Result<usize> {
        let i = self.op_index(operation)?;
        if self.operations[i].owner != actor && !self.endpoints.iter().any(|h| h.endpoint.holder == actor
            && h.endpoint.operation == operation && h.endpoint.prepared.is_none() && h.endpoint.state == 0) {
            return Err(Status::WrongOwner);
        }
        Ok(i)
    }
    fn op_active(&self, actor: Principal, operation: Ref) -> Result<usize> {
        let i = self.op_member(actor, operation)?;
        if self.operations[i].state != 0 { return Err(Status::Cancelled); }
        Ok(i)
    }
    fn endpoint_index(&self, reference: Ref) -> Result<usize> {
        let i = self.endpoints.binary_search_by_key(&reference.id, |h| h.endpoint.reference.id).map_err(|_| Status::Stale)?;
        if self.endpoints[i].endpoint.reference == reference { Ok(i) } else { Err(Status::Stale) }
    }
    /// Resolve an open, enabled, non-staged endpoint held by `actor` that
    /// carries every right in `rights` within a live operation.
    fn endpoint(&self, actor: Principal, reference: Ref, rights: u32) -> Result<Endpoint> {
        let e = self.endpoints[self.endpoint_index(reference)?].endpoint;
        if e.holder != actor { return Err(Status::WrongOwner); }
        if e.prepared.is_some() || !e.enabled { return Err(Status::Denied); }
        match e.state { 0 => {}, 1 => return Err(Status::Stale), 2 => return Err(Status::Revoked),
            _ => return Err(Status::Cancelled) }
        if self.operations[self.op_index(e.operation)?].state != 0 { return Err(Status::Cancelled); }
        if e.rights & rights != rights {
            if rights & 3 != 0 && e.rights & 3 & rights == 0 { return Err(Status::WrongDirection); }
            return Err(Status::Denied);
        }
        Ok(e)
    }
    fn preparation(&self, reference: Ref) -> Result<usize> {
        self.prepared.iter().position(|p| p.reference == reference).ok_or(Status::Stale)
    }
    fn preparing(&self, actor: Principal, reference: Ref) -> Result<usize> {
        let i = self.preparation(reference)?;
        if self.prepared[i].preparer != actor { return Err(Status::WrongOwner); }
        if self.prepared[i].state != 0 { return Err(Status::InvalidChild); }
        Ok(i)
    }
    fn receipt_index(&self, reference: Ref) -> Result<usize> {
        let i = self.receipts.binary_search_by_key(&reference.id, |r| r.reference.id).map_err(|_| Status::Stale)?;
        if self.receipts[i].reference == reference { Ok(i) } else { Err(Status::Stale) }
    }
    fn bootstrap_index(&self, reference: Ref) -> Result<usize> {
        let i = self.bootstraps.binary_search_by_key(&reference.id, |r| r.reference.id).map_err(|_| Status::Stale)?;
        if self.bootstraps[i].reference == reference { Ok(i) } else { Err(Status::Stale) }
    }
    /// At most 64 live holder nodes per operation (retained closed records do
    /// not count), with the vector capacity reserved before any mutation.
    fn holder_capacity(&mut self, operation: Ref, count: usize) -> Result<()> {
        let live = self.endpoints.iter().filter(|h| h.endpoint.operation == operation
            && (h.endpoint.state == 0 || h.endpoint.state == 4)).count();
        if live + count > 64 { return Err(Status::QuotaExceeded); }
        self.endpoints.try_reserve(count).map_err(|_| Status::QuotaExceeded)
    }
    #[allow(clippy::too_many_arguments)]
    fn add_holder(&mut self, charged: Principal, holder: Principal, operation: Ref, handle: PipeHandle,
        rights: u32, parent: Ref, prepared: Option<Child>) -> Result<Ref> {
        self.holder_capacity(operation, 1)?;
        let charge = Charge::reserve(charged, core::mem::size_of::<Holder>())?;
        let reference = self.mint()?;
        self.endpoints.push(Holder { endpoint: Endpoint { reference, holder, operation, handle,
            rights, parent, prepared, enabled: true, state: 0, issuer_held: false }, charge });
        Ok(reference)
    }
    fn create_stream(&mut self, actor: Principal, operation: Ref, capacity: u32) -> Result<[Ref; 2]> {
        self.op_active(actor, operation)?;
        if capacity != 0 && !(64..=65536).contains(&capacity) { return Err(Status::Malformed); }
        self.holder_capacity(operation, 2)?;
        let first = Charge::reserve(actor, core::mem::size_of::<Holder>())?;
        let second = Charge::reserve(actor, core::mem::size_of::<Holder>())?;
        let reader = self.mint()?;
        let writer = self.mint()?;
        let (read, write) = pipe::create(capacity as usize, actor.cell_id as usize).map_err(|_| Status::QuotaExceeded)?;
        for (reference, handle, rights, charge) in [(reader, read, READ|DUPLICATE|DELEGATE|CLOSE, first),
            (writer, write, WRITE|DUPLICATE|DELEGATE|CLOSE, second)] {
            self.endpoints.push(Holder { endpoint: Endpoint { reference, holder: actor, operation,
                parent: ZERO, handle, rights, state: 0, prepared: None, enabled: true, issuer_held: false }, charge });
        }
        Ok([reader, writer])
    }
    fn is_subtree(&self, child: Ref, root: Ref) -> bool {
        if child == root { return true; }
        let (Ok(mut at), Ok(top)) = (self.op_index(child), self.op_index(root)) else { return false; };
        let target = self.operations[top].depth;
        while self.operations[at].depth > target {
            let Ok(parent) = self.op_index(self.operations[at].parent) else { return false; };
            at = parent;
        }
        at == top
    }
    /// Revoke one holder node and run any cancellation it causes (see `close_one`).
    fn close_holder(&mut self, sched: &mut Scheduler, index: usize, state: u32) {
        self.close_one(sched, index, state);
        self.cascade(sched);
    }
    /// Revoke one holder node. A holder with a ring reference releases it; a
    /// staged holder (prepared) or reserved peer (state 4) owns none yet. Any
    /// waiter parked on the ring wakes to recheck its typed state. Losing the
    /// final consumer of a ring only *records* the ring: the producers on it are
    /// gathered in one pass by `close_producers`.
    fn close_one(&mut self, sched: &mut Scheduler, index: usize, state: u32) {
        let e = self.endpoints[index].endpoint;
        if e.state == 4 { self.endpoints[index].endpoint.state = state; return; }
        if e.state != 0 { return; }
        self.endpoints[index].endpoint.state = state;
        if e.prepared.is_some() { return; }
        let final_reader = pipe::session_close(sched, e.handle);
        pipe::session_wake_all(sched, e.handle);
        if e.handle.end == PipeEnd::Read && final_reader && !self.dead_overflow {
            if self.dead_rings.try_reserve(1).is_ok() { self.dead_rings.push(e.handle.pipe); }
            else { self.dead_overflow = true; }
        }
    }
    /// A holder placed inside a bootstrap's own operation by that bootstrap is
    /// shared session state (the controller's retained writers): the operation
    /// holds other sessions and the controller's own holders, so losing the
    /// consumer closes this holder but never cancels the operation.
    fn bootstrap_holder(&self, holder: &Endpoint) -> bool {
        self.bootstraps.iter().any(|b| b.operation == holder.operation && b.endpoints.contains(&holder.reference))
    }
    /// One pass over every holder for the rings whose final reader just closed:
    /// each upstream writer on such a ring is closed, and the operation that
    /// owns a producer's stage is queued for cancellation. Only the holders on
    /// those rings and the stages that produce into them are touched; a bootstrap
    /// root operation and its other sessions are not.
    fn close_producers(&mut self, sched: &mut Scheduler) {
        let overflow = core::mem::take(&mut self.dead_overflow);
        if self.dead_rings.is_empty() && !overflow { return; }
        let mut rings = core::mem::take(&mut self.dead_rings);
        rings.sort_unstable();
        for j in 0..self.endpoints.len() {
            let producer = self.endpoints[j].endpoint;
            if producer.state != 0 || producer.handle.end != PipeEnd::Write || producer.issuer_held { continue; }
            let dead = if overflow {
                pipe::session_counts(producer.handle).is_none_or(|(readers, _)| readers == 0)
            } else { rings.binary_search(&producer.handle.pipe).is_ok() };
            if !dead { continue; }
            self.close_one(sched, j, 2);
            if !self.bootstrap_holder(&producer) { self.request_cancel(producer.operation, 3); }
        }
        rings.clear();
        if self.dead_rings.is_empty() { self.dead_rings = rings; }
    }
    /// Close without cascading to the producer's operation (bootstrap teardown).
    fn close_reservation_holder(&mut self, sched: &mut Scheduler, index: usize) {
        let e = self.endpoints[index].endpoint;
        if e.state == 4 { self.endpoints[index].endpoint.state = 2; return; }
        if e.state == 0 {
            self.endpoints[index].endpoint.state = 2;
            if e.prepared.is_none() {
                pipe::session_close(sched, e.handle);
                pipe::session_wake_all(sched, e.handle);
            }
        }
    }
    fn terminal_sequence(&mut self) -> u64 {
        let value = self.terminal_next;
        if value == 0 { return 0; }
        self.terminal_next = value.checked_add(1).unwrap_or(0);
        value
    }
    /// Abort or expire an unpublished child. The reservation guards drop here,
    /// which refunds every creator claim exactly once and releases the owner
    /// slot, task row, log promise and ready promise without publishing.
    fn abort_prepared(&mut self, sched: &mut Scheduler, index: usize, expired: bool) {
        if self.prepared[index].state != 0 { return; }
        let reference = self.prepared[index].reference;
        let child = self.prepared[index].child;
        if let Some(publication) = self.prepared[index].publication.take() {
            sched.abort_session_owner(publication.owner);
            drop(publication);
        }
        self.prepared[index].state = if expired { 5 } else { 4 };
        self.prepared[index].reason = if expired { 6 } else { 5 };
        let sequence = self.terminal_sequence();
        self.prepared[index].terminal_sequence = sequence;
        self.prepared[index].context = Vec::new();
        self.prepared[index].parsed = None;
        for r in &mut self.receipts {
            if r.prepared == reference && r.state == 0 { r.state = if expired { 3 } else { 2 }; }
        }
        // Reserved issuer peers hold no ring count; releasing them is a state change.
        for i in 0..self.receipts.len() {
            if self.receipts[i].prepared == reference {
                for producer in self.receipts[i].producers {
                    if let Ok(ei) = self.endpoint_index(producer) { self.close_holder(sched, ei, 2); }
                }
            }
        }
        for i in 0..self.endpoints.len() {
            if self.endpoints[i].endpoint.prepared == Some(child) { self.close_holder(sched, i, 2); }
        }
    }
    /// Cancel an operation subtree: mark, revoke every holder, abort staged
    /// children and flag live committed children for the canonical kill funnel.
    fn cancel(&mut self, sched: &mut Scheduler, operation: Ref, reason: u32) {
        self.request_cancel(operation, reason);
        self.cascade(sched);
    }
    /// Queue a subtree cancellation. An operation that is already cancelled has a
    /// fully cancelled subtree (no child or holder can be added to a cancelled
    /// operation), so there is nothing left to do for it.
    fn request_cancel(&mut self, operation: Ref, reason: u32) {
        let Ok(i) = self.op_index(operation) else { return; };
        if self.operations[i].state != 0 || self.operations[i].pending != 0 { return; }
        self.operations[i].pending = reason.max(1);
        self.pending_cancels += 1;
    }
    /// Flat cancellation worklist: consumer loss cascades upstream through
    /// `close_producers` without recursion, one pass per round.
    fn cascade(&mut self, sched: &mut Scheduler) {
        if self.cascading { return; }
        self.cascading = true;
        while self.pending_cancels != 0 || !self.dead_rings.is_empty() || self.dead_overflow {
            for i in 0..self.operations.len() {
                let reason = self.operations[i].pending;
                if reason == 0 { continue; }
                self.operations[i].pending = 0;
                self.pending_cancels -= 1;
                if self.operations[i].state == 0 { self.cancel_subtree(sched, i, reason); }
            }
            self.close_producers(sched);
        }
        self.cascading = false;
    }
    /// Cancel the still-live subtree below operation `root`. Operations are kept
    /// in creation order and a parent always precedes its children, so a single
    /// forward pass finds every descendant; holders and staged children are then
    /// matched by their operation's pass mark.
    fn cancel_subtree(&mut self, sched: &mut Scheduler, root: usize, reason: u32) {
        self.pass += 1;
        let pass = self.pass;
        self.operations[root].state = 1;
        self.operations[root].cancelled_by = pass;
        for i in root + 1..self.operations.len() {
            if self.operations[i].state != 0 || self.operations[i].parent == ZERO { continue; }
            let parent = self.operations[i].parent;
            let marked = {
                let known = &self.operations[root..i];
                known.binary_search_by_key(&parent.id, |o| o.reference.id).ok()
                    .is_some_and(|at| known[at].reference == parent && known[at].cancelled_by == pass)
            };
            if marked { self.operations[i].state = 1; self.operations[i].cancelled_by = pass; }
        }
        for i in 0..self.endpoints.len() {
            let e = &self.endpoints[i].endpoint;
            if e.state != 0 && e.state != 4 { continue; }
            let Ok(oi) = self.op_index(e.operation) else { continue; };
            if self.operations[oi].cancelled_by == pass { self.close_one(sched, i, 3); }
        }
        for i in 0..self.prepared.len() {
            if self.prepared[i].state > 1 { continue; }
            let Ok(oi) = self.op_index(self.prepared[i].operation) else { continue; };
            if self.operations[oi].cancelled_by != pass { continue; }
            if self.prepared[i].state == 0 { self.abort_prepared(sched, i, false); }
            else {
                if self.prepared[i].reason == 0 { self.prepared[i].reason = reason; }
                self.prepared[i].kill = true;
            }
        }
    }
    fn forfeit_charges(&mut self, cell: usize) {
        for o in &mut self.operations { o.charge.forfeit(cell); }
        for h in &mut self.endpoints { h.charge.forfeit(cell); }
        for p in &mut self.prepared { p.charge.forfeit(cell); }
        for r in &mut self.receipts { r.charge.forfeit(cell); }
        for b in &mut self.bootstraps { b.charge.forfeit(cell); }
        for l in &mut self.ledgers { for o in &mut l.outcomes { o.charge.forfeit(cell); } }
    }
    /// Close a bootstrap: abort the session's committed foreground child, then
    /// revoke every bootstrap endpoint. `new_state`: 3 aborted, 4 expired, 5 closed.
    fn close_bootstrap(&mut self, sched: &mut Scheduler, bi: usize, new_state: u32) {
        let session = self.bootstraps[bi].session;
        let endpoints = self.bootstraps[bi].endpoints;
        self.bootstraps[bi].state = new_state;
        if session != ZERO {
            for i in 0..self.receipts.len() {
                let rr = &self.receipts[i];
                if rr.kind == 1 && rr.state == 1 && !rr.restored && read_ref(&rr.seal, 0).ok() == Some(session) {
                    let operation = rr.operation;
                    self.cancel(sched, operation, 2);
                }
            }
        }
        for reference in endpoints {
            if let Ok(ei) = self.endpoint_index(reference) { self.close_reservation_holder(sched, ei); }
        }
    }
}
fn resolve_actor(sched: &Scheduler, tid: usize) -> Result<Principal> {
    let task = sched.tasks.get(&tid).ok_or(Status::Denied)?;
    sched.resolve_live_cell_owner(task.cell_id, task.cell_generation).ok_or(Status::Stale)?;
    Ok(Principal { cell_id: task.cell_id.0, generation: task.cell_generation })
}
fn binding(sched: &Scheduler, service: u16) -> Result<Binding> {
    let (tid, cell, generation) = crate::cell::service_registry::lookup_bound(service).ok_or(Status::Revoked)?;
    let owner = sched.resolve_live_cell_owner(CellId(cell), generation).ok_or(Status::Revoked)?;
    let task = sched.tasks.get(&tid).ok_or(Status::Revoked)?;
    if task.cell_id.0 != owner.cell_id || task.cell_generation != generation { return Err(Status::Revoked); }
    Ok(Binding::new(tid as u64, cell, generation))
}
/// The caller must be the exact currently registered provider of `service`.
fn issuer(sched: &Scheduler, actor: Principal, service: u16) -> Result<Binding> {
    let current = binding(sched, service)?;
    if current.cell_id != actor.cell_id || current.generation != actor.generation { return Err(Status::Denied); }
    Ok(current)
}
fn zero(value: u32) -> Result<()> { if value == 0 { Ok(()) } else { Err(Status::Malformed) } }
fn vi(error: ViError) -> Status { match error { ViError::OutOfMemory => Status::QuotaExceeded,
    ViError::PermissionDenied => Status::Denied, ViError::NotSupported => Status::Unsupported,
    ViError::InvalidInput | ViError::InvalidArgument => Status::Malformed, _ => Status::Io } }

impl State {
    /// Expiry, owner/issuer death and registry drain, all under the owner lock.
    /// Every pass is bounded: the common scans are linear in the record tables,
    /// and the costly per-record work (aborting an unpublished child, probing a
    /// dead owner's records for drainability) is capped per pass, with the rest
    /// left for the next dispatch or tick, so no request or timer tick can be made
    /// to hold SCHEDULER for super-linear time.
    fn maintenance(&mut self, sched: &mut Scheduler) {
        let now = super::system_ticks() as u64;
        let mut aborts = ABORT_BUDGET;
        for i in 0..self.prepared.len() {
            if aborts == 0 { break; }
            if self.prepared[i].state == 0 {
                let expired = now >= self.prepared[i].deadline;
                if expired || !owner_live(sched, self.prepared[i].preparer) {
                    self.abort_prepared(sched, i, expired);
                    aborts -= 1;
                }
            }
        }
        let input = binding(sched, api::syscall::service::INPUT).ok();
        let vfs = binding(sched, api::syscall::service::VFS).ok();
        for i in 0..self.receipts.len() {
            let rr = &self.receipts[i];
            if rr.state > 1 { continue; }
            let current = if receipt_service(rr.kind) == api::syscall::service::INPUT { input } else { vfs };
            if current == Some(rr.issuer) { continue; }
            let (operation, prepared) = (rr.operation, rr.prepared);
            if let Ok(p) = self.preparation(prepared) {
                let current_state = self.prepared[p].state;
                match current_state {
                    0 if aborts != 0 => { self.abort_prepared(sched, p, false); aborts -= 1; }
                    1 => self.cancel(sched, operation, 4),
                    _ => {}
                }
            }
        }
        for i in 0..self.bootstraps.len() {
            let b = &self.bootstraps[i];
            if b.state > 2 { continue; }
            let invalid = input != Some(b.issuer) || !owner_live(sched, b.controller);
            let expired = b.state < 2 && now >= b.deadline;
            if invalid || expired { self.close_bootstrap(sched, i, if invalid { 5 } else { 4 }); }
        }
        // Receipts whose issuer is no longer the registered provider are
        // unreadable by anyone once their child is terminal: drain them so a
        // restarted issuer cannot leave records pinning the shared caps.
        let prepared = &self.prepared;
        self.receipts.retain(|rr| {
            let current = if receipt_service(rr.kind) == api::syscall::service::INPUT { input } else { vfs };
            current == Some(rr.issuer)
                || prepared.iter().find(|p| p.reference == rr.prepared).is_some_and(|p| p.state < 3)
        });
        self.drain_dead(sched, input);
    }
    /// Creator death drains owned records once nothing depends on them. At most
    /// `DRAIN_BUDGET` dead-owner records are probed per pass (each probe scans the
    /// tables), resuming from where the previous pass stopped so none starves.
    fn drain_dead(&mut self, sched: &Scheduler, input: Option<Binding>) {
        let mut budget = DRAIN_BUDGET;
        let total = self.prepared.len();
        let mut i = self.drain_prepared_cursor;
        let mut visited = 0;
        while visited < total && budget != 0 && !self.prepared.is_empty() {
            if i >= self.prepared.len() { i = 0; }
            visited += 1;
            let p = &self.prepared[i];
            if p.state >= 3 && !owner_live(sched, p.preparer) {
                budget -= 1;
                let reference = p.reference;
                if !self.receipts.iter().any(|r| r.prepared == reference) {
                    self.prepared.remove(i);
                    continue;
                }
            }
            i += 1;
        }
        self.drain_prepared_cursor = i;
        // A terminal bootstrap nobody can still read (controller dead, or the
        // issuer that recorded it replaced) holds no further authority.
        self.bootstraps.retain(|b| b.state <= 2 || (owner_live(sched, b.controller) && input == Some(b.issuer)));
        let total = self.operations.len();
        let mut at = self.operations.partition_point(|o| o.reference.id <= self.drain_operation_cursor);
        let mut last = self.drain_operation_cursor;
        let (mut visited, mut budget) = (0, DRAIN_BUDGET);
        while visited < total && budget != 0 && !self.operations.is_empty() {
            if at >= self.operations.len() { at = 0; }
            visited += 1;
            let reference = self.operations[at].reference;
            last = reference.id;
            if !owner_live(sched, self.operations[at].owner) {
                budget -= 1;
                if self.drained(reference) {
                    self.remove_operation(at);
                    continue;
                }
            }
            at += 1;
        }
        self.drain_operation_cursor = last;
    }
    /// Remove an operation, its holder records and every request ledger scoped
    /// to it. Operation ids are never reused, so no fence can reopen.
    fn remove_operation(&mut self, index: usize) {
        let reference = self.operations.remove(index).reference;
        self.endpoints.retain(|h| h.endpoint.operation != reference);
        self.ledgers.retain(|l| l.operation != reference);
        // Receipt-less terminal child records are retained for their preparer
        // (`QueryChild`) until here; nothing else can refer to them.
        self.prepared.retain(|p| p.operation != reference);
    }
    /// A finished child record (terminal 3, aborted 4, expired 5) that no receipt names
    /// any more: only its preparer can read it (`QueryChild`), and it is released with its
    /// operation, so it never blocks closing it. Neither a VFS (cwd) receipt nor an
    /// input foreground receipt pins the record to its issuer once resolved: the issuer's
    /// resolution removes the receipt, never the preparer's view of the child's exit.
    fn retained_terminal(&self, p: &Prepared) -> bool {
        p.state >= 3 && !self.receipts.iter().any(|rr| rr.prepared == p.reference)
    }
    fn drained(&self, operation: Ref) -> bool {
        !self.operations.iter().any(|o| o.parent == operation)
            && !self.prepared.iter().any(|p| p.operation == operation && !self.retained_terminal(p))
            && !self.receipts.iter().any(|rr| rr.operation == operation)
            && !self.bootstraps.iter().any(|b| b.operation == operation && b.state <= 2)
            && !self.endpoints.iter().any(|h| h.endpoint.operation == operation
                && (h.endpoint.state == 0 || h.endpoint.state == 4))
    }
    /// Correlation scope of a request. A ledgered request must come from a
    /// principal already authorised in that scope (an existing ledger proves
    /// it was), checked before any ledger or outcome slot is created, so a
    /// guessed reference can neither mint a ledger nor consume an outcome.
    fn request_operation(&self, sched: &Scheduler, actor: Principal, selector: u16, body: &[u8],
        ledgered: bool) -> Result<Ref> {
        if matches!(selector, 17 | 26) { return Ok(ZERO); }
        let reference = Reader::new(body).reference()?;
        let is_actor = |b: Binding| b.cell_id == actor.cell_id && b.generation == actor.generation;
        let (operation, authorised) = match selector {
            // Zero is the correlation scope only for own-principal root
            // operation creation (and its acknowledgement/query).
            1 | 2 | 8 | 9 | 10 | 20 | 21 | 25 | 28 => {
                if reference == ZERO && !matches!(selector, 1 | 25 | 28) { return Err(Status::Malformed); }
                if !ledgered || reference == ZERO { return Ok(reference); }
                let i = self.op_index(reference)?;
                let member = if selector == 8 {
                    // The owner, or the exact input issuer that holds a receipt on it.
                    self.operations[i].owner == actor
                        || self.receipts.iter().any(|rr| rr.operation == reference && is_actor(rr.issuer))
                } else { self.op_member(actor, reference).is_ok() };
                (reference, member)
            }
            3..=7 | 19 => {
                let e = self.endpoints[self.endpoint_index(reference)?].endpoint;
                (e.operation, e.holder == actor)
            }
            11..=15 | 18 | 23 | 27 => {
                let p = &self.prepared[self.preparation(reference)?];
                let allowed = if selector == 15 {
                    issuer(sched, actor, api::syscall::service::INPUT).is_ok()
                        || issuer(sched, actor, api::syscall::service::VFS).is_ok()
                } else { p.preparer == actor };
                (p.operation, allowed)
            }
            16 | 24 | 29 => {
                let rr = &self.receipts[self.receipt_index(reference)?];
                (rr.operation, is_actor(rr.issuer))
            }
            30 => {
                let child = Reader::new(body).child()?;
                let p = &self.prepared[self.preparation(child.prepared_ref())?];
                (p.operation, p.preparer == actor)
            }
            22 => {
                let b = &self.bootstraps[self.bootstrap_index(reference)?];
                (b.operation, b.controller == actor || is_actor(b.issuer))
            }
            _ => return Err(Status::UnknownOperation),
        };
        if ledgered && !authorised && self.ledger_index(actor, operation).is_none() {
            return Err(Status::WrongOwner);
        }
        Ok(operation)
    }
    fn ledger_index(&self, actor: Principal, operation: Ref) -> Option<usize> {
        self.ledgers.iter().position(|l| l.actor == actor && l.operation == operation)
    }
    fn ledger(&mut self, actor: Principal, operation: Ref) -> Result<usize> {
        if let Some(i) = self.ledger_index(actor, operation) { return Ok(i); }
        if self.ledgers.len() >= MAX_LEDGERS
            || self.ledgers.iter().filter(|l| l.actor == actor).count() >= MAX_LEDGERS_PER_ACTOR {
            return Err(Status::QuotaExceeded);
        }
        self.ledgers.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        self.ledgers.push(Ledger { actor, operation, next: 1, acknowledged: 0, outcomes: Vec::new() });
        Ok(self.ledgers.len()-1)
    }
    /// Scope: authenticated sender generation x operation. A repeated ID replays
    /// its exact retained outcome; only the next monotonic ID is new, and a
    /// charged outcome record is reserved before any effect.
    fn begin_request(&mut self, actor: Principal, operation: Ref, header: RequestHeaderV1,
        request: &[u8], result: &mut [u8]) -> Result<Begin> {
        let li = self.ledger(actor, operation)?;
        let ledger = &mut self.ledgers[li];
        if let Some(outcome) = ledger.outcomes.iter_mut().find(|o| o.request_id == header.request_id) {
            if outcome.request != request { return Err(Status::Malformed); }
            if !outcome.complete { return Err(Status::WouldBlock); }
            result[..outcome.result.len()].copy_from_slice(&outcome.result);
            outcome.observed = true;
            return Ok(Begin::Replay(outcome.result.len()));
        }
        if header.request_id != ledger.next { return Err(Status::UnknownRequest); }
        let next = ledger.next.checked_add(1).ok_or(Status::QuotaExceeded)?;
        if ledger.outcomes.len() >= MAX_OUTCOMES { return Err(Status::QuotaExceeded); }
        ledger.outcomes.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        let charge = Charge::reserve(actor, request.len() + RESULT_MAX + core::mem::size_of::<Outcome>())?;
        let mut saved = Vec::new();
        saved.try_reserve_exact(request.len()).map_err(|_| Status::QuotaExceeded)?;
        saved.extend_from_slice(request);
        let mut response = Vec::new();
        response.try_reserve_exact(RESULT_MAX).map_err(|_| Status::QuotaExceeded)?;
        ledger.outcomes.push(Outcome { request_id: header.request_id, request: saved, result: response,
            complete: false, observed: false, charge });
        ledger.next = next;
        Ok(Begin::New)
    }
    /// Completion records the result; it is not yet *observed*: the caller has
    /// only seen it once the copy-out succeeded (`delivered`), a replay, or a
    /// QueryRequest.
    fn finish_request(&mut self, actor: Principal, operation: Ref, id: u64, result: &[u8]) {
        let Some(li) = self.ledger_index(actor, operation) else { return; };
        if let Some(o) = self.ledgers[li].outcomes.iter_mut().find(|o| o.request_id == id) {
            if result.len() <= o.result.capacity() { o.result.extend_from_slice(result); }
            o.complete = true;
        }
    }
    /// The reply bytes of exactly this completed request reached the caller.
    fn mark_observed(&mut self, actor: Principal, request: &[u8], id: u64) {
        for l in self.ledgers.iter_mut().filter(|l| l.actor == actor) {
            for o in l.outcomes.iter_mut() {
                if o.request_id == id && o.complete && o.request == request { o.observed = true; }
            }
        }
    }
    /// True while `principal` is a published child still (or just) alive; its
    /// legacy stdio is session-routed and never falls back to the console.
    fn session_child(&self, principal: Principal) -> bool {
        self.prepared.iter().any(|p| p.child.principal == principal && (1..=3).contains(&p.state))
    }
}
fn read_ref(bytes: &[u8], offset: usize) -> Result<Ref> {
    Ref::from_bytes(bytes.get(offset..offset+16).ok_or(Status::Malformed)?)
}

impl State {
    /// QueryRequest: the original result bytes, never a re-execution.
    fn query_request(&mut self, actor: Principal, body: &[u8], out: &mut [u8]) -> Result<usize> {
        let mut r = Reader::new(body);
        let operation = r.reference()?; let id = r.u64()?; r.finish()?;
        let li = self.ledger_index(actor, operation).ok_or(Status::UnknownRequest)?;
        let o = self.ledgers[li].outcomes.iter_mut().find(|o| o.request_id == id).ok_or(Status::UnknownRequest)?;
        if !o.complete { return Err(Status::WouldBlock); }
        out[..o.result.len()].copy_from_slice(&o.result);
        o.observed = true;
        Ok(o.result.len() | RAW)
    }
    fn execute(&mut self, sched: &mut Scheduler, actor: Principal, selector: u16,
        body: &[u8], out: &mut [u8]) -> Result<usize> {
        if selector == 28 { return self.query_request(actor, body, out); }
        let mut r = Reader::new(body);
        let mut w = Writer::new(out);
        match selector {
            1 => {
                let parent = r.reference()?; r.finish()?;
                let mut depth = 0;
                if parent != ZERO {
                    let pi = self.op_active(actor, parent)?;
                    depth = self.operations[pi].depth.checked_add(1).ok_or(Status::QuotaExceeded)?;
                    if self.operations[pi].owner != actor && !self.endpoints.iter().any(|h|
                        h.endpoint.holder == actor && h.endpoint.operation == parent && h.endpoint.state == 0
                        && h.endpoint.rights & DELEGATE != 0 && h.endpoint.prepared.is_none()) {
                        return Err(Status::Denied);
                    }
                    if self.operations.iter().filter(|o| o.parent == parent).count() >= 32 {
                        return Err(Status::QuotaExceeded);
                    }
                }
                if self.operations.len() >= 512 || self.operations.iter().filter(|o| o.owner == actor).count() >= 64 {
                    return Err(Status::QuotaExceeded);
                }
                let charge = Charge::reserve(actor, core::mem::size_of::<Operation>())?;
                self.operations.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
                let reference = self.mint()?;
                self.operations.push(Operation { reference, owner: actor, parent, state: 0, charge,
                    depth, cancelled_by: 0, pending: 0 });
                w.reference(reference)?;
            }
            2 => {
                let operation = r.reference()?; let capacity = r.u32()?; zero(r.u32()?)?; r.finish()?;
                let [reader, writer] = self.create_stream(actor, operation, capacity)?;
                w.reference(reader)?; w.reference(writer)?;
            }
            3 | 4 => {
                let source = r.reference()?;
                let target = if selector == 4 { Some(r.child()?) } else { None };
                let rights = r.u32()?; zero(r.u32()?)?; r.finish()?;
                if rights == 0 || rights & !31 != 0 { return Err(Status::Malformed); }
                let e = self.endpoint(actor, source, if selector == 3 { DUPLICATE } else { DELEGATE })?;
                if rights & !e.rights != 0 || rights & 3 != e.rights & 3 { return Err(Status::Denied); }
                let (holder, operation) = if let Some(child) = target {
                    let pi = self.preparing(actor, child.prepared_ref())?;
                    if self.prepared[pi].child != child || !self.is_subtree(self.prepared[pi].operation, e.operation) {
                        return Err(Status::InvalidChild);
                    }
                    (child.principal, self.prepared[pi].operation)
                } else { (actor, e.operation) };
                let reference = self.add_holder(actor, holder, operation, e.handle, rights, source, target)?;
                if target.is_none() && !pipe::duplicate_end(e.handle) {
                    self.endpoints.pop();
                    return Err(Status::Stale);
                }
                w.reference(reference)?;
            }
            7 => {
                let reference = r.reference()?; r.finish()?;
                let i = self.endpoint_index(reference)?;
                let e = self.endpoints[i].endpoint;
                if e.holder != actor { return Err(Status::WrongOwner); }
                if e.rights & CLOSE == 0 || e.prepared.is_some() { return Err(Status::Denied); }
                self.close_holder(sched, i, 1);
            }
            8 => {
                let operation = r.reference()?; let reason = r.u32()?; zero(r.u32()?)?; r.finish()?;
                let i = self.op_index(operation)?;
                if self.operations[i].owner != actor {
                    // The exact registered input issuer may cut the committed
                    // foreground child's own operation, and nothing else.
                    let input = issuer(sched, actor, api::syscall::service::INPUT)?;
                    if !self.receipts.iter().any(|rr| rr.kind == 1 && rr.state == 1 && !rr.restored
                        && rr.operation == operation && rr.issuer == input
                        && self.preparation(rr.prepared).ok().is_some_and(|p| self.prepared[p].state == 1)) {
                        return Err(Status::WrongOwner);
                    }
                }
                self.cancel(sched, operation, if reason == 3 { 3 } else { 2 });
            }
            9 => {
                let operation = r.reference()?; r.finish()?;
                // Members, and the exact input issuer probing a frontend
                // holder's operation lifetime (read-only).
                let i = match self.op_member(actor, operation) {
                    Err(Status::WrongOwner) if issuer(sched, actor, api::syscall::service::INPUT).is_ok() => self.op_index(operation)?,
                    other => other?,
                };
                w.u32(self.operations[i].state)?;
                w.u32(self.endpoints.iter().filter(|h| h.endpoint.operation == operation && h.endpoint.state == 0).count() as u32)?;
                w.u32(self.prepared.iter().filter(|p| p.operation == operation && p.state <= 2).count() as u32)?;
                w.u32(0)?;
            }
            11 => self.write_context(sched, actor, r, &mut w)?,
            12 => {
                let reference = r.reference()?; let kind = r.u16()?; let slot = r.u16()?; zero(r.u32()?)?;
                let owner = r.binding()?; let receipt = r.reference()?; r.finish()?;
                let pi = self.preparing(actor, reference)?;
                let ri = self.receipt_index(receipt)?;
                let rr = &self.receipts[ri];
                if rr.prepared != reference || rr.kind != kind || rr.slot != slot || rr.issuer != owner || rr.state != 0 {
                    return Err(Status::Denied);
                }
                let index = match kind { 1 => 0, 2 => 1, GRANT_KIND_INPUT_FRONTEND => 2, _ => return Err(Status::Unsupported) };
                // The receipt must still be the exact live issuer's.
                if binding(sched, receipt_service(kind))? != owner { return Err(Status::Revoked); }
                if self.prepared[pi].receipts[index] != ZERO && self.prepared[pi].receipts[index] != receipt { return Err(Status::Denied); }
                self.prepared[pi].receipts[index] = receipt;
                self.receipts[ri].bound = true;
            }
            13 => {
                let reference = r.reference()?; r.finish()?;
                let pi = self.preparing(actor, reference)?;
                self.commit_child(sched, pi)?;
                w.child(self.prepared[pi].child)?;
            }
            14 => {
                let reference = r.reference()?; r.finish()?;
                let pi = self.preparation(reference)?;
                if self.prepared[pi].preparer != actor { return Err(Status::WrongOwner); }
                if (1..=3).contains(&self.prepared[pi].state) { return Err(Status::InvalidChild); }
                self.abort_prepared(sched, pi, false);
            }
            15 => self.register_receipt(sched, actor, r, &mut w)?,
            16 => {
                let reference = r.reference()?; let action = r.u32()?; zero(r.u32()?)?; r.finish()?;
                let ri = self.receipt_index(reference)?;
                let kind = self.receipts[ri].kind;
                if issuer(sched, actor, receipt_service(kind))? != self.receipts[ri].issuer { return Err(Status::WrongOwner); }
                if action > 2 { return Err(Status::Malformed); }
                if action == 1 && self.receipts[ri].state == 0 {
                    let pi = self.preparation(self.receipts[ri].prepared)?;
                    self.abort_prepared(sched, pi, false);
                }
                let (state, child, producers) = {
                    let rr = &self.receipts[ri];
                    (rr.state, rr.child, if rr.state == 1 { rr.producers } else { [ZERO; 2] })
                };
                if action == 2 {
                    self.receipts[ri].materialized = true;
                    let operation = self.receipts[ri].operation;
                    self.reclaim(operation);
                }
                w.u32(state)?; w.u32(0)?; w.child(child)?;
                for producer in producers { w.reference(producer)?; }
            }
            17 => {
                let offset = r.u32()? as usize; let max = r.u32()? as usize; r.finish()?;
                if !(1..=512).contains(&max) { return Err(Status::Malformed); }
                let owner = sched.resolve_live_cell_owner(CellId(actor.cell_id), actor.generation).ok_or(Status::Stale)?;
                let bytes = sched.tasks.get(&(owner.root_tid as usize)).and_then(|t| t.child_context.as_ref()).ok_or(Status::Denied)?;
                if offset > bytes.len() { return Err(Status::Malformed); }
                let count = max.min(bytes.len()-offset);
                w.u32(count as u32)?; w.u32(bytes.len() as u32)?; w.bytes(&bytes[offset..offset+count])?;
            }
            18 => self.inspect_grant(sched, actor, r, &mut w)?,
            19 => {
                let reference = r.reference()?; r.finish()?;
                let e = self.endpoints[self.endpoint_index(reference)?].endpoint;
                if e.holder != actor && issuer(sched, actor, api::syscall::service::INPUT).is_err() { return Err(Status::WrongOwner); }
                w.principal(e.holder)?; w.reference(e.operation)?; w.u32(e.rights)?;
                // A reserved issuer peer is not yet a revoked node.
                w.u32(if e.state == 4 { 0 } else { e.state })?;
                w.u32(if e.handle.end == PipeEnd::Read { 0 } else { 1 })?; w.u32(0)?;
            }
            20 => {
                let operation = r.reference()?; r.finish()?;
                let i = self.op_index(operation)?;
                if self.operations[i].owner != actor { return Err(Status::WrongOwner); }
                if self.endpoints.iter().any(|h| h.endpoint.operation == operation
                        && (h.endpoint.state == 0 || h.endpoint.state == 4))
                    || self.operations.iter().any(|o| o.parent == operation)
                    || self.prepared.iter().any(|p| p.operation == operation && !self.retained_terminal(p))
                    || self.receipts.iter().any(|rr| rr.operation == operation)
                    || self.bootstraps.iter().any(|b| b.operation == operation && b.state <= 2) { return Err(Status::WouldBlock); }
                self.remove_operation(i);
            }
            21 => self.bootstrap(sched, actor, r, &mut w)?,
            22 => self.resolve_bootstrap(sched, actor, r, &mut w)?,
            23 => {
                let reference = r.reference()?; r.finish()?;
                issuer(sched, actor, api::syscall::service::VFS)?;
                let p = &self.prepared[self.preparation(reference)?];
                if p.state != 0 { return Err(Status::InvalidChild); }
                w.child(p.child)?; w.reference(p.operation)?; w.principal(p.preparer)?;
            }
            24 => {
                let reference = r.reference()?; r.finish()?;
                let rr = &self.receipts[self.receipt_index(reference)?];
                if issuer(sched, actor, receipt_service(rr.kind))? != rr.issuer { return Err(Status::WrongOwner); }
                let p = &self.prepared[self.preparation(rr.prepared)?];
                w.child(p.child)?; w.u32(p.state)?; w.u32(p.reason)?; w.i32(p.exit_code)?; w.u32(0)?;
                w.u64(p.terminal_sequence)?;
            }
            30 => {
                let child = r.child()?; r.finish()?;
                let p = &self.prepared[self.preparation(child.prepared_ref())?];
                if p.preparer != actor { return Err(Status::WrongOwner); }
                if p.child != child { return Err(Status::Stale); }
                if p.state == 0 { return Err(Status::InvalidChild); }
                w.u32(p.state)?; w.u32(p.reason)?; w.i32(p.exit_code)?; w.u32(0)?;
                w.u64(p.terminal_sequence)?;
            }
            25 => {
                let operation = r.reference()?; let through = r.u64()?; r.finish()?;
                let li = self.ledger_index(actor, operation).ok_or(Status::UnknownRequest)?;
                let l = &mut self.ledgers[li];
                if through <= l.acknowledged { return Ok(0); }
                if through >= l.next || l.outcomes.iter().any(|o| o.request_id <= through && (!o.complete || !o.observed)) {
                    return Err(Status::UnknownRequest);
                }
                l.outcomes.retain(|o| o.request_id > through);
                l.acknowledged = through;
                self.reclaim(operation);
            }
            26 => {
                r.finish()?;
                let rr = self.receipts.iter().find(|rr| rr.kind == 1 && rr.state == 1 && !rr.restored
                    && rr.child.principal == actor).ok_or(Status::Denied)?;
                w.reference(read_ref(&rr.seal, 0)?)?; w.reference(read_ref(&rr.seal, 16)?)?;
                w.u64(rr.next_epoch)?; w.reference(read_ref(&rr.seal, 144)?)?; w.binding(rr.issuer)?;
            }
            27 => {
                let reference = r.reference()?; let input = r.binding()?;
                let writers = [r.reference()?, r.reference()?]; r.finish()?;
                let pi = self.preparing(actor, reference)?;
                if binding(sched, api::syscall::service::INPUT)? != input { return Err(Status::Revoked); }
                if self.prepared[pi].parsed.is_none() || self.prepared[pi].intent.is_some() || writers[0] == writers[1] {
                    return Err(Status::Denied);
                }
                let mut rings = [0usize; 2];
                for (n, writer) in writers.iter().enumerate() {
                    let e = self.endpoint(actor, *writer, WRITE|DELEGATE)?;
                    if e.operation != self.prepared[pi].operation || pipe::session_counts(e.handle).map(|p| p.1) != Some(1) {
                        return Err(Status::Denied);
                    }
                    rings[n] = e.handle.pipe;
                }
                if rings[0] == rings[1] { return Err(Status::Denied); }
                let intent = self.mint()?;
                self.prepared[pi].intent = Some(Intent { reference: intent, issuer: input, writers });
                w.reference(intent)?;
            }
            29 => self.restore(sched, actor, r, &mut w)?,
            _ => return Err(Status::UnknownOperation),
        }
        Ok(w.position())
    }
    /// WriteChildContext: strictly sequential, no holes, no overwrite. The
    /// completed context is validated once; any invalid final context aborts the
    /// preparation (no byte of it can be rewritten).
    fn write_context(&mut self, sched: &mut Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let reference = r.reference()?; let offset = r.u32()? as usize; let count = r.u32()? as usize;
        let bytes = r.take(count)?; r.finish()?;
        if !(1..=512).contains(&count) { return Err(Status::Malformed); }
        let p = self.preparing(actor, reference)?;
        let end = offset.checked_add(count).ok_or(Status::Malformed)?;
        if offset != self.prepared[p].written || end > self.prepared[p].context.len() { return Err(Status::Malformed); }
        self.prepared[p].context[offset..end].copy_from_slice(bytes);
        self.prepared[p].written = end;
        if end == self.prepared[p].context.len() {
            if let Err(status) = self.install_context(p) {
                self.abort_prepared(sched, p, false);
                return Err(status);
            }
        }
        w.u32(end as u32)?; w.u32(0)
    }
    fn install_context(&mut self, p: usize) -> Result<()> {
        let parsed = child_context::validate(&self.prepared[p].context, self.prepared[p].operation)?;
        let argv = &self.prepared[p].context[parsed.argv.clone()];
        let argv = if argv.is_empty() { None } else {
            let mut copy = Vec::new();
            copy.try_reserve_exact(argv.len()).map_err(|_| Status::QuotaExceeded)?;
            copy.extend_from_slice(argv);
            Some(copy)
        };
        let cwd = core::str::from_utf8(&self.prepared[p].context[parsed.cwd.clone()]).map_err(|_| Status::UnsupportedInput)?;
        let mut cwd_copy = alloc::string::String::new();
        cwd_copy.try_reserve_exact(cwd.len()).map_err(|_| Status::QuotaExceeded)?;
        cwd_copy.push_str(cwd);
        self.prepared[p].parsed = Some(parsed);
        // Staged kernel grants must already exist, match and be delegated by
        // this preparer; an invalid grant is never silently dropped.
        self.staged_grants(p)?;
        let mut stdio = [ZERO; 4];
        {
            let parsed = self.prepared[p].parsed.as_ref().ok_or(Status::InvalidChild)?;
            for (slot, reference) in stdio.iter_mut().enumerate() {
                *reference = parsed.grants[slot].map_or(ZERO, |g| g.object);
            }
        }
        let publication = self.prepared[p].publication.as_mut().ok_or(Status::InvalidChild)?;
        publication.task.inherited_argv = argv;
        publication.task.cwd = cwd_copy;
        publication.task.child_stdio = stdio;
        Ok(())
    }
    /// Every kernel-endpoint grant (kind 1: stdio slots 0..=3 and the slot-5
    /// completion writer) must name a holder staged for this child by its
    /// preparer's delegation; every staged holder must be named.
    fn staged_grants(&self, pi: usize) -> Result<[Option<usize>; child_context::MAX_GRANTS]> {
        let p = &self.prepared[pi];
        let parsed = p.parsed.as_ref().ok_or(Status::WouldBlock)?;
        let mut staged = [None; child_context::MAX_GRANTS];
        for slot in 0..child_context::MAX_GRANTS {
            let Some(grant) = parsed.grants[slot] else { continue };
            if grant.kind != 1 { continue; }
            let ei = self.endpoint_index(grant.object)?;
            let e = self.endpoints[ei].endpoint;
            if e.holder != p.child.principal || e.prepared != Some(p.child) || e.state != 0
                || e.operation != p.operation || e.rights != grant.rights as u32 { return Err(Status::Denied); }
            let source = self.endpoint(p.preparer, e.parent, DELEGATE)?;
            if source.handle != e.handle || e.rights & !source.rights != 0 { return Err(Status::Denied); }
            staged[slot] = Some(ei);
        }
        if self.endpoints.iter().any(|h| h.endpoint.prepared == Some(p.child) && h.endpoint.state == 0
            && !staged.iter().flatten().any(|i| self.endpoints[*i].endpoint.reference == h.endpoint.reference)) {
            return Err(Status::Denied);
        }
        Ok(staged)
    }
}

impl State {
    fn inspect_grant(&self, sched: &Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let reference = r.reference()?; let kind = r.u16()?; let slot = r.u16()?;
        let offset = r.u32()? as usize; let max = r.u32()? as usize; zero(r.u32()?)?; r.finish()?;
        if !(1..=512).contains(&max) { return Err(Status::Malformed); }
        let p = &self.prepared[self.preparation(reference)?];
        if p.state != 0 { return Err(Status::InvalidChild); }
        let parsed = p.parsed.as_ref().ok_or(Status::WouldBlock)?;
        let stdio;
        let mut identity = [0u8; 32];
        let (grant, projection): (&child_context::Grant, &[u8]) = match (kind, slot) {
            (1, 0) => {
                let intent = p.intent.ok_or(Status::Denied)?;
                if issuer(sched, actor, api::syscall::service::INPUT)? != intent.issuer { return Err(Status::Denied); }
                stdio = parsed.stdio_projection()?;
                (parsed.grants[0].as_ref().ok_or(Status::Denied)?, &stdio)
            }
            (2, 4) => {
                issuer(sched, actor, api::syscall::service::VFS)?;
                (parsed.grants[4].as_ref().ok_or(Status::Denied)?, &p.context[parsed.cwd.clone()])
            }
            (GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND) => {
                // Frontend binding: the input issuer proves the child is prepared
                // by the frontend holder under an operation the holder owns.
                issuer(sched, actor, api::syscall::service::INPUT)?;
                let grant = parsed.grants[SLOT_FRONTEND as usize].as_ref().ok_or(Status::Denied)?;
                let owner = self.operations[self.op_index(p.operation)?].owner;
                identity[..16].copy_from_slice(&p.preparer.to_bytes());
                identity[16..].copy_from_slice(&owner.to_bytes());
                (grant, &identity)
            }
            _ => return Err(Status::Denied),
        };
        if offset > projection.len() { return Err(Status::Malformed); }
        let count = max.min(projection.len()-offset);
        w.child(p.child)?; w.reference(p.operation)?; w.bytes(&parsed.digest)?; w.bytes(&grant.wire)?;
        w.u32(projection.len() as u32)?; w.u32(offset as u32)?; w.u32(count as u32)?; w.u32(0)?;
        w.bytes(&projection[offset..offset+count])
    }
    fn register_receipt(&mut self, sched: &Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let reference = r.reference()?; let kind = r.u16()?; let slot = r.u16()?; zero(r.u32()?)?;
        let object = r.reference()?; let expected_epoch = r.u64()?; let next_epoch = r.u64()?;
        let digest: [u8; 32] = r.take(32)?.try_into().map_err(|_| Status::Malformed)?;
        let seal_len = r.u32()? as usize; zero(r.u32()?)?;
        let seal = r.take(seal_len)?; r.finish()?;
        let pi = self.preparation(reference)?;
        let p = &self.prepared[pi];
        if p.state != 0 { return Err(Status::InvalidChild); }
        let parsed = p.parsed.as_ref().ok_or(Status::WouldBlock)?;
        if digest != parsed.digest { return Err(Status::Denied); }
        if self.receipts.iter().any(|rr| rr.prepared == reference && rr.kind == kind && rr.state <= 1) {
            return Err(Status::Denied);
        }
        let service = match (kind, slot, seal_len) {
            (1, 0, 192) => api::syscall::service::INPUT,
            (2, 4, 128) => api::syscall::service::VFS,
            (GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND, FRONTEND_SEAL_LEN) => api::syscall::service::INPUT,
            _ => return Err(Status::Malformed),
        };
        let issuer = issuer(sched, actor, service)?;
        if kind == 1 {
            let intent = p.intent.ok_or(Status::Denied)?;
            if intent.issuer != issuer { return Err(Status::Denied); }
            let mut sr = Reader::new(seal);
            let session = sr.reference()?; let lease = sr.reference()?; let operation = sr.reference()?;
            let child = sr.child()?; let expected = sr.u64()?; let next = sr.u64()?;
            let streams = [sr.reference()?, sr.reference()?, sr.reference()?, sr.reference()?];
            let writers = [sr.reference()?, sr.reference()?]; sr.finish()?;
            if session == ZERO || lease != object || child != p.child || operation != p.operation
                || expected != expected_epoch || next != next_epoch || next != expected.checked_add(1).ok_or(Status::WrongEpoch)?
                || writers != intent.writers { return Err(Status::Denied); }
            // The seal's session must be the bootstrap-bound SessionRef, and the
            // child's operation must descend from the bootstrap's root through
            // kernel-owned operation links, never a payload claim.
            let b = self.bootstraps.iter().find(|b| b.state == 2 && b.session == session && b.issuer == issuer
                && b.controller == p.preparer).ok_or(Status::WrongSession)?;
            if !self.is_subtree(p.operation, b.operation) || p.operation == b.operation { return Err(Status::Denied); }
            // Foreground exclusivity and monotonic epochs per session.
            for rr in self.receipts.iter().filter(|rr| rr.kind == 1 && read_ref(&rr.seal, 0).ok() == Some(session)) {
                if (rr.state == 0 || (rr.state == 1 && !rr.restored)) && rr.prepared != reference {
                    return Err(Status::WrongEpoch);
                }
                if rr.state == 1 && expected < rr.next_epoch { return Err(Status::WrongEpoch); }
            }
            for (slot, stream) in streams.iter().enumerate() {
                let grant = parsed.grants[slot].ok_or(Status::Denied)?;
                let endpoint = self.endpoints[self.endpoint_index(*stream)?].endpoint;
                if grant.object != *stream || endpoint.holder != p.child.principal || endpoint.prepared != Some(p.child)
                    || endpoint.operation != p.operation || endpoint.rights != grant.rights as u32 || endpoint.state != 0 {
                    return Err(Status::Denied);
                }
            }
            let ring = |reference| -> Result<usize> {
                Ok(self.endpoints[self.endpoint_index(reference)?].endpoint.handle.pipe)
            };
            // stdout/stderr continue on the session's rings; stdin/control are
            // new exclusive rings distinct from the baseline and every earlier epoch.
            if ring(streams[1])? != ring(b.endpoints[1])? || ring(streams[2])? != ring(b.endpoints[2])?
                || ring(streams[0])? == ring(streams[3])?
                || [streams[0], streams[3]].iter().any(|new| [b.endpoints[0], b.endpoints[3]]
                    .iter().any(|old| ring(*new).ok() == ring(*old).ok())) {
                return Err(Status::WrongSession);
            }
            for previous in self.receipts.iter().filter(|rr| rr.kind == 1 && read_ref(&rr.seal, 0).ok() == Some(session)) {
                for offset in [96, 144] {
                    if let Ok(old) = read_ref(&previous.seal, offset).and_then(ring) {
                        if old == ring(streams[0])? || old == ring(streams[3])? { return Err(Status::WrongEpoch); }
                    }
                }
            }
            for (n, writer) in writers.iter().enumerate() {
                let e = self.endpoint(p.preparer, *writer, WRITE|DELEGATE)?;
                let target = self.endpoints[self.endpoint_index(streams[if n == 0 { 0 } else { 3 }])?].endpoint;
                if e.handle.pipe != target.handle.pipe || e.operation != p.operation
                    || pipe::session_counts(e.handle) != Some((1, 1)) { return Err(Status::Denied); }
            }
        } else if kind == GRANT_KIND_INPUT_FRONTEND {
            // No producer peers and no session side effect: the receipt only
            // admits the slot-8 grant of this exact child at CommitChild.
            if expected_epoch != 0 || next_epoch != 0 { return Err(Status::WrongEpoch); }
            check_frontend_seal(seal, object, parsed.grants[SLOT_FRONTEND as usize].map(|g| g.object), p.child)?;
            // One live receipt per frontend: a frontend binds at most one child.
            if self.receipts.iter().any(|rr| rr.kind == GRANT_KIND_INPUT_FRONTEND && rr.object == object
                && rr.issuer == issuer && rr.state <= 1) { return Err(Status::Denied); }
        } else {
            if expected_epoch != 0 || next_epoch != 0 { return Err(Status::WrongEpoch); }
            let mut sr = Reader::new(seal);
            let source = sr.principal()?; let source_grant = sr.reference()?; let child = sr.child()?;
            let operation = sr.reference()?; let cwd_digest = sr.take(32)?; let child_grant = sr.reference()?; sr.finish()?;
            let grant = parsed.grants[4].ok_or(Status::Denied)?;
            if child_grant.generation != child.principal.generation { return Err(Status::InvalidChild); }
            if source != p.preparer || source_grant == ZERO || child != p.child || operation != p.operation
                || child_grant != grant.object || object != child_grant
                || cwd_digest != crate::sha256::sha256(&p.context[parsed.cwd.clone()]) {
                return Err(Status::Denied);
            }
        }
        if self.receipts.len() >= 1024 { return Err(Status::QuotaExceeded); }
        let (child, operation, preparer, intent) = (p.child, p.operation, p.preparer, p.intent);
        self.receipts.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        let charge = Charge::reserve(preparer, core::mem::size_of::<Receipt>())?;
        let mut seal_bytes = [0u8; 256];
        seal_bytes[..seal_len].copy_from_slice(seal);
        let receipt = self.mint()?;
        let mut producers = [ZERO; 2];
        if kind == 1 {
            // The issuer's producer peers are reserved now, so commit can move
            // the sole writer count to them without allocating or failing.
            self.holder_capacity(operation, 2)?;
            let charges = [Charge::reserve(preparer, core::mem::size_of::<Holder>())?,
                Charge::reserve(preparer, core::mem::size_of::<Holder>())?];
            let writers = intent.ok_or(Status::Denied)?.writers;
            let sources = [self.endpoint(preparer, writers[0], WRITE|DELEGATE)?,
                self.endpoint(preparer, writers[1], WRITE|DELEGATE)?];
            producers = [self.mint()?, self.mint()?];
            for (n, holder_charge) in charges.into_iter().enumerate() {
                self.endpoints.push(Holder { endpoint: Endpoint { reference: producers[n],
                    holder: Principal { cell_id: issuer.cell_id, generation: issuer.generation },
                    operation, parent: writers[n], handle: sources[n].handle, rights: WRITE|CLOSE,
                    state: 4, prepared: None, enabled: false, issuer_held: true }, charge: holder_charge });
            }
        }
        self.receipts.push(Receipt { reference: receipt, prepared: reference, child, operation, issuer,
            kind, slot, object, expected_epoch, next_epoch, digest, seal: seal_bytes, seal_len,
            state: 0, bound: false, producers, restored: false, materialized: false, charge });
        w.reference(receipt)
    }
}

impl State {
    /// Final child publication. Every check precedes the first mutation, and
    /// the mutation sequence below allocates nothing and cannot fail: holder
    /// storage, ring counts, owner slot, task row, CSpace row, measurement
    /// entry and ready slot were all reserved during preparation.
    fn commit_child(&mut self, sched: &mut Scheduler, pi: usize) -> Result<()> {
        let (parent, operation, reference, deadline, intent) = {
            let p = &self.prepared[pi];
            (p.preparer, p.operation, p.reference, p.deadline, p.intent)
        };
        self.op_active(parent, operation)?;
        if super::system_ticks() as u64 >= deadline {
            self.abort_prepared(sched, pi, true);
            return Err(Status::TimedOut);
        }
        // A receipt registered for this child but never bound by the preparer
        // would outlive the commit as a stale "Prepared" record and keep the
        // issuer reading a running child as pending: refuse the commit.
        if self.receipts.iter().any(|rr| rr.prepared == reference && rr.state == 0 && !rr.bound) {
            return Err(Status::Denied);
        }
        let (digest, has_cwd, has_stdio, frontend) = {
            let parsed = self.prepared[pi].parsed.as_ref().ok_or(Status::WouldBlock)?;
            (parsed.digest, parsed.grants[4].is_some(), (0..4).all(|slot| parsed.grants[slot].is_some()),
                parsed.grants[SLOT_FRONTEND as usize].map(|g| g.object))
        };
        // A frontend grant is admitted only with its own bound, unresolved
        // kind-6 receipt for this exact child and frontend object.
        if let Some(object) = frontend {
            let child = self.prepared[pi].child;
            let named = self.prepared[pi].receipts[2];
            if !self.receipt_index(named).ok().is_some_and(|ri| {
                let rr = &self.receipts[ri];
                rr.kind == GRANT_KIND_INPUT_FRONTEND && rr.prepared == reference && rr.child == child
                    && rr.object == object && rr.bound && rr.state == 0
            }) { return Err(Status::InvalidChild); }
        }
        if !self.prepared[pi].publication.as_ref().is_some_and(|p| p.current(sched)) { return Err(Status::Revoked); }
        let mut receipt_indices: [Option<usize>; 3] = [None; 3];
        for n in 0..3 {
            let named = self.prepared[pi].receipts[n];
            if named == ZERO { continue; }
            let ri = self.receipt_index(named)?;
            let rr = &self.receipts[ri];
            let service = if n == 1 { api::syscall::service::VFS } else { api::syscall::service::INPUT };
            if rr.prepared != reference || !rr.bound || rr.state != 0 || rr.digest != digest
                || binding(sched, service)? != rr.issuer { return Err(Status::Revoked); }
            receipt_indices[n] = Some(ri);
        }
        if has_cwd && receipt_indices[1].is_none() { return Err(Status::Denied); }
        let input = receipt_indices[0];
        if input.is_some() && !has_stdio { return Err(Status::Denied); }
        let staged = self.staged_grants(pi)?;
        // Reader grants are moves: the preparer's source reader is consumed so
        // the projected post-commit reader set is exactly the child.
        let mut consumed: [Option<usize>; 2] = [None; 2];
        let mut reader_rings = [0usize; 2];
        for (slot, staged_index) in staged.iter().enumerate() {
            let Some(staged_index) = staged_index else { continue };
            if slot != 0 && slot != 3 { continue; }
            let endpoint = self.endpoints[*staged_index].endpoint;
            if pipe::session_counts(endpoint.handle).map(|c| c.0) != Some(1) { return Err(Status::Denied); }
            let n = if slot == 0 { 0 } else { 1 };
            consumed[n] = Some(self.endpoint_index(endpoint.parent)?);
            reader_rings[n] = endpoint.handle.pipe;
        }
        if consumed[0].is_some() && consumed[1].is_some() && reader_rings[0] == reader_rings[1] {
            return Err(Status::Denied);
        }
        let mut producers = [ZERO; 2];
        let mut writer_indices: [Option<usize>; 2] = [None; 2];
        if let Some(ri) = input {
            let intent = intent.ok_or(Status::Denied)?;
            if intent.issuer != self.receipts[ri].issuer { return Err(Status::Revoked); }
            for n in 0..2 {
                let source = self.endpoint(parent, intent.writers[n], WRITE|DELEGATE)?;
                let target_index = staged[if n == 0 { 0 } else { 3 }].ok_or(Status::Denied)?;
                let target = self.endpoints[target_index].endpoint;
                if source.operation != operation || source.handle.pipe != target.handle.pipe
                    || pipe::session_counts(source.handle) != Some((1, 1)) { return Err(Status::Denied); }
                writer_indices[n] = Some(self.endpoint_index(intent.writers[n])?);
            }
            producers = self.receipts[ri].producers;
            for producer in producers {
                let ei = self.endpoint_index(producer)?;
                if self.endpoints[ei].endpoint.state != 4 { return Err(Status::Revoked); }
            }
        }
        // ---- No fallible step below this line. ----
        if input.is_some() {
            for n in 0..2 {
                let ei = self.endpoint_index(producers[n]).expect("validated reserved peer");
                self.endpoints[ei].endpoint.state = 0;
                self.endpoints[ei].endpoint.enabled = true;
                // The sole writer count moves to the issuer; it is not duplicated.
                self.endpoints[writer_indices[n].expect("validated source writer")].endpoint.state = 1;
            }
        }
        for (slot, staged_index) in staged.into_iter().enumerate() {
            let Some(index) = staged_index else { continue };
            if slot == 0 || slot == 3 {
                self.endpoints[consumed[if slot == 0 { 0 } else { 1 }].expect("validated source reader")].endpoint.state = 1;
            } else {
                assert!(pipe::duplicate_end(self.endpoints[index].endpoint.handle),
                    "validated child writer ring vanished");
            }
            self.endpoints[index].endpoint.prepared = None;
        }
        for ri in receipt_indices.into_iter().flatten() {
            self.receipts[ri].state = 1;
            if self.receipts[ri].kind == 1 { self.receipts[ri].producers = producers; }
        }
        let mut publication = self.prepared[pi].publication.take().expect("reserved publication");
        publication.task.child_context = Some(core::mem::take(&mut self.prepared[pi].context));
        self.prepared[pi].state = 1;
        publication.publish(sched);
        Ok(())
    }
}

impl State {
    fn bootstrap(&mut self, sched: &mut Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let operation = r.reference()?; let owner = r.binding()?; let capacity = r.u32()?; zero(r.u32()?)?; r.finish()?;
        let oi = self.op_active(actor, operation)?;
        if self.operations[oi].owner != actor || binding(sched, api::syscall::service::INPUT)? != owner {
            return Err(Status::WrongOwner);
        }
        if self.bootstraps.len() >= 256 || self.bootstraps.iter().filter(|b| b.state <= 2).count() >= 64
            || self.bootstraps.iter().filter(|b| b.controller == actor && b.state <= 2).count() >= 8 {
            return Err(Status::QuotaExceeded);
        }
        let (reference, endpoints) = self.install_bootstrap(sched, actor, operation, owner, capacity)?;
        w.reference(reference)?;
        for endpoint in &endpoints[..6] { w.reference(*endpoint)?; }
        Ok(())
    }
    /// Create the bootstrap record and its four rings, every precondition having
    /// been checked by the caller.
    fn install_bootstrap(&mut self, sched: &mut Scheduler, actor: Principal, operation: Ref, owner: Binding,
        capacity: u32) -> Result<(Ref, [Ref; 8])> {
        self.holder_capacity(operation, 8)?;
        self.bootstraps.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        let charge = Charge::reserve(actor, core::mem::size_of::<Bootstrap>())?;
        let reference = self.mint()?;
        let deadline = (super::system_ticks() as u64).checked_add(PREPARE_TICKS).ok_or(Status::QuotaExceeded)?;
        let mut streams = [[ZERO; 2]; 4];
        for n in 0..4 {
            match self.create_stream(actor, operation, capacity) {
                Ok(pair) => streams[n] = pair,
                Err(error) => {
                    for pair in streams {
                        for created in pair {
                            if let Ok(ei) = self.endpoint_index(created) { self.close_reservation_holder(sched, ei); }
                        }
                    }
                    return Err(error);
                }
            }
        }
        // stdin reader, stdout writer, stderr writer, control reader, frontend
        // output/diagnostic readers, and issuer-only producer peers.
        let endpoints = [streams[0][0], streams[1][1], streams[2][1], streams[3][0],
            streams[1][0], streams[2][0], streams[0][1], streams[3][1]];
        for (n, endpoint) in endpoints.iter().enumerate() {
            let ei = self.endpoint_index(*endpoint)?;
            let e = &mut self.endpoints[ei].endpoint;
            e.enabled = false;
            e.rights = match n { 0 | 3 => READ|CLOSE, 1 | 2 => WRITE|DUPLICATE|DELEGATE|CLOSE,
                4 | 5 => READ|DUPLICATE|CLOSE, _ => WRITE|CLOSE };
            if n >= 6 {
                e.holder = Principal { cell_id: owner.cell_id, generation: owner.generation };
                e.issuer_held = true;
            }
        }
        self.bootstraps.push(Bootstrap { reference, controller: actor, operation, issuer: owner,
            endpoints, session: ZERO, state: 0, deadline, charge });
        Ok((reference, endpoints))
    }
    fn resolve_bootstrap(&mut self, sched: &mut Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let reference = r.reference()?; let action = r.u32()?; zero(r.u32()?)?; let session = r.reference()?; r.finish()?;
        let bi = self.bootstrap_index(reference)?;
        let (controller, recorded, state) = {
            let b = &self.bootstraps[bi];
            (b.controller, b.issuer, b.state)
        };
        let is_issuer = issuer(sched, actor, api::syscall::service::INPUT).ok() == Some(recorded);
        if actor != controller && !is_issuer { return Err(Status::WrongOwner); }
        if action > 3 || (action != 3 && session != ZERO) { return Err(Status::Malformed); }
        match action {
            0 => {},
            1 => {
                if !is_issuer { return Err(Status::Denied); }
                if state == 0 { self.bootstraps[bi].state = 1; }
                else if state != 1 { return Err(Status::Stale); }
            }
            2 => {
                // Controller or issuer: abort a reserved bootstrap, or close an
                // active one (cancelling its committed foreground child).
                if state <= 2 { self.close_bootstrap(sched, bi, if state == 2 { 5 } else { 3 }); }
            }
            3 => {
                if !is_issuer || session == ZERO || state != 1 { return Err(Status::Denied); }
                if self.bootstraps.iter().any(|b| b.state == 2 && b.session == session && b.issuer == recorded) {
                    return Err(Status::WrongSession);
                }
                // Resolve every endpoint before the first mutation: nothing may
                // fail once the bootstrap is active.
                let mut indices = [0usize; 8];
                for (n, endpoint) in self.bootstraps[bi].endpoints.iter().enumerate() {
                    indices[n] = self.endpoint_index(*endpoint)?;
                }
                self.bootstraps[bi].session = session;
                self.bootstraps[bi].state = 2;
                for ei in indices { self.endpoints[ei].endpoint.enabled = true; }
            }
            _ => return Err(Status::Malformed),
        }
        let b = &self.bootstraps[bi];
        w.u32(b.state)?; w.u32(0)?; w.principal(b.controller)?; w.reference(b.operation)?;
        for (n, endpoint) in b.endpoints.iter().enumerate() {
            w.reference(if n >= 6 && !is_issuer { ZERO } else { *endpoint })?;
        }
        Ok(())
    }
    /// RestoreSessionInput: exact input issuer, exact canonical terminal child.
    /// Validates both baseline rings before any close or discard, then closes the
    /// child's producer peers and discards stale baseline bytes in one transition.
    fn restore(&mut self, sched: &mut Scheduler, actor: Principal, mut r: Reader<'_>, w: &mut Writer<'_>) -> Result<()> {
        let reference = r.reference()?; let terminal = r.u64()?; let expected_epoch = r.u64()?; r.finish()?;
        let ri = self.receipt_index(reference)?;
        let rr = &self.receipts[ri];
        if rr.kind != 1 || rr.state != 1 || issuer(sched, actor, api::syscall::service::INPUT)? != rr.issuer {
            return Err(Status::WrongOwner);
        }
        let (prepared, next_epoch, rr_issuer, producers, restored) = (rr.prepared, rr.next_epoch, rr.issuer, rr.producers, rr.restored);
        let session = read_ref(&rr.seal, 0)?;
        let p = &self.prepared[self.preparation(prepared)?];
        if p.state != 3 || terminal == 0 || p.terminal_sequence != terminal { return Err(Status::InvalidChild); }
        let preparer = p.preparer;
        if next_epoch != expected_epoch { return Err(Status::WrongEpoch); }
        let bi = self.bootstraps.iter().position(|b| b.state == 2 && b.session == session && b.issuer == rr_issuer
            && b.controller == preparer).ok_or(Status::WrongSession)?;
        if self.receipts.iter().any(|other| other.kind == 1 && other.state == 1 && !other.restored
            && other.reference != reference && read_ref(&other.seal, 0).ok() == Some(session)
            && other.next_epoch > expected_epoch) { return Err(Status::WrongEpoch); }
        let baseline = [self.bootstraps[bi].endpoints[0], self.bootstraps[bi].endpoints[3]];
        if !restored {
            let controller = self.bootstraps[bi].controller;
            let handles = [self.endpoint(controller, baseline[0], READ)?.handle,
                self.endpoint(controller, baseline[1], READ)?.handle];
            for handle in handles { if pipe::session_counts(handle).is_none() { return Err(Status::Stale); } }
            for producer in producers {
                if let Ok(ei) = self.endpoint_index(producer) { self.close_holder(sched, ei, 1); }
            }
            for handle in handles { pipe::session_flush(sched, handle); }
            self.receipts[ri].restored = true;
        }
        w.reference(baseline[0])?; w.reference(baseline[1])
    }
    /// Release terminal records of `operation` that nothing can still refer to.
    /// A retained outcome keeps exactly the record named by its request: a replay
    /// resolves that record to find its correlation scope, so removing it would
    /// turn the retained result into `Stale`. Every other terminal record is
    /// reclaimed regardless of unrelated outstanding or in-flight outcomes.
    fn reclaim(&mut self, operation: Ref) {
        let ledgers = &self.ledgers;
        let outcomes = ledgers.iter().any(|l| l.operation == operation && !l.outcomes.is_empty());
        let pinned = |reference: Ref| outcomes && ledgers.iter().any(|l| l.operation == operation
            && l.outcomes.iter().any(|o| o.request.get(HEADER_LEN..HEADER_LEN + 16)
                .is_some_and(|named| Ref::from_bytes(named).ok() == Some(reference))));
        self.receipts.retain(|rr| rr.operation != operation || !rr.materialized
            || (rr.state == 1 && !rr.restored && rr.kind == 1)
            || self.prepared.iter().any(|p| p.reference == rr.prepared && p.state <= 2)
            || pinned(rr.reference));
        // A terminal (exited) record stays readable by its preparer (`QueryChild`) even
        // after the receipts bound to it (VFS cwd, input foreground) were resolved and
        // removed: a controller reads its foreground child's exit status there, and the
        // input owner may restore and acknowledge before the controller asks.
        // `remove_operation` releases it. An aborted or expired record is dropped here as
        // soon as no receipt names it.
        self.prepared.retain(|p| p.operation != operation || p.state <= 2
            || self.receipts.iter().any(|rr| rr.prepared == p.reference) || pinned(p.reference)
            || p.state == 3);
        self.endpoints.retain(|h| h.endpoint.operation != operation || h.endpoint.state == 0
            || h.endpoint.state == 4 || pinned(h.endpoint.reference));
        self.bootstraps.retain(|b| b.operation != operation || b.state <= 2 || pinned(b.reference));
    }
}

/// Temporarily borrow canonical session bookkeeping so its transitions can
/// update the scheduler without any second owner lock or recursive acquisition.
/// Bookkeeping heap is kernel memory; per-principal charges are explicit.
fn with_state<T>(sched: &mut Scheduler, f: impl FnOnce(&mut State, &mut Scheduler) -> T) -> T {
    let _kernel = KernelAllocationContext::enter();
    let mut state = core::mem::replace(&mut sched.cell_session, State::new());
    let result = f(&mut state, sched);
    sched.cell_session = state;
    result
}
/// Root retirement began: the lifetime record is no longer live. Terminal
/// sequence is minted later, by `retired`, after resource teardown.
pub(crate) fn retiring(sched: &mut Scheduler, owner: api::cell_owner::CellOwner, exit_reason: usize) {
    with_state(sched, |state, sched| {
        let principal = Principal { cell_id: owner.cell_id, generation: owner.generation };
        for i in 0..state.prepared.len() {
            if state.prepared[i].child.principal == principal && state.prepared[i].state == 1 {
                state.prepared[i].state = 2;
                if state.prepared[i].reason == 0 {
                    state.prepared[i].reason = if exit_reason == usize::MAX { 1 } else { 0 };
                    state.prepared[i].exit_code = if exit_reason == usize::MAX { 0 } else { exit_reason as i32 };
                }
            }
            if state.prepared[i].preparer == principal && state.prepared[i].state == 0 {
                state.abort_prepared(sched, i, false);
            }
        }
        for i in 0..state.operations.len() {
            if state.operations[i].owner == principal {
                let operation = state.operations[i].reference;
                state.cancel(sched, operation, 2);
            }
        }
        for i in 0..state.endpoints.len() {
            if state.endpoints[i].endpoint.holder == principal { state.close_holder(sched, i, 1); }
        }
        state.maintenance(sched);
    });
}
/// Root retirement completed (owner slot empty, quota still registered): mint
/// the canonical terminal sequence and retire everything charged to the dead
/// principal so a reused CellId can never be refunded or charged for it.
pub(crate) fn retired(sched: &mut Scheduler, owner: api::cell_owner::CellOwner) {
    with_state(sched, |state, _| {
        for i in 0..state.prepared.len() {
            let p = &state.prepared[i];
            if p.child.principal.cell_id == owner.cell_id && p.child.principal.generation == owner.generation && p.state == 2 {
                let terminal = state.terminal_sequence();
                state.prepared[i].state = 3;
                state.prepared[i].terminal_sequence = terminal;
            }
        }
        state.forfeit_charges(owner.cell_id as usize);
        state.ledgers.retain(|l| l.actor.cell_id != owner.cell_id || l.actor.generation != owner.generation);
    });
    pipe::disown_cell(owner.cell_id as usize);
}
pub(crate) fn sweep(sched: &mut Scheduler) {
    with_state(sched, |state, sched| state.maintenance(sched));
    loop {
        let tid = with_state(sched, |state, _| {
            for p in state.prepared.iter_mut() {
                if p.kill {
                    p.kill = false;
                    if p.state == 1 { return Some(p.child.root_tid as usize); }
                }
            }
            None
        });
        match tid { Some(tid) => sched.exit_task(tid, usize::MAX), None => break }
    }
}

fn mutating(selector: u16, body: &[u8]) -> bool {
    if matches!(selector, 9 | 17 | 18 | 19 | 23 | 24 | 25 | 26 | 28 | 30) { return false; }
    if matches!(selector, 16 | 22) {
        return body.get(16..20).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) != Some(0);
    }
    true
}
/// Largest success record for `selector`; reserved before any mutation so a
/// short result buffer can never leave an outcome ambiguous.
pub(crate) fn required_result(selector: u16, body: &[u8]) -> usize {
    HEADER_LEN + match selector {
        1 | 3 | 4 | 15 | 27 => 16, 2 | 13 | 29 => 32, 10 => 48,
        5 => 8 + body.get(16..20).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).unwrap_or(512).min(512) as usize,
        6 | 11 => 8, 9 => 16, 16 => 72, 17 => 520, 18 => 648, 19 => 48,
        21 => 112, 22 => 168, 23 => 64, 24 => 56, 26 => 80, 28 => RESULT_MAX-HEADER_LEN, 30 => 24,
        _ => 0,
    }
}
fn encode_result(header: RequestHeaderV1, status: Status, payload: usize, out: &mut [u8]) -> usize {
    let len = HEADER_LEN + payload;
    ResultHeaderV1 { selector: header.selector, status, total_len: len as u32,
        request_id: header.request_id }.encode(out).expect("bounded result header");
    len
}
fn render(header: RequestHeaderV1, outcome: Result<usize>, result: &mut [u8]) -> usize {
    match outcome {
        Ok(n) if n & RAW != 0 => {
            let len = n & !RAW;
            result.copy_within(HEADER_LEN..HEADER_LEN + len, 0);
            len
        }
        Ok(n) => encode_result(header, Status::Ok, n, result),
        Err(status) => encode_result(header, status, 0, result),
    }
}
/// One CellSessionV1 transaction. Returns the byte length written to `result`.
/// Selectors that block (5, 6) or load an artifact (10) run their slow part
/// outside the owner lock; every other selector is a single locked transition.
pub(crate) fn dispatch(caller: usize, request: &[u8], result: &mut [u8]) -> usize {
    let header = match RequestHeaderV1::decode(request) {
        Ok(header) => header,
        Err(status) => {
            // Header-only syntax rejections may echo a zero request ID.
            let selector = request.get(4..6).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0);
            let request_id = request.get(16..24).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).unwrap_or(0);
            ResultHeaderV1 { selector, request_id, status, total_len: HEADER_LEN as u32 }.encode(result).ok();
            return HEADER_LEN;
        }
    };
    let body = &request[HEADER_LEN..];
    let selector = header.selector;
    let ledgered = mutating(selector, body);
    let (principal, operation) = {
        let _kernel = KernelAllocationContext::enter();
        let mut guard = super::SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else { return encode_result(header, Status::Denied, 0, result); };
        // Death, expiry and kill propagation run first: a caller retired by
        // this sweep must not complete a request after its retirement.
        sweep(sched);
        let principal = match resolve_actor(sched, caller) {
            Ok(principal) => principal,
            Err(status) => return encode_result(header, status, 0, result),
        };
        let operation = match sched.cell_session.request_operation(&*sched, principal, selector, body, ledgered) {
            Ok(operation) => operation,
            Err(status) => return encode_result(header, status, 0, result),
        };
        if ledgered {
            match sched.cell_session.begin_request(principal, operation, header, request, result) {
                Ok(Begin::New) => {}
                Ok(Begin::Replay(len)) => return len,
                Err(status) => return encode_result(header, status, 0, result),
            }
        }
        if !matches!(selector, 5 | 6 | 10) {
            let outcome = with_state(sched, |state, sched| {
                state.execute(sched, principal, selector, body, &mut result[HEADER_LEN..])
            });
            let len = render(header, outcome, result);
            if ledgered { sched.cell_session.finish_request(principal, operation, header.request_id, &result[..len]); }
            sweep(sched);
            return len;
        }
        (principal, operation)
    };
    let outcome = if selector == 10 {
        prepare_child(caller, principal, operation, body, &mut result[HEADER_LEN..])
    } else {
        stream_io(caller, principal, selector, body, &mut result[HEADER_LEN..])
    };
    let len = render(header, outcome, result);
    let _kernel = KernelAllocationContext::enter();
    let mut guard = super::SCHEDULER.lock();
    if let Some(sched) = guard.as_mut() {
        if ledgered { sched.cell_session.finish_request(principal, operation, header.request_id, &result[..len]); }
        sweep(sched);
    }
    len
}
/// Context staging bytes are kernel memory: allocated and released under the
/// kernel accounting identity, with the creator's explicit `Charge` as the quota.
struct KernelBytes(Vec<u8>);
impl Drop for KernelBytes {
    fn drop(&mut self) {
        let _kernel = KernelAllocationContext::enter();
        drop(core::mem::take(&mut self.0));
    }
}
fn prepare_child(caller: usize, principal: Principal, operation: Ref, body: &[u8], out: &mut [u8]) -> Result<usize> {
    let deadline = (super::system_ticks() as u64).checked_add(PREPARE_TICKS).ok_or(Status::QuotaExceeded)?;
    let mut r = Reader::new(body);
    if r.reference()? != operation { return Err(Status::Malformed); }
    let route = r.u32()?; let path_len = r.u32()? as usize; let context_len = r.u32()? as usize; zero(r.u32()?)?;
    let path = r.take(path_len)?; r.finish()?;
    if route != 1 || !(1..=4096).contains(&path_len) || path.contains(&0)
        || !(48..=child_context::MAX_LEN).contains(&context_len) { return Err(Status::Malformed); }
    let path = core::str::from_utf8(path).map_err(|_| Status::Malformed)?;
    {
        let guard = super::SCHEDULER.lock(); let sched = guard.as_ref().ok_or(Status::Revoked)?;
        sched.cell_session.op_active(principal, operation)?;
        if sched.cell_session.prepared.iter().filter(|p| p.operation == operation && p.state <= 2).count() >= 16
            || sched.cell_session.prepared.iter().any(|p| p.operation == operation && p.state == 0)
            || sched.cell_session.prepared.iter().filter(|p| p.preparer == principal && p.state == 0).count()
                >= MAX_STAGING_PER_PRINCIPAL
            || sched.cell_session.prepared.len() >= 512 { return Err(Status::QuotaExceeded); }
    }
    let charge = Charge::reserve(principal, context_len + core::mem::size_of::<Prepared>())?;
    let mut context = KernelBytes(Vec::new());
    {
        let _kernel = KernelAllocationContext::enter();
        context.0.try_reserve_exact(context_len).map_err(|_| Status::QuotaExceeded)?;
        context.0.resize(context_len, 0);
    }
    let request = super::syscall::session_launch_request(caller, path).map_err(|_| Status::Denied)?;
    let (prepared, state) = request.prepare_session_path(path).map_err(vi)?;
    if super::system_ticks() as u64 >= deadline { return Err(Status::TimedOut); }
    let _kernel = KernelAllocationContext::enter();
    let mut guard = super::SCHEDULER.lock(); let sched = guard.as_mut().ok_or(Status::Revoked)?;
    if resolve_actor(sched, caller)? != principal { return Err(Status::Revoked); }
    sched.cell_session.op_active(principal, operation)?;
    if sched.cell_session.prepared.iter().any(|p| p.operation == operation && p.state == 0)
        || sched.cell_session.prepared.iter().filter(|p| p.preparer == principal && p.state == 0).count()
            >= MAX_STAGING_PER_PRINCIPAL {
        return Err(Status::QuotaExceeded);
    }
    sched.cell_session.prepared.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
    let publication = super::launch::SessionPublication::reserve(sched, prepared, state).map_err(vi)?;
    let child = Child { principal: Principal { cell_id: publication.owner.cell_id, generation: publication.owner.generation },
        root_tid: publication.owner.root_tid, publication_generation: publication.owner.generation };
    let reference = child.prepared_ref();
    sched.cell_session.prepared.push(Prepared { reference, child, preparer: principal, operation,
        deadline, written: 0, context: core::mem::take(&mut context.0), parsed: None, publication: Some(publication),
        state: 0, reason: 0, exit_code: 0, terminal_sequence: 0, kill: false,
        intent: None, receipts: [ZERO; 3], charge });
    let mut w = Writer::new(out); w.reference(reference)?; w.child(child)?;
    Ok(w.position())
}

/// ReadStream / WriteStream. Wait registration, endpoint checks, cancellation and
/// readiness are one SCHEDULER-serialized check-and-park; a wake rechecks typed
/// state, so timeout is never confused with EOF.
fn stream_io(caller: usize, principal: Principal, selector: u16, body: &[u8], out: &mut [u8]) -> Result<usize> {
    let mut r = Reader::new(body); let reference = r.reference()?; let count = r.u32()? as usize;
    zero(r.u32()?)?; let deadline_ns = r.u64()?;
    if !(1..=512).contains(&count) { return Err(Status::Malformed); }
    let bytes = if selector == 6 { r.take(count)? } else { &[] }; r.finish()?;
    loop {
        let mut guard = super::SCHEDULER.lock(); let sched = guard.as_mut().ok_or(Status::Revoked)?;
        if resolve_actor(sched, caller)? != principal { return Err(Status::Revoked); }
        let e = sched.cell_session.endpoint(principal, reference, if selector == 5 { READ } else { WRITE })?;
        let mut scratch = [0u8; 512];
        let progress = if selector == 5 {
            match pipe::session_read(sched, e.handle, &mut scratch[..count]).ok_or(Status::Stale)? {
                pipe::ReadOutcome::Read(n) => Some(n), pipe::ReadOutcome::Eof => return Err(Status::Eof),
                pipe::ReadOutcome::Empty => None,
            }
        } else {
            match pipe::session_write(sched, e.handle, bytes).ok_or(Status::Stale)? {
                pipe::WriteOutcome::Wrote(n) => Some(n), pipe::WriteOutcome::NoReader => return Err(Status::BrokenPipe),
                pipe::WriteOutcome::Full => None,
            }
        };
        if let Some(n) = progress {
            let mut w = Writer::new(out); w.u32(n as u32)?; w.u32(0)?;
            if selector == 5 { w.bytes(&scratch[..n])?; }
            return Ok(w.position());
        }
        if deadline_ns == 0 { return Err(Status::WouldBlock); }
        // Absolute monotonic ns -> scheduler ticks, rounded up plus one tick so
        // the wake can never precede the requested deadline.
        let wait = if deadline_ns == u64::MAX { None } else {
            let now = super::telemetry::monotonic_ns().ok_or(Status::Unsupported)?;
            if now >= deadline_ns { return Err(Status::TimedOut); }
            let ticks = (deadline_ns - now).div_ceil(TICK_NS);
            Some((super::system_ticks() as u64).checked_add(ticks).and_then(|t| t.checked_add(1)).ok_or(Status::Malformed)?)
        };
        if selector == 5 { pipe::park_reader(e.handle, caller); } else { pipe::park_writer(e.handle, caller); }
        let task = sched.tasks.get_mut(&caller).ok_or(Status::Revoked)?;
        task.state = if selector == 5 { super::tcb::TaskState::PipeRead { handle: e.handle, deadline: wait } }
            else { super::tcb::TaskState::PipeWrite { handle: e.handle, deadline: wait } };
        drop(guard);
        super::yield_cpu();
    }
}

/// Legacy fd 0/1/2 of a published session child, as
/// `Some(Ok((principal, endpoint)))`; `None` only for a task that is not a
/// session child, which keeps the unchanged console path. A published child
/// never falls back to the console, even if its root row is already gone.
fn stdio_ref(caller: usize, fd: usize) -> Option<Result<(Principal, Ref)>> {
    let guard = super::SCHEDULER.lock();
    let sched = guard.as_ref()?;
    let task = sched.tasks.get(&caller)?;
    let principal = Principal { cell_id: task.cell_id.0, generation: task.cell_generation };
    let root = sched.tasks.get(&task.root_tid);
    let has_context = root.is_some_and(|root| root.child_context.is_some());
    if !has_context && !sched.cell_session.session_child(principal) { return None; }
    let Some(root) = root.filter(|_| has_context) else { return Some(Err(Status::Revoked)); };
    if sched.resolve_live_cell_owner(task.cell_id, task.cell_generation).is_none() {
        return Some(Err(Status::Revoked));
    }
    let reference = root.child_stdio.get(fd).copied().unwrap_or(ZERO);
    Some(if reference == ZERO { Err(Status::Denied) } else { Ok((principal, reference)) })
}
/// The reply of `request` was copied out to the caller: only now may an
/// acknowledgement cover its outcome. Called by the syscall layer after a
/// successful copy-out; a failed copy-out leaves the outcome unobserved so the
/// caller must recover it with QueryRequest before acknowledging.
pub(crate) fn delivered(caller: usize, request: &[u8]) {
    let Ok(header) = RequestHeaderV1::decode(request) else { return; };
    if !mutating(header.selector, request.get(HEADER_LEN..).unwrap_or(&[])) { return; }
    let mut guard = super::SCHEDULER.lock();
    let Some(sched) = guard.as_mut() else { return; };
    let Ok(actor) = resolve_actor(sched, caller) else { return; };
    sched.cell_session.mark_observed(actor, request, header.request_id);
}
pub(crate) fn stdio_read(caller: usize, dst: &mut [u8]) -> Option<usize> {
    let (principal, reference) = match stdio_ref(caller, 0)? { Ok(pair) => pair, Err(_) => return Some(usize::MAX) };
    if dst.is_empty() { return Some(0); }
    let mut body = [0u8; 32]; let mut w = Writer::new(&mut body);
    w.reference(reference).ok()?; w.u32(dst.len().min(512) as u32).ok()?; w.u32(0).ok()?; w.u64(u64::MAX).ok()?;
    let mut out = [0u8; 520];
    Some(match stream_io(caller, principal, 5, &body, &mut out) {
        Ok(_) => { let count = u32::from_le_bytes(out[..4].try_into().unwrap()) as usize;
            dst[..count].copy_from_slice(&out[8..8+count]); count },
        Err(Status::Eof) => 0,
        Err(_) => usize::MAX,
    })
}
pub(crate) fn stdio_write(caller: usize, fd: usize, src: &[u8]) -> Option<usize> {
    let (principal, reference) = match stdio_ref(caller, fd)? { Ok(pair) => pair, Err(_) => return Some(usize::MAX) };
    if src.is_empty() { return Some(0); }
    let count = src.len().min(512);
    let mut body = [0u8; 544]; let mut w = Writer::new(&mut body);
    w.reference(reference).ok()?; w.u32(count as u32).ok()?; w.u32(0).ok()?; w.u64(u64::MAX).ok()?;
    w.bytes(&src[..count]).ok()?;
    let mut out = [0u8; 8];
    Some(match stream_io(caller, principal, 6, &body[..32+count], &mut out) {
        Ok(_) => u32::from_le_bytes(out[..4].try_into().unwrap()) as usize, Err(_) => usize::MAX,
    })
}

/// Owner-code probes for `loader::atomic_publication_tests`: they stage and
/// observe records directly in the canonical state, but every transition under
/// test (`commit_child`, ledger scoping) is the production code.
#[cfg(feature = "test-hooks")]
pub(crate) mod test_hooks {
    use super::*;
    /// Request ledgers held by one principal `(cell, generation)`, or all of them.
    pub(crate) fn ledgers(sched: &Scheduler, actor: Option<(u64, u64)>) -> usize {
        sched.cell_session.ledgers.iter()
            .filter(|l| actor.map_or(true, |(cell, generation)| l.actor.cell_id == cell && l.actor.generation == generation))
            .count()
    }
    /// Stage an unpublished child record for `(cell, generation)` on `operation`
    /// with one registered receipt that the preparer never bound.
    pub(crate) fn stage_unbound_receipt(sched: &mut Scheduler, cell: u64, generation: u64, operation: Ref) -> Option<Ref> {
        let _kernel = KernelAllocationContext::enter();
        let preparer = Principal { cell_id: cell, generation };
        let child = Child { principal: Principal { cell_id: cell, generation: generation.wrapping_add(1) },
            root_tid: 93_001_000, publication_generation: 0x5E55_F001 };
        let reference = child.prepared_ref();
        let state = &mut sched.cell_session;
        let receipt = state.mint().ok()?;
        let deadline = (super::super::system_ticks() as u64).checked_add(PREPARE_TICKS)?;
        state.prepared.try_reserve(1).ok()?;
        state.receipts.try_reserve(1).ok()?;
        state.prepared.push(Prepared { reference, child, preparer, operation, deadline, written: 0,
            context: Vec::new(), parsed: None, publication: None, state: 0, reason: 0, exit_code: 0,
            terminal_sequence: 0, kill: false, intent: None, receipts: [ZERO; 3],
            charge: Charge { cell: cell as usize, bytes: 0 } });
        state.receipts.push(Receipt { reference: receipt, prepared: reference, child, operation,
            issuer: Binding::new(u64::MAX, u64::MAX, u64::MAX), kind: 2, slot: 4, object: ZERO,
            expected_epoch: 0, next_epoch: 0, digest: [0; 32], seal: [0; 256], seal_len: 0, state: 0,
            bound: false, producers: [ZERO; 2], restored: false, materialized: false,
            charge: Charge { cell: cell as usize, bytes: 0 } });
        Some(reference)
    }
    /// Mark the staged receipt bound, as `BindReceipt` would.
    pub(crate) fn bind_receipt(sched: &mut Scheduler, prepared: Ref) {
        for rr in sched.cell_session.receipts.iter_mut().filter(|rr| rr.prepared == prepared) { rr.bound = true; }
    }
    /// The production `commit_child`, without an intervening maintenance pass.
    pub(crate) fn commit(sched: &mut Scheduler, prepared: Ref) -> Option<Status> {
        with_state(sched, |state, sched| {
            let pi = state.preparation(prepared).ok()?;
            Some(match state.commit_child(sched, pi) { Ok(()) => Status::Ok, Err(status) => status })
        })
    }
    pub(crate) fn prepared_state(sched: &Scheduler, prepared: Ref) -> Option<u32> {
        sched.cell_session.preparation(prepared).ok().map(|i| sched.cell_session.prepared[i].state)
    }
    /// Remove every staged record of this fixture so nothing outlives the case.
    pub(crate) fn discard(sched: &mut Scheduler, prepared: Ref) {
        sched.cell_session.receipts.retain(|rr| rr.prepared != prepared);
        sched.cell_session.prepared.retain(|p| p.reference != prepared);
    }
    /// One production transition (`execute`, no ledger) run as `(cell, generation)`
    /// inside the caller's SCHEDULER hold, so no maintenance pass can interleave.
    pub(crate) fn exec(sched: &mut Scheduler, cell: u64, generation: u64, selector: u16, body: &[u8])
        -> (Status, Vec<u8>) {
        let actor = Principal { cell_id: cell, generation };
        with_state(sched, |state, sched| {
            let mut out = [0u8; 1024];
            match state.execute(sched, actor, selector, body, &mut out) {
                Ok(n) => (Status::Ok, out[..n & !RAW].to_vec()),
                Err(status) => (status, Vec::new()),
            }
        })
    }
    /// Install an active bootstrap for `controller` in `operation` through the
    /// production `install_bootstrap` (the input-service preconditions of the
    /// wire request are not the subject of the callers).
    pub(crate) fn stage_bootstrap(sched: &mut Scheduler, cell: u64, generation: u64, operation: Ref)
        -> Option<[Ref; 8]> {
        let actor = Principal { cell_id: cell, generation };
        with_state(sched, |state, sched| {
            let issuer = Binding::new(u64::MAX, u64::MAX, u64::MAX);
            let (reference, endpoints) = state.install_bootstrap(sched, actor, operation, issuer, 64).ok()?;
            let bi = state.bootstrap_index(reference).ok()?;
            state.bootstraps[bi].session = Ref { id: u64::MAX, generation: 1 };
            state.bootstraps[bi].state = 2;
            for endpoint in endpoints {
                let ei = state.endpoint_index(endpoint).ok()?;
                state.endpoints[ei].endpoint.enabled = true;
            }
            Some(endpoints)
        })
    }
    /// A further live holder of `source`'s ring held by `(cell, generation)` in
    /// `operation`, as a retained or descendant writer would be.
    pub(crate) fn stage_holder(sched: &mut Scheduler, source: Ref, operation: Ref, cell: u64, generation: u64)
        -> Option<Ref> {
        with_state(sched, |state, _| {
            let src = state.endpoints[state.endpoint_index(source).ok()?].endpoint;
            state.endpoints.try_reserve(1).ok()?;
            if !pipe::duplicate_end(src.handle) { return None; }
            let reference = state.mint().ok()?;
            state.endpoints.push(Holder { endpoint: Endpoint { reference, holder: Principal { cell_id: cell, generation },
                operation, parent: source, handle: src.handle, rights: src.rights, state: 0, prepared: None,
                enabled: true, issuer_held: false }, charge: Charge { cell: cell as usize, bytes: 0 } });
            Some(reference)
        })
    }
    /// The production close of one holder (`CloseEndpoint` minus the actor checks).
    pub(crate) fn close_endpoint(sched: &mut Scheduler, reference: Ref) -> bool {
        with_state(sched, |state, sched| {
            let Ok(i) = state.endpoint_index(reference) else { return false; };
            state.close_holder(sched, i, 1);
            true
        })
    }
    pub(crate) fn cancel_operation(sched: &mut Scheduler, operation: Ref) {
        with_state(sched, |state, sched| state.cancel(sched, operation, 2));
    }
    /// One full maintenance pass (no kill delivery), as `sweep` runs first.
    pub(crate) fn maintain(sched: &mut Scheduler) {
        with_state(sched, |state, sched| state.maintenance(sched));
    }
    pub(crate) fn operation_state(sched: &Scheduler, operation: Ref) -> Option<u32> {
        let state = &sched.cell_session;
        state.op_index(operation).ok().map(|i| state.operations[i].state)
    }
    /// `None` once the holder record itself has been reclaimed.
    pub(crate) fn endpoint_state(sched: &Scheduler, endpoint: Ref) -> Option<u32> {
        let state = &sched.cell_session;
        state.endpoint_index(endpoint).ok().map(|i| state.endpoints[i].endpoint.state)
    }
    /// Receipts recorded against `operation`.
    pub(crate) fn receipts_on(sched: &Scheduler, operation: Ref) -> usize {
        sched.cell_session.receipts.iter().filter(|rr| rr.operation == operation).count()
    }
    /// `(state, reason, kill flag)` of a prepared child record.
    pub(crate) fn prepared_flags(sched: &Scheduler, prepared: Ref) -> Option<(u32, u32, bool)> {
        let state = &sched.cell_session;
        state.preparation(prepared).ok().map(|i| (state.prepared[i].state, state.prepared[i].reason, state.prepared[i].kill))
    }
    /// Stage a committed foreground child of `preparer` in `operation` whose
    /// InputForeground receipt was recorded by an issuer that is not the
    /// registered provider (replaced or dead), with the issuer's two producer
    /// peers live on the rings of `writers` (held by `preparer`).
    pub(crate) fn stage_issuer_receipt(sched: &mut Scheduler, preparer: (u64, u64), child: (u64, u64),
        operation: Ref, writers: [Ref; 2]) -> Option<(Ref, [Ref; 2])> {
        let _kernel = KernelAllocationContext::enter();
        let preparer = Principal { cell_id: preparer.0, generation: preparer.1 };
        let child = Child { principal: Principal { cell_id: child.0, generation: child.1 },
            root_tid: 93_002_000, publication_generation: 0x5E55_F002 };
        let reference = child.prepared_ref();
        let issuer = Binding::new(u64::MAX, u64::MAX, u64::MAX);
        let deadline = (super::super::system_ticks() as u64).checked_add(PREPARE_TICKS)?;
        let state = &mut sched.cell_session;
        state.prepared.try_reserve(1).ok()?;
        state.receipts.try_reserve(1).ok()?;
        state.endpoints.try_reserve(2).ok()?;
        let sources = [state.endpoints[state.endpoint_index(writers[0]).ok()?].endpoint,
            state.endpoints[state.endpoint_index(writers[1]).ok()?].endpoint];
        let receipt = state.mint().ok()?;
        let mut producers = [ZERO; 2];
        for n in 0..2 {
            if !pipe::duplicate_end(sources[n].handle) { return None; }
            producers[n] = state.mint().ok()?;
            state.endpoints.push(Holder { endpoint: Endpoint { reference: producers[n],
                holder: Principal { cell_id: issuer.cell_id, generation: issuer.generation }, operation,
                parent: writers[n], handle: sources[n].handle, rights: WRITE|CLOSE, state: 0, prepared: None,
                enabled: true, issuer_held: true },
                charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        }
        state.prepared.push(Prepared { reference, child, preparer, operation, deadline, written: 0,
            context: Vec::new(), parsed: None, publication: None, state: 1, reason: 0, exit_code: 0,
            terminal_sequence: 0, kill: false, intent: None, receipts: [ZERO; 3],
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        state.receipts.push(Receipt { reference: receipt, prepared: reference, child, operation, issuer,
            kind: 1, slot: 0, object: ZERO, expected_epoch: 0, next_epoch: 1, digest: [0; 32], seal: [0; 256],
            seal_len: 0, state: 1, bound: true, producers, restored: false, materialized: false,
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        Some((reference, producers))
    }
    /// Stage a committed, receipt-less child (state 1) of `preparer` in `operation`,
    /// as a shell pipeline stage looks after `CommitChild`. `root` distinguishes records.
    pub(crate) fn stage_receiptless_child(sched: &mut Scheduler, preparer: (u64, u64), child: (u64, u64),
        operation: Ref, root: u64) -> Option<Child> {
        let _kernel = KernelAllocationContext::enter();
        let preparer = Principal { cell_id: preparer.0, generation: preparer.1 };
        let child = Child { principal: Principal { cell_id: child.0, generation: child.1 },
            root_tid: root, publication_generation: 0x5E55_F003 };
        let deadline = (super::super::system_ticks() as u64).checked_add(PREPARE_TICKS)?;
        let state = &mut sched.cell_session;
        state.prepared.try_reserve(1).ok()?;
        state.prepared.push(Prepared { reference: child.prepared_ref(), child, preparer, operation, deadline,
            written: 0, context: Vec::new(), parsed: None, publication: None, state: 1, reason: 0, exit_code: 0,
            terminal_sequence: 0, kill: false, intent: None, receipts: [ZERO; 3],
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        Some(child)
    }
    /// Like [`stage_receiptless_child`] but with the VFS cwd receipt (kind 2, slot 4)
    /// every shell pipeline stage with a working directory carries: bound in the
    /// preparer's record, registered by an issuer, not yet resolved.
    pub(crate) fn stage_vfs_receipted_child(sched: &mut Scheduler, preparer: (u64, u64), child: (u64, u64),
        operation: Ref, root: u64) -> Option<Child> {
        let _kernel = KernelAllocationContext::enter();
        let preparer = Principal { cell_id: preparer.0, generation: preparer.1 };
        let child = Child { principal: Principal { cell_id: child.0, generation: child.1 },
            root_tid: root, publication_generation: 0x5E55_F004 };
        let deadline = (super::super::system_ticks() as u64).checked_add(PREPARE_TICKS)?;
        let state = &mut sched.cell_session;
        state.prepared.try_reserve(1).ok()?;
        state.receipts.try_reserve(1).ok()?;
        let receipt = state.mint().ok()?;
        state.prepared.push(Prepared { reference: child.prepared_ref(), child, preparer, operation, deadline,
            written: 0, context: Vec::new(), parsed: None, publication: None, state: 1, reason: 0, exit_code: 0,
            terminal_sequence: 0, kill: false, intent: None, receipts: [ZERO, receipt, ZERO],
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        state.receipts.push(Receipt { reference: receipt, prepared: child.prepared_ref(), child, operation,
            issuer: Binding::new(u64::MAX, u64::MAX, u64::MAX), kind: 2, slot: 4, object: ZERO,
            expected_epoch: 0, next_epoch: 0, digest: [0; 32], seal: [0; 256], seal_len: 0, state: 1,
            bound: true, producers: [ZERO; 2], restored: false, materialized: false,
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        Some(child)
    }
    /// Mark every receipt on `operation` materialized, as the issuer's
    /// `ResolveBindReceipt` action 2 does (the next `reclaim` then drops it).
    pub(crate) fn materialize_receipts(sched: &mut Scheduler, operation: Ref) {
        for rr in sched.cell_session.receipts.iter_mut().filter(|rr| rr.operation == operation) { rr.materialized = true; }
    }
    /// Whether the prepared record still exists.
    pub(crate) fn prepared_exists(sched: &Scheduler, prepared: Ref) -> bool {
        sched.cell_session.preparation(prepared).is_ok()
    }
    /// Stage an unpublished child of `(cell, generation)` on `operation` whose
    /// canonical, validated context carries one kind-6 grant naming `frontend`.
    pub(crate) fn stage_frontend_child(sched: &mut Scheduler, cell: u64, generation: u64, operation: Ref,
        root: u64, frontend: Ref) -> Option<Child> {
        let _kernel = KernelAllocationContext::enter();
        let preparer = Principal { cell_id: cell, generation };
        let child = Child { principal: Principal { cell_id: cell, generation: generation.wrapping_add(1) },
            root_tid: root, publication_generation: 0x5E55_F006 };
        let mut context = Vec::new();
        context.try_reserve_exact(88).ok()?;
        for word in [1u32, 88, 0, 0] { context.extend_from_slice(&word.to_le_bytes()); }
        context.extend_from_slice(&0u16.to_le_bytes());
        context.extend_from_slice(&1u16.to_le_bytes());
        for word in [0u32; 3] { context.extend_from_slice(&word.to_le_bytes()); }
        context.extend_from_slice(&operation.to_bytes());
        for half in [GRANT_KIND_INPUT_FRONTEND, 0, SLOT_FRONTEND, 0] { context.extend_from_slice(&half.to_le_bytes()); }
        context.extend_from_slice(&(api::syscall::service::INPUT as u64).to_le_bytes());
        context.extend_from_slice(&frontend.to_bytes());
        context.extend_from_slice(&0u64.to_le_bytes());
        let parsed = child_context::validate(&context, operation).ok()?;
        let deadline = (super::super::system_ticks() as u64).checked_add(PREPARE_TICKS)?;
        let state = &mut sched.cell_session;
        state.prepared.try_reserve(1).ok()?;
        state.prepared.push(Prepared { reference: child.prepared_ref(), child, preparer, operation, deadline,
            written: 88, context, parsed: Some(parsed), publication: None, state: 0, reason: 0, exit_code: 0,
            terminal_sequence: 0, kill: false, intent: None, receipts: [ZERO; 3],
            charge: Charge { cell: cell as usize, bytes: 0 } });
        Some(child)
    }
    /// Record an unbound kind-6 receipt on `prepared` sealed for `child` and
    /// `frontend`, as `RegisterBindReceipt` would for the fixture issuer.
    pub(crate) fn stage_frontend_receipt(sched: &mut Scheduler, prepared: Ref, child: Child, frontend: Ref) -> Option<Ref> {
        let _kernel = KernelAllocationContext::enter();
        let state = &mut sched.cell_session;
        let pi = state.preparation(prepared).ok()?;
        let (operation, preparer, digest) = (state.prepared[pi].operation, state.prepared[pi].preparer,
            state.prepared[pi].parsed.as_ref()?.digest);
        state.receipts.try_reserve(1).ok()?;
        let receipt = state.mint().ok()?;
        let mut seal = [0u8; 256];
        seal[..16].copy_from_slice(&frontend.to_bytes());
        seal[16..48].copy_from_slice(&child.to_bytes());
        state.receipts.push(Receipt { reference: receipt, prepared, child, operation,
            issuer: Binding::new(u64::MAX, u64::MAX, u64::MAX), kind: GRANT_KIND_INPUT_FRONTEND, slot: SLOT_FRONTEND,
            object: frontend, expected_epoch: 0, next_epoch: 0, digest, seal, seal_len: FRONTEND_SEAL_LEN, state: 0,
            bound: false, producers: [ZERO; 2], restored: false, materialized: false,
            charge: Charge { cell: preparer.cell_id as usize, bytes: 0 } });
        Some(receipt)
    }
    /// Name `receipt` in `prepared`'s frontend slot and mark it bound, bypassing
    /// `BindOwnerReceipt`'s own checks (the commit gate is the subject).
    pub(crate) fn force_frontend_bind(sched: &mut Scheduler, prepared: Ref, receipt: Ref) -> Option<()> {
        let state = &mut sched.cell_session;
        let pi = state.preparation(prepared).ok()?;
        let ri = state.receipt_index(receipt).ok()?;
        state.prepared[pi].receipts[2] = receipt;
        state.receipts[ri].bound = true;
        Some(())
    }
    /// Frontend slot of a prepared record.
    pub(crate) fn frontend_receipt(sched: &Scheduler, prepared: Ref) -> Option<Ref> {
        sched.cell_session.preparation(prepared).ok().map(|i| sched.cell_session.prepared[i].receipts[2])
    }
    /// Remove every record of the fixture children and their receipts.
    pub(crate) fn discard_all(sched: &mut Scheduler, prepared: &[Ref]) {
        sched.cell_session.receipts.retain(|rr| !prepared.contains(&rr.prepared));
        sched.cell_session.prepared.retain(|p| !prepared.contains(&p.reference));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FRONTEND: Ref = Ref { id: 41, generation: 3 };
    const CHILD: Child = Child { principal: Principal { cell_id: 12, generation: 5 }, root_tid: 77, publication_generation: 9 };

    fn seal(frontend: Ref, child: Child) -> [u8; FRONTEND_SEAL_LEN] {
        let mut bytes = [0u8; FRONTEND_SEAL_LEN];
        bytes[..16].copy_from_slice(&frontend.to_bytes());
        bytes[16..].copy_from_slice(&child.to_bytes());
        bytes
    }

    #[test]
    fn frontend_seal_names_exact_child_and_frontend() {
        assert_eq!(check_frontend_seal(&seal(FRONTEND, CHILD), FRONTEND, Some(FRONTEND), CHILD), Ok(()));
        // A receipt sealed for another child, or another generation of it, is refused.
        let other = Child { root_tid: 78, ..CHILD };
        assert_eq!(check_frontend_seal(&seal(FRONTEND, other), FRONTEND, Some(FRONTEND), CHILD), Err(Status::InvalidChild));
        let older = Child { principal: Principal { generation: 4, ..CHILD.principal }, ..CHILD };
        assert_eq!(check_frontend_seal(&seal(FRONTEND, older), FRONTEND, Some(FRONTEND), CHILD), Err(Status::InvalidChild));
        // The object, the seal and the context grant must all name one frontend.
        let foreign = Ref { id: 42, generation: 3 };
        assert_eq!(check_frontend_seal(&seal(foreign, CHILD), FRONTEND, Some(FRONTEND), CHILD), Err(Status::Denied));
        assert_eq!(check_frontend_seal(&seal(FRONTEND, CHILD), foreign, Some(FRONTEND), CHILD), Err(Status::Denied));
        assert_eq!(check_frontend_seal(&seal(FRONTEND, CHILD), FRONTEND, Some(foreign), CHILD), Err(Status::Denied));
        // No slot-8 grant in the context: nothing to admit.
        assert_eq!(check_frontend_seal(&seal(FRONTEND, CHILD), FRONTEND, None, CHILD), Err(Status::Denied));
        assert_eq!(check_frontend_seal(&seal(ZERO, CHILD), ZERO, Some(ZERO), CHILD), Err(Status::Denied));
        // Exact length: short or trailing bytes are malformed.
        assert_eq!(check_frontend_seal(&seal(FRONTEND, CHILD)[..47], FRONTEND, Some(FRONTEND), CHILD), Err(Status::Malformed));
        let mut long = [0u8; 49];
        long[..48].copy_from_slice(&seal(FRONTEND, CHILD));
        assert_eq!(check_frontend_seal(&long, FRONTEND, Some(FRONTEND), CHILD), Err(Status::Malformed));
    }

    #[test]
    fn frontend_receipts_belong_to_input() {
        assert_eq!(receipt_service(GRANT_KIND_INPUT_FRONTEND), api::syscall::service::INPUT);
        assert_eq!(receipt_service(1), api::syscall::service::INPUT);
        assert_eq!(receipt_service(2), api::syscall::service::VFS);
    }
}
