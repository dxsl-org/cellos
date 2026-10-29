//! Prepared cwd admission in the existing directory/owner/quota lifetime.
//!
//! Entries are minted once, linked to the source, and sealed but unpublished.
//! The kernel's immutable issuer receipt is the only publication decision. Every
//! ordinary ingress reconciles that decision before looking up a directory.
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use api::cell_session::{ChildRefV1, KernelOp, ObjectRefV1, PrincipalV1, Reader,
    RequestHeaderV1, ResultHeaderV1, Status, Writer, HEADER_LEN, MAX_SERVICE_RECORD_LEN};
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsResponse, IPC_BUF_SIZE};
use sha2::{Digest, Sha256};
use types::CellId;

use crate::caller::Caller;
use crate::dirs::DirError;
use crate::manager::VfsManager;

type Scope = (u64, u64, u64, u64);
const MAX_LOGS: usize = 512;
const MAX_CREATOR_LOGS: usize = 64;
const MAX_OUTCOMES: usize = 64;
const MAX_RESERVATIONS: usize = 16;
const OUTCOME_CHARGE: u64 = 256;
const LOG_CHARGE: u64 = 256;
const RESERVATION_CHARGE: u64 = 512;
const ZERO: ObjectRefV1 = ObjectRefV1 { id: 0, generation: 0 };

#[derive(Default)]
pub(crate) struct PreparedCwd {
    next: u64,
    reservations: BTreeMap<u64, Reservation>,
    logs: BTreeMap<Scope, RequestLog>,
}

struct Reservation {
    reference: ObjectRefV1,
    owner: Caller,
    prepared: ObjectRefV1,
    child: ChildRefV1,
    operation: ObjectRefV1,
    source: ObjectRefV1,
    grant: ObjectRefV1,
    receipt: ObjectRefV1,
    state: u32,
    charge: u64,
    register: Option<KernelCall>,
    resolving: Option<KernelCall>,
    resolve_action: u32,
    retired: bool,
    reconciled: bool,
    resolved_abort: Option<u32>,
    terminal_ack: bool,
}

#[derive(Default)]
struct RequestLog {
    next: u64,
    acknowledged: u64,
    outcomes: BTreeMap<u64, Outcome>,
    kernel_next: u64,
    kernel_pending: BTreeSet<u64>,
    kernel_acked: u64,
}

impl RequestLog {
    /// Highest kernel mutation ID that may be acknowledged: everything below the
    /// oldest unresolved one.
    fn ack_through(&self) -> u64 {
        self.kernel_pending.iter().next().copied().unwrap_or(self.kernel_next).saturating_sub(1)
    }
}

struct Outcome {
    request: Vec<u8>,
    result: [u8; 64],
    len: usize,
    pending: bool,
    observed: bool,
}

struct KernelCall {
    id: u64,
    selector: u16,
    digest: [u8; 32],
    // Retained so a query-only retry can resend the identical request when the kernel
    // proves the ID was never consumed.
    body: Vec<u8>,
}

fn scope(owner: Caller, operation: ObjectRefV1) -> Scope {
    (owner.cell.0, owner.generation, operation.id, operation.generation)
}
fn principal(owner: Caller) -> PrincipalV1 {
    PrincipalV1 { cell_id: owner.cell.0, generation: owner.generation }
}
fn child_owner(child: ChildRefV1) -> Caller {
    Caller::principal(CellId(child.principal.cell_id), child.principal.generation)
}
fn nonzero(reference: ObjectRefV1) -> bool { reference.id != 0 && reference.generation != 0 }
fn hash(bytes: &[u8]) -> [u8; 32] { Sha256::digest(bytes).into() }
fn dir_error(error: DirError) -> Status {
    match error {
        DirError::TooManyHandles => Status::QuotaExceeded,
        DirError::UnknownHandle => Status::Denied,
        DirError::BadName(_) => Status::UnsupportedInput,
    }
}

fn kernel_query(op: KernelOp, body: &[u8], out: &mut [u8]) -> Result<usize, Status> {
    let h = ostd::session::kernel_request(op, 1, body, out)?;
    if h.status != Status::Ok { return Err(h.status); }
    Ok(h.total_len as usize)
}

/// Ask the kernel for the retained outcome of one mutation ID. `Some` is the retained
/// original record (the ID was consumed); `None` is a definite UnknownRequest (it was
/// not); anything else is uncertain.
fn query_retained(operation: ObjectRefV1, id: u64, selector: u16, out: &mut [u8]) -> Result<Option<ResultHeaderV1>, Status> {
    let mut query = [0u8; 24];
    let mut w = Writer::new(&mut query);
    w.reference(operation)?;
    w.u64(id)?;
    let h = ostd::session::kernel_request(KernelOp::QueryRequest, 1, &query, out)?;
    if h.selector == selector && h.request_id == id { return Ok(Some(h)); }
    if h.status == Status::UnknownRequest { return Ok(None); }
    Err(Status::Indeterminate)
}

fn identity(prepared: ObjectRefV1) -> Result<(ChildRefV1, ObjectRefV1, PrincipalV1), Status> {
    let mut result = [0u8; 88];
    let len = kernel_query(KernelOp::InspectPreparedIdentity, &prepared.to_bytes(), &mut result)?;
    let mut r = Reader::new(&result[HEADER_LEN..len]);
    let child = r.child()?;
    let operation = r.reference()?;
    let preparer = r.principal()?;
    r.finish()?;
    Ok((child, operation, preparer))
}

impl PreparedCwd {
    fn log(&mut self, vfs: &mut VfsManager, owner: Caller, operation: ObjectRefV1) -> Result<(), Status> {
        let key = scope(owner, operation);
        if self.logs.contains_key(&key) { return Ok(()); }
        if self.logs.len() >= MAX_LOGS || self.logs.keys().filter(|k| k.0 == key.0 && k.1 == key.1).count() >= MAX_CREATOR_LOGS {
            return Err(Status::QuotaExceeded);
        }
        vfs.quota.charge(owner.cell, LOG_CHARGE).map_err(|_| Status::QuotaExceeded)?;
        self.logs.insert(key, RequestLog { next: 1, kernel_next: 1, ..RequestLog::default() });
        Ok(())
    }

    // Mutation IDs survive transport uncertainty. An existing ID is queried before it is
    // ever submitted again, even if a service-level caller retries its request. The kernel
    // consumes an ID only after a request is admitted to its ledger, so a refusal that left
    // no retained outcome (QueryRequest answers UnknownRequest) did not consume it: the
    // local counter is not advanced past it and the same ID is reused.
    fn kernel_mutation(&mut self, key: Scope, call: &mut Option<KernelCall>, op: KernelOp,
        body: &[u8], out: &mut [u8]) -> Result<usize, Status> {
        let fingerprint = hash(body);
        let log = self.logs.get_mut(&key).ok_or(Status::Stale)?;
        let operation = ObjectRefV1 { id: key.2, generation: key.3 };
        let mut retained = false;
        let header = if let Some(saved) = call.as_ref() {
            if saved.selector != op as u16 || (!body.is_empty() && saved.digest != fingerprint) { return Err(Status::Indeterminate); }
            // QueryRequest returns the original record, not a nested envelope.
            match query_retained(operation, saved.id, saved.selector, out)? {
                Some(h) => { retained = true; h }
                None => ostd::session::kernel_request(op, saved.id, &saved.body, out)?,
            }
        } else {
            if !log.kernel_pending.is_empty() { return Err(Status::Indeterminate); }
            let id = log.kernel_next;
            let next = id.checked_add(1).ok_or(Status::QuotaExceeded)?;
            let mut saved_body = Vec::new();
            saved_body.try_reserve(body.len()).map_err(|_| Status::QuotaExceeded)?;
            saved_body.extend_from_slice(body);
            log.kernel_next = next;
            log.kernel_pending.insert(id);
            *call = Some(KernelCall { id, selector: op as u16, digest: fingerprint, body: saved_body });
            ostd::session::kernel_request(op, id, body, out)?
        };
        let saved = call.as_ref().ok_or(Status::Indeterminate)?;
        // Only the retained original result (or an exact echo of the submitted ID) resolves
        // an uncertain mutation. Query correlation errors never authorize re-execution.
        if header.selector != saved.selector || header.request_id != saved.id {
            return Err(Status::Indeterminate);
        }
        let id = saved.id; let selector = saved.selector;
        if header.status != Status::Ok && !retained {
            // A non-Ok echo is either a retained terminal outcome (ID consumed) or a
            // pre-ledger refusal (ID untouched). Only the kernel's retained record decides.
            match query_retained(operation, id, selector, out) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    log.kernel_pending.remove(&id);
                    if log.kernel_next == id.saturating_add(1) { log.kernel_next = id; }
                    *call = None;
                    return Err(header.status);
                }
                Err(_) => return Err(Status::Indeterminate),
            }
        }
        log.kernel_pending.remove(&id);
        *call = None;
        if header.status != Status::Ok { return Err(header.status); }
        Ok(header.total_len as usize)
    }

    fn flush_kernel_acks(&mut self) {
        for (key, log) in &mut self.logs {
            let through = log.ack_through();
            if through <= log.kernel_acked { continue; }
            let mut body = [0u8; 24];
            let mut w = Writer::new(&mut body);
            if w.reference(ObjectRefV1 { id: key.2, generation: key.3 }).is_err() || w.u64(through).is_err() { continue; }
            let mut out = [0u8; HEADER_LEN];
            if kernel_query(KernelOp::AcknowledgeRequests, &body, &mut out).is_ok() { log.kernel_acked = through; }
        }
    }

    fn reserve(&mut self, vfs: &mut VfsManager, owner: Caller, prepared: ObjectRefV1,
        source: ObjectRefV1, operation: ObjectRefV1) -> Result<[u8; 32], Status> {
        let (child, actual_operation, preparer) = identity(prepared)?;
        if preparer != principal(owner) { return Err(Status::WrongOwner); }
        if actual_operation != operation { return Err(Status::Stale); }
        if source.generation != owner.generation { return Err(Status::Stale); }
        let path = vfs.dirs.dir_path(owner, ViDirHandle(source.id)).ok_or(Status::Denied)?;
        if !vfs.access.can_read(owner, path) { return Err(Status::Denied); }
        if !vfs.healthy(path) || !matches!(vfs.stat(path), Some((_, true))) { return Err(Status::Io); }
        if self.reservations.values().any(|r| r.prepared == prepared)
            || self.reservations.values().filter(|r| r.owner == owner && r.operation == operation).count() >= MAX_RESERVATIONS
            || self.reservations.len() >= MAX_LOGS * MAX_RESERVATIONS {
            return Err(Status::QuotaExceeded);
        }
        let charge = RESERVATION_CHARGE + path.len() as u64;
        let mut binding = [0u8; api::service_binding::SERVICE_BINDING_LEN];
        let issuer = ostd::syscall::sys_lookup_service_bound(api::syscall::service::VFS, &mut binding).ok_or(Status::Revoked)?;
        let id = if self.next == 0 { 1 } else { self.next };
        self.next = id.checked_add(1).ok_or(Status::QuotaExceeded)?;
        vfs.quota.charge(owner.cell, charge).map_err(|_| Status::QuotaExceeded)?;
        let mut revoked = Vec::new();
        let reserved = vfs.dirs.reserve_prepared(owner, child_owner(child), ViDirHandle(source.id), &mut revoked);
        // Replacing an older generation of the child cell revoked its directories; the
        // file handles opened through them die with them, even if this reservation fails.
        let _ = vfs.files.revoke_by_parent_dirs(&revoked);
        let handle = match reserved {
            Ok(handle) => handle,
            Err(error) => { vfs.quota.release(owner.cell, charge); return Err(dir_error(error)); }
        };
        let reference = ObjectRefV1 { id, generation: issuer.generation };
        let grant = ObjectRefV1 { id: handle.0, generation: child.principal.generation };
        self.reservations.insert(id, Reservation { reference, owner, prepared, child, operation,
            source, grant, receipt: ZERO, state: 0, charge, register: None, resolving: None,
            resolve_action: 0, retired: false, reconciled: false, resolved_abort: None, terminal_ack: false });
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&reference.to_bytes());
        out[16..].copy_from_slice(&grant.to_bytes());
        Ok(out)
    }

    fn finalize(&mut self, vfs: &mut VfsManager, reservation: &mut Reservation) -> Result<ObjectRefV1, Status> {
        if nonzero(reservation.receipt) { return Ok(reservation.receipt); }
        if reservation.register.is_some() {
            let mut reply = [0u8; 40];
            let len = self.kernel_mutation(scope(reservation.owner, reservation.operation), &mut reservation.register,
                KernelOp::RegisterBindReceipt, &[], &mut reply)?;
            let mut r = Reader::new(&reply[HEADER_LEN..len]);
            let receipt = r.reference()?; r.finish()?;
            if !nonzero(receipt) { return Err(Status::Malformed); }
            reservation.receipt = receipt;
            return Ok(receipt);
        }
        if reservation.retired { return Err(Status::Revoked); }
        let path = vfs.dirs.dir_path(reservation.owner, ViDirHandle(reservation.source.id)).ok_or(Status::Revoked)?;
        let expected_path_digest = hash(path.as_bytes());
        let expected_path_len = path.len();
        let mut offset = 0usize;
        let mut cwd_hash = Sha256::new();
        let mut context_digest = [0u8; 32];
        loop {
            let mut body = [0u8; 32];
            let mut w = Writer::new(&mut body);
            w.reference(reservation.prepared)?;
            w.u16(2)?; w.u16(4)?; w.u32(offset as u32)?; w.u32(512)?; w.u32(0)?;
            let mut reply = [0u8; 680];
            let len = kernel_query(KernelOp::InspectPreparedGrant, &body, &mut reply)?;
            let mut r = Reader::new(&reply[HEADER_LEN..len]);
            if r.child()? != reservation.child || r.reference()? != reservation.operation { return Err(Status::InvalidChild); }
            let digest = r.take(32)?;
            if offset == 0 { context_digest.copy_from_slice(digest); }
            else if digest != context_digest { return Err(Status::Stale); }
            let grant = r.take(40)?;
            validate_grant(grant, reservation.grant)?;
            let total = r.u32()? as usize;
            let got_offset = r.u32()? as usize;
            let count = r.u32()? as usize;
            if total != expected_path_len { return Err(Status::UnsupportedInput); }
            if r.u32()? != 0 || got_offset != offset || count > 512 || count > total.saturating_sub(offset) {
                return Err(Status::Malformed);
            }
            let bytes = r.take(count)?;
            r.finish()?;
            // Compare unchanged bytes with the already representable backend path.
            // Never decode lossy, normalize, or resolve against an ambient cwd.
            if bytes != &path.as_bytes()[offset..offset + count] { return Err(Status::UnsupportedInput); }
            cwd_hash.update(bytes);
            offset += count;
            if offset == total { break; }
            if count == 0 { return Err(Status::Malformed); }
        }
        let cwd_digest: [u8; 32] = cwd_hash.finalize().into();
        if cwd_digest != expected_path_digest { return Err(Status::Malformed); }
        let mut seal = [0u8; 128];
        let mut w = Writer::new(&mut seal);
        w.principal(principal(reservation.owner))?;
        w.reference(reservation.source)?; w.child(reservation.child)?;
        w.reference(reservation.operation)?; w.bytes(&cwd_digest)?; w.reference(reservation.grant)?;
        let mut body = [0u8; 224];
        let mut w = Writer::new(&mut body);
        w.reference(reservation.prepared)?; w.u16(2)?; w.u16(4)?; w.u32(0)?;
        w.reference(reservation.grant)?; w.u64(0)?; w.u64(0)?;
        w.bytes(&context_digest)?; w.u32(128)?; w.u32(0)?; w.bytes(&seal)?;
        let mut reply = [0u8; 40];
        let len = self.kernel_mutation(scope(reservation.owner, reservation.operation), &mut reservation.register,
            KernelOp::RegisterBindReceipt, &body, &mut reply)?;
        let mut r = Reader::new(&reply[HEADER_LEN..len]);
        let receipt = r.reference()?; r.finish()?;
        if !nonzero(receipt) { return Err(Status::Malformed); }
        reservation.receipt = receipt;
        Ok(receipt)
    }

    fn resolve(&mut self, vfs: &mut VfsManager, reservation: &mut Reservation, action: u32) -> Result<u32, Status> {
        if action == 1 {
            if let Some(state) = reservation.resolved_abort { return Ok(state); }
        }
        let mut body = [0u8; 24];
        let mut w = Writer::new(&mut body);
        w.reference(reservation.receipt)?; w.u32(action)?; w.u32(0)?;
        let mut reply = [0u8; 96];
        let len = if action == 0 {
            kernel_query(KernelOp::ResolveBindReceipt, &body, &mut reply)?
        } else {
            if reservation.resolving.is_some() && reservation.resolve_action != action { return Err(Status::Indeterminate); }
            reservation.resolve_action = action;
            self.kernel_mutation(scope(reservation.owner, reservation.operation), &mut reservation.resolving,
                KernelOp::ResolveBindReceipt, &body, &mut reply)?
        };
        let mut r = Reader::new(&reply[HEADER_LEN..len]);
        let state = r.u32()?;
        if r.u32()? != 0 || r.child()? != reservation.child || r.reference()? != ZERO || r.reference()? != ZERO || state > 3 {
            return Err(Status::Malformed);
        }
        r.finish()?;
        if action == 1 { reservation.resolved_abort = Some(state); }
        if action == 2 && reservation.retired { reservation.terminal_ack = true; }
        reservation.reconciled = true;
        reservation.state = state;
        if state == 1 && !reservation.retired {
            // All namespace/grant storage already exists; no allocation/backend
            // check, legacy inheritance, or fresh authority decision at this point.
            let _ = vfs.dirs.publish_prepared(child_owner(reservation.child), reservation.grant.id);
        } else if state >= 2 {
            retire(vfs, reservation);
        }
        Ok(state)
    }

    fn execute(&mut self, vfs: &mut VfsManager, owner: Caller, operation: ObjectRefV1,
        selector: u16, body: &[u8], payload: &mut [u8; 40]) -> Result<usize, Status> {
        let mut r = Reader::new(body);
        match selector {
            1 => {
                let prepared = r.reference()?; let source = r.reference()?; r.finish()?;
                let result = self.reserve(vfs, owner, prepared, source, operation)?;
                payload[..32].copy_from_slice(&result); Ok(32)
            }
            2 | 3 => {
                let reference = r.reference()?;
                let (prepared, action) = if selector == 2 { (r.reference()?, 0) }
                    else { let action = r.u32()?; if action > 2 || r.u32()? != 0 { return Err(Status::Malformed); } (ZERO, action) };
                r.finish()?;
                let id = self.reservations.iter().find_map(|(&id, value)| {
                    let matches = if selector == 2 { value.reference == reference } else { value.receipt == reference };
                    (matches && value.owner == owner && value.operation == operation).then_some(id)
                }).ok_or(Status::Stale)?;
                let mut value = self.reservations.remove(&id).ok_or(Status::Stale)?;
                let result = if selector == 2 {
                    if value.prepared != prepared { Err(Status::InvalidChild) }
                    else { self.finalize(vfs, &mut value).map(|receipt| { payload[..16].copy_from_slice(&receipt.to_bytes()); 16 }) }
                } else {
                    self.resolve(vfs, &mut value, action).map(|state| {
                        payload[..4].copy_from_slice(&state.to_le_bytes());
                        payload[4..8].fill(0); payload[8..24].copy_from_slice(&value.grant.to_bytes()); 24
                    })
                };
                self.reservations.insert(id, value);
                result
            }
            _ => Err(Status::UnknownOperation),
        }
    }
}

fn validate_grant(bytes: &[u8], expected: ObjectRefV1) -> Result<(), Status> {
    let mut r = Reader::new(bytes);
    if r.u16()? != 2 || r.u16()? != 0 || r.u16()? != 4 || r.u16()? != 0
        || r.u64()? != api::syscall::service::VFS as u64 || r.reference()? != expected || r.u64()? != 0 {
        return Err(Status::Denied);
    }
    r.finish()
}

fn retire(vfs: &mut VfsManager, value: &mut Reservation) {
    if value.retired { return; }
    let removed = vfs.dirs.revoke_ids(&[value.grant.id]);
    let _ = vfs.files.revoke_by_parent_dirs(&removed.revoked_ids);
    vfs.quota.release(value.owner.cell, value.charge.saturating_sub(RESERVATION_CHARGE));
    value.charge = RESERVATION_CHARGE;
    value.retired = true;
    // Keep the generation's seal. A retired grant must not reopen path strings.
}

fn encode_result(out: &mut [u8], selector: u16, request_id: u64, status: Status, payload: &[u8]) -> usize {
    let len = HEADER_LEN + payload.len();
    let header = ResultHeaderV1 { selector, total_len: len as u32, request_id, status };
    if header.encode(&mut out[..HEADER_LEN]).is_err() { return 0; }
    out[HEADER_LEN..len].copy_from_slice(payload);
    len
}

fn operation_for(state: &PreparedCwd, owner: Caller, selector: u16, body: &[u8]) -> Result<ObjectRefV1, Status> {
    let mut r = Reader::new(body);
    let reference = r.reference()?;
    match selector {
        1 => {
            let _ = r.reference()?; r.finish()?;
            if let Some(value) = state.reservations.values().find(|v| v.owner == owner && v.prepared == reference) {
                return Ok(value.operation);
            }
            if let Some((key, _)) = state.logs.iter().find(|(key, log)| {
                key.0 == owner.cell.0 && key.1 == owner.generation && log.outcomes.values().any(|o| {
                    o.request.get(4..6) == Some(&1u16.to_le_bytes()[..])
                        && o.request.get(HEADER_LEN..HEADER_LEN + 16) == Some(&reference.to_bytes()[..])
                })
            }) {
                return Ok(ObjectRefV1 { id: key.2, generation: key.3 });
            }
            let (_, operation, preparer) = identity(reference).map_err(|status| {
                if matches!(status, Status::Stale | Status::InvalidChild | Status::NotFound | Status::TimedOut) {
                    Status::UnknownRequest
                } else { status }
            })?;
            if preparer != principal(owner) { return Err(Status::WrongOwner); }
            Ok(operation)
        }
        2 | 3 => state.reservations.values().find(|v| v.owner == owner &&
            (if selector == 2 { v.reference == reference } else { v.receipt == reference }))
            .map(|v| v.operation).ok_or(Status::UnknownRequest),
        4 | 5 => { let _ = r.u64()?; r.finish()?; Ok(reference) }
        _ => Err(Status::UnknownOperation),
    }
}

pub fn handle<'a>(vfs: &mut VfsManager, owner: Caller, record: &[u8], out: &'a mut [u8; IPC_BUF_SIZE]) -> VfsResponse<'a> {
    let header = match RequestHeaderV1::decode(record) {
        Ok(h) if record.len() <= MAX_SERVICE_RECORD_LEN => h,
        Ok(h) => {
            let len = encode_result(out, h.selector, h.request_id, Status::Malformed, &[]);
            return VfsResponse::PreparedChildV1(&out[..len]);
        }
        Err(status) => {
            let selector = record.get(4..6).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0);
            let id = record.get(16..24).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8]))).unwrap_or(0);
            let len = encode_result(out, selector, id, status, &[]);
            return VfsResponse::PreparedChildV1(&out[..len]);
        }
    };
    let mut state = core::mem::take(&mut vfs.dirs.prepared);
    let result = serve(&mut state, vfs, owner, header, record, out);
    state.flush_kernel_acks();
    vfs.dirs.prepared = state;
    let len = match result {
        Ok(len) => len,
        Err(status) => encode_result(out, header.selector, header.request_id, status, &[]),
    };
    VfsResponse::PreparedChildV1(&out[..len])
}

fn serve(state: &mut PreparedCwd, vfs: &mut VfsManager, owner: Caller, header: RequestHeaderV1,
    record: &[u8], out: &mut [u8]) -> Result<usize, Status> {
    let body = &record[HEADER_LEN..];
    let operation = operation_for(state, owner, header.selector, body)?;
    if !nonzero(operation) { return Err(Status::Malformed); }
    let key = scope(owner, operation);
    if header.selector == 4 {
        let mut r = Reader::new(body); let _ = r.reference()?; let through = r.u64()?; r.finish()?;
        let log = state.logs.get_mut(&key).ok_or(Status::UnknownRequest)?;
        if through >= log.next || log.outcomes.range(..=through).any(|(_, o)| !o.observed || o.pending) { return Err(Status::UnknownRequest); }
        if through > log.acknowledged {
            let ids: Vec<u64> = log.outcomes.range(..=through).map(|(&id, _)| id).collect();
            for id in ids { log.outcomes.remove(&id); vfs.quota.release(owner.cell, OUTCOME_CHARGE); }
            log.acknowledged = through;
        }
        return Ok(encode_result(out, header.selector, header.request_id, Status::Ok, &[]));
    }
    if header.selector == 5 {
        let mut r = Reader::new(body); let _ = r.reference()?; let wanted = r.u64()?; r.finish()?;
        complete_pending(state, vfs, owner, operation, wanted)?;
        let outcome = state.logs.get_mut(&key).and_then(|l| l.outcomes.get_mut(&wanted)).ok_or(Status::UnknownRequest)?;
        if outcome.pending { return Err(Status::WouldBlock); }
        outcome.observed = true;
        out[..outcome.len].copy_from_slice(&outcome.result[..outcome.len]);
        return Ok(outcome.len);
    }
    if header.selector == 3 {
        let mut r = Reader::new(body);
        let _ = r.reference()?;
        let action = r.u32()?;
        if action != 1 {
            if action > 2 || r.u32()? != 0 { return Err(Status::Malformed); }
            r.finish()?;
            let mut payload = [0u8; 40];
            let count = state.execute(vfs, owner, operation, header.selector, body, &mut payload)?;
            return Ok(encode_result(out, header.selector, header.request_id, Status::Ok, &payload[..count]));
        }
    }
    state.log(vfs, owner, operation)?;
    let log = state.logs.get_mut(&key).ok_or(Status::Stale)?;
    if let Some(outcome) = log.outcomes.get_mut(&header.request_id) {
        if outcome.request != record { return Err(Status::Malformed); }
        if outcome.pending { return Err(Status::WouldBlock); }
        outcome.observed = true;
        out[..outcome.len].copy_from_slice(&outcome.result[..outcome.len]);
        return Ok(outcome.len);
    }
    if header.request_id != log.next { return Err(Status::UnknownRequest); }
    if log.outcomes.len() >= MAX_OUTCOMES { return Err(Status::QuotaExceeded); }
    let next = log.next.checked_add(1).ok_or(Status::QuotaExceeded)?;
    // Fixed schemas are small; reserve the bounded retained record before effects.
    if (header.selector == 1 || header.selector == 2) && body.len() != 32
        || header.selector == 3 && body.len() != 24 { return Err(Status::Malformed); }
    vfs.quota.charge(owner.cell, OUTCOME_CHARGE).map_err(|_| Status::QuotaExceeded)?;
    log.next = next;
    log.outcomes.insert(header.request_id, Outcome { request: record.to_vec(), result: [0; 64], len: 0, pending: true, observed: false });
    complete_pending(state, vfs, owner, operation, header.request_id)?;
    let outcome = state.logs.get_mut(&key).and_then(|l| l.outcomes.get_mut(&header.request_id)).ok_or(Status::UnknownRequest)?;
    if outcome.pending { return Err(Status::WouldBlock); }
    outcome.observed = true;
    out[..outcome.len].copy_from_slice(&outcome.result[..outcome.len]);
    Ok(outcome.len)
}

fn complete_pending(state: &mut PreparedCwd, vfs: &mut VfsManager, owner: Caller, operation: ObjectRefV1, id: u64) -> Result<(), Status> {
    let key = scope(owner, operation);
    let Some(mut outcome) = state.logs.get_mut(&key).and_then(|l| l.outcomes.remove(&id)) else { return Err(Status::UnknownRequest); };
    if outcome.pending {
        let header = RequestHeaderV1::decode(&outcome.request)?;
        let mut payload = [0u8; 40];
        let result = state.execute(vfs, owner, operation, header.selector, &outcome.request[HEADER_LEN..], &mut payload);
        let (status, count) = match result {
            Ok(count) => (Status::Ok, count),
            Err(Status::Indeterminate) => (Status::WouldBlock, 0),
            Err(status) => (status, 0),
        };
        outcome.pending = status == Status::WouldBlock && state.logs.get(&key).is_some_and(|l| !l.kernel_pending.is_empty());
        outcome.len = encode_result(&mut outcome.result, header.selector, id, status, &payload[..count]);
    }
    state.logs.get_mut(&key).ok_or(Status::Stale)?.outcomes.insert(id, outcome);
    Ok(())
}

/// Reconcile every bounded reservation. Failed transport leaves entries
/// unpublished/sealed, not accessible by an old handle. Used on the idle cadence;
/// requests reconcile only the reservations the caller is party to.
pub fn reconcile(vfs: &mut VfsManager) { reconcile_matching(vfs, None) }

/// Reconcile the reservations `caller` prepared or was prepared for, before any of
/// its filesystem requests. Unrelated callers' reservations cost nothing here.
pub fn reconcile_for(vfs: &mut VfsManager, caller: Caller) { reconcile_matching(vfs, Some(caller)) }

fn reconcile_matching(vfs: &mut VfsManager, only: Option<Caller>) {
    let mut state = core::mem::take(&mut vfs.dirs.prepared);
    let ids: Vec<u64> = state.reservations.iter()
        .filter(|(_, value)| only.is_none_or(|c| value.owner == c || child_owner(value.child) == c))
        .map(|(&id, _)| id).collect();
    for id in ids {
        let Some(mut value) = state.reservations.remove(&id) else { continue; };
        value.reconciled = false;
        if value.register.is_some() {
            let _ = state.finalize(vfs, &mut value);
        }
        if nonzero(value.receipt) {
            let action = if value.resolving.is_some() { value.resolve_action } else { 0 };
            if state.resolve(vfs, &mut value, action).is_err() { value.reconciled = false; }
            if value.state == 1 {
                value.reconciled = false;
                let mut result = [0u8; 88];
                if let Ok(len) = kernel_query(KernelOp::QueryReceiptChild, &value.receipt.to_bytes(), &mut result) {
                    let mut r = Reader::new(&result[HEADER_LEN..len]);
                    let proof = (|| -> Result<u32, Status> {
                        if r.child()? != value.child { return Err(Status::InvalidChild); }
                        let child_state = r.u32()?;
                        if child_state > 5 || r.u32()? > 6 { return Err(Status::Malformed); }
                        let _ = r.i32()?;
                        if r.u32()? != 0 { return Err(Status::Malformed); }
                        let sequence = r.u64()?;
                        r.finish()?;
                        if child_state >= 3 && sequence == 0 { return Err(Status::Malformed); }
                        Ok(child_state)
                    })();
                    if let Ok(child_state) = proof {
                        if child_state >= 3 { retire(vfs, &mut value); }
                        else { value.reconciled = child_state == 1; }
                    }
                }
            }
        } else if identity(value.prepared).is_err_and(|status| matches!(status, Status::Stale | Status::InvalidChild | Status::Cancelled | Status::Revoked | Status::TimedOut | Status::NotFound)) {
            retire(vfs, &mut value);
        }
        state.reservations.insert(id, value);
    }
    reclaim(&mut state, vfs);
    // Root-lifetime retirement uses the same owner table as directory handles.
    let dead: Vec<Scope> = state.logs.keys().copied().filter(|key| {
        !vfs.dirs.has_met(Caller::principal(CellId(key.0), key.1))
    }).collect();
    for key in &dead {
        if let Some(log) = state.logs.get_mut(key) {
            vfs.quota.release(CellId(key.0), log.outcomes.len() as u64 * OUTCOME_CHARGE);
            log.outcomes.clear();
        }
    }
    // A committed child may outlive its parent. Its revoked directory namespace
    // stays sealed until exact terminal-child proof, never just until parent
    // death. Keep the issuer lane too if a kernel mutation is still unresolved.
    reclaim(&mut state, vfs);
    for key in dead {
        if !state.reservations.values().any(|value| scope(value.owner, value.operation) == key)
            && state.logs.get(&key).is_some_and(|log| log.kernel_pending.is_empty())
        {
            if state.logs.remove(&key).is_some() { vfs.quota.release(CellId(key.0), LOG_CHARGE); }
        }
    }
    state.flush_kernel_acks();
    vfs.dirs.prepared = state;
}

/// Linearize source revocation against receipt commit before changing lineage.
/// Unregistered reservations have no commit authority and can retire directly.
pub fn before_revoke(vfs: &mut VfsManager, owner: Caller, source: ViDirHandle) -> bool {
    if vfs.dirs.owned(owner, source).is_none() { return true; }
    let mut state = core::mem::take(&mut vfs.dirs.prepared);
    let ids: Vec<u64> = state.reservations.iter().filter_map(|(&id, value)| {
        let mut cursor = Some(value.source.id);
        while let Some(handle) = cursor {
            if handle == source.0 { return Some(id); }
            cursor = vfs.dirs.entries.get(&handle).and_then(|e| e.parent);
        }
        None
    }).collect();
    let mut settled = true;
    for id in ids {
        let Some(mut value) = state.reservations.remove(&id) else { continue; };
        if value.state == 0 && !value.retired {
            if value.register.is_some() && state.finalize(vfs, &mut value).is_err() { settled = false; }
            if nonzero(value.receipt) {
                if state.resolve(vfs, &mut value, 1).is_err() { settled = false; }
            } else if value.register.is_none() { retire(vfs, &mut value); }
        }
        state.reservations.insert(id, value);
    }
    state.flush_kernel_acks();
    vfs.dirs.prepared = state;
    settled
}

// Local records remain charged until both the caller's outcomes and the exact
// kernel terminal receipt have been acknowledged. IDs and request fences never
// return to an allocation pool.
fn reclaim(state: &mut PreparedCwd, vfs: &mut VfsManager) {
    let ids: Vec<u64> = state.reservations.iter().filter_map(|(&id, value)| {
        let ready = value.retired && state.logs.get(&scope(value.owner, value.operation))
            .is_some_and(|log| log.outcomes.is_empty() && log.kernel_pending.is_empty());
        ready.then_some(id)
    }).collect();
    for id in ids {
        let Some(mut value) = state.reservations.remove(&id) else { continue; };
        let resolved = !nonzero(value.receipt) || value.terminal_ack
            || state.resolve(vfs, &mut value, 2).is_ok();
        state.flush_kernel_acks();
        let acknowledged = state.logs.get(&scope(value.owner, value.operation))
            .is_some_and(|log| log.kernel_pending.is_empty() && log.kernel_acked >= log.kernel_next.saturating_sub(1));
        if resolved && acknowledged {
            vfs.quota.release(value.owner.cell, value.charge);
            let child = child_owner(value.child);
            if vfs.dirs.state(child).is_some_and(|s| s.handles == 0) {
                vfs.dirs.cells.remove(&child.cell.0);
            }
        } else {
            state.reservations.insert(id, value);
        }
    }
}

/// Even a transiently unavailable receipt owner cannot make an un-reconciled
/// child fall back to ambient path authority or retain cached live authority.
pub fn ready(vfs: &VfsManager, caller: Caller) -> bool {
    vfs.dirs.prepared.reservations.values().filter(|value| child_owner(value.child) == caller)
        .all(|value| value.reconciled && value.state == 1 && !value.retired)
}

/// Whether the idle `reconcile` cadence can still change anything. `false` means every
/// pending change arrives as a message (a request from the owner or child, or the
/// owner-death notification of a watched principal), so the receive loop may block.
///
/// A reservation is settled while it waits only on such a message: retired with
/// outcomes the owner has not acknowledged, or committed with its child proven running
/// and either principal watched. A watched child's exit arrives as its own death
/// notification. A child that never contacts VFS cannot be watched (the kernel watches
/// only the current receive principal), so its exit is silent; its owner is watched
/// instead. A live owner cannot close the child's Operation while the receipt is
/// unresolved, and its SDK contacts VFS once the child is reaped (the cwd lease), and
/// every request first reconciles the caller's reservations. The owner's death arrives
/// as a notification and cancels its Operations, which ends the child; from then on
/// neither principal is watched and the cadence polls until the child's terminal state
/// is observed. In-flight kernel mutations, unregistered or uncommitted receipts (the
/// preparation can expire), an unproven child, unflushed acknowledgements and dead
/// owners' logs all need the kernel polled.
pub fn idle_work(vfs: &VfsManager) -> bool {
    let state = &vfs.dirs.prepared;
    let unsettled = |value: &Reservation| {
        if value.register.is_some() || value.resolving.is_some() { return true; }
        if value.retired {
            return state.logs.get(&scope(value.owner, value.operation))
                .is_some_and(|log| log.outcomes.is_empty());
        }
        let watched = vfs.watches_owner(child_owner(value.child)) || vfs.watches_owner(value.owner);
        !(value.state == 1 && value.reconciled && watched)
    };
    let log_work = |key: &Scope, log: &RequestLog| {
        if log.ack_through() > log.kernel_acked { return true; }
        if vfs.dirs.has_met(Caller::principal(CellId(key.0), key.1)) { return false; }
        !log.outcomes.is_empty()
            || (log.kernel_pending.is_empty()
                && !state.reservations.values().any(|value| scope(value.owner, value.operation) == *key))
    };
    state.reservations.values().any(unsettled) || state.logs.iter().any(|(key, log)| log_work(key, log))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dirs::DirTable;

    const PARENT: Caller = Caller::principal(CellId(20), 7);
    const CHILD: Caller = Caller::principal(CellId(21), 9);

    #[test]
    fn prepared_directory_is_fresh_sealed_and_unusable_until_materialized() {
        let mut dirs = DirTable::new();
        dirs.on_contact(PARENT);
        let source = dirs.open_root(PARENT, "/tmp").unwrap();
        let child = dirs.reserve_prepared(PARENT, CHILD, source, &mut Vec::new()).unwrap();
        assert_ne!(source, child);
        assert!(dirs.is_sealed(CHILD));
        assert!(dirs.state(CHILD).unwrap().attested);
        assert_eq!(dirs.held_by(CHILD), 1);
        assert_eq!(dirs.dir_path(CHILD, child), None);
        assert!(dirs.resolve(CHILD, child, "entry").is_err());
        assert!(dirs.publish_prepared(CHILD, child.0));
        assert_eq!(dirs.dir_path(CHILD, child), Some("/tmp"));
        assert_eq!(dirs.dir_path(PARENT, child), None);
        assert_eq!(dirs.dir_path(CHILD, source), None);
    }

    #[test]
    fn source_revocation_retires_unpublished_lineage_without_ambient_fallback() {
        let mut dirs = DirTable::new();
        dirs.on_contact(PARENT);
        let source = dirs.open_root(PARENT, "/tmp").unwrap();
        let child = dirs.reserve_prepared(PARENT, CHILD, source, &mut Vec::new()).unwrap();
        let revoked = dirs.revoke(PARENT, source).unwrap();
        assert!(revoked.revoked_ids.contains(&child.0));
        assert!(!dirs.publish_prepared(CHILD, child.0));
        assert_eq!(dirs.held_by(CHILD), 0);
        assert!(dirs.is_sealed(CHILD));
    }

    #[test]
    fn reservation_rejects_foreign_generation_and_existing_child_namespace() {
        let mut dirs = DirTable::new();
        dirs.on_contact(PARENT);
        let source = dirs.open_root(PARENT, "/tmp").unwrap();
        let successor = Caller::principal(PARENT.cell, PARENT.generation + 1);
        assert_eq!(dirs.reserve_prepared(successor, CHILD, source, &mut Vec::new()), Err(DirError::UnknownHandle));
        assert!(!dirs.has_met(CHILD));
        let _ = dirs.reserve_prepared(PARENT, CHILD, source, &mut Vec::new()).unwrap();
        assert_eq!(dirs.reserve_prepared(PARENT, CHILD, source, &mut Vec::new()), Err(DirError::UnknownHandle));
        assert_eq!(dirs.held_by(CHILD), 1);
    }

    #[test]
    fn refused_reservation_never_removes_a_newer_child_generation() {
        let mut dirs = DirTable::new();
        dirs.on_contact(PARENT);
        let source = dirs.open_root(PARENT, "/tmp").unwrap();
        let newer = Caller::principal(CHILD.cell, CHILD.generation + 1);
        dirs.on_contact(newer);
        dirs.seal(newer);
        assert_eq!(dirs.reserve_prepared(PARENT, CHILD, source, &mut Vec::new()), Err(DirError::UnknownHandle));
        assert!(dirs.state(newer).is_some_and(|s| s.sealed));
    }

    #[test]
    fn cwd_projection_accepts_only_exact_owner_membership_grant() {
        let reference = ObjectRefV1 { id: 32, generation: 9 };
        let mut bytes = [0u8; 40];
        let mut w = Writer::new(&mut bytes);
        w.u16(2).unwrap(); w.u16(0).unwrap(); w.u16(4).unwrap(); w.u16(0).unwrap();
        w.u64(api::syscall::service::VFS as u64).unwrap();
        w.reference(reference).unwrap(); w.u64(0).unwrap();
        assert_eq!(validate_grant(&bytes, reference), Ok(()));
        bytes[2] = 1;
        assert_eq!(validate_grant(&bytes, reference), Err(Status::Denied));
        bytes[2] = 0;
        assert_eq!(validate_grant(&bytes, ObjectRefV1 { id: 32, generation: 10 }), Err(Status::Denied));
        bytes[39] = 1;
        assert_eq!(validate_grant(&bytes, reference), Err(Status::Denied));
    }

    const OPERATION: ObjectRefV1 = ObjectRefV1 { id: 40, generation: 3 };
    const RECEIPT: ObjectRefV1 = ObjectRefV1 { id: 5, generation: 1 };

    fn reservation(receipt: ObjectRefV1, state: u32, reconciled: bool) -> Reservation {
        Reservation {
            reference: ObjectRefV1 { id: 1, generation: 1 }, owner: PARENT,
            prepared: ObjectRefV1 { id: 2, generation: 1 },
            child: ChildRefV1 { principal: principal(CHILD), ..ChildRefV1::default() },
            operation: OPERATION, source: ObjectRefV1 { id: 3, generation: PARENT.generation },
            grant: ObjectRefV1 { id: 4, generation: CHILD.generation }, receipt,
            state, charge: RESERVATION_CHARGE, register: None, resolving: None, resolve_action: 0,
            retired: false, reconciled, resolved_abort: None, terminal_ack: false,
        }
    }

    fn outcome() -> Outcome {
        Outcome { request: Vec::new(), result: [0; 64], len: 0, pending: false, observed: false }
    }

    fn log(vfs: &mut VfsManager) -> &mut RequestLog {
        vfs.dirs.prepared.logs.entry(scope(PARENT, OPERATION))
            .or_insert(RequestLog { next: 1, kernel_next: 1, ..RequestLog::default() })
    }

    #[test]
    fn idle_loop_blocks_only_once_every_reservation_waits_on_a_message() {
        let mut vfs = VfsManager::new();
        assert!(!idle_work(&vfs), "no preparations: nothing to poll");
        vfs.dirs.on_contact(PARENT);
        log(&mut vfs);
        assert!(!idle_work(&vfs), "a live owner's settled log needs no polling");

        // Unregistered: the preparation can expire in the kernel without a message.
        vfs.dirs.prepared.reservations.insert(1, reservation(ZERO, 0, false));
        assert!(idle_work(&vfs));
        // Registered but uncommitted: the kernel commits or aborts it on its own.
        vfs.dirs.prepared.reservations.insert(1, reservation(RECEIPT, 0, true));
        assert!(idle_work(&vfs));
        // Committed and running, the child never contacted VFS and no owner is watched:
        // nothing will report the child's exit.
        vfs.dirs.prepared.reservations.insert(1, reservation(RECEIPT, 1, true));
        assert!(idle_work(&vfs));
        // A watched child's exit arrives as an owner-death notification.
        vfs.install_owner_watch(CHILD, 9001, 1);
        assert!(!idle_work(&vfs));
        // The last pass could not prove the child running.
        vfs.dirs.prepared.reservations.get_mut(&1).unwrap().reconciled = false;
        assert!(idle_work(&vfs));
        let value = vfs.dirs.prepared.reservations.get_mut(&1).unwrap();
        value.reconciled = true;
        // An uncertain kernel mutation is settled only by querying it again.
        value.register = Some(KernelCall { id: 1, selector: 0, digest: [0; 32], body: Vec::new() });
        assert!(idle_work(&vfs));
    }

    #[test]
    fn silent_child_of_a_live_owner_lets_the_loop_block_until_the_owner_dies() {
        // An external filter (`top --records | /bin/p03-filter cat`): committed and
        // proven running, but it never sends to VFS, so only its owner is watched.
        let mut vfs = VfsManager::new();
        vfs.dirs.on_contact(PARENT);
        log(&mut vfs);
        vfs.install_owner_watch(PARENT, 9000, 1);
        vfs.dirs.prepared.reservations.insert(1, reservation(RECEIPT, 1, true));
        assert!(!vfs.watches_owner(CHILD));
        assert!(!idle_work(&vfs), "the owner reaches VFS after reaping the child");
        // Not yet proven running: the cadence must observe the commit first.
        vfs.dirs.prepared.reservations.get_mut(&1).unwrap().reconciled = false;
        assert!(idle_work(&vfs));
        vfs.dirs.prepared.reservations.get_mut(&1).unwrap().reconciled = true;
        assert!(!idle_work(&vfs));
        // The owner died: the kernel cancels its Operations, so the child is ending and
        // no message will report it. Poll until its terminal state is observed.
        assert!(vfs.handle_unattributed_owner_death(9000));
        assert!(!vfs.watches_owner(PARENT));
        assert!(idle_work(&vfs), "an orphaned silent child is polled to its end");
    }

    #[test]
    fn retired_and_dead_owner_state_polls_only_while_reconcile_can_progress() {
        let mut vfs = VfsManager::new();
        vfs.dirs.on_contact(PARENT);
        log(&mut vfs).outcomes.insert(1, outcome());
        let mut retired = reservation(RECEIPT, 1, true);
        retired.retired = true;
        vfs.dirs.prepared.reservations.insert(1, retired);
        assert!(!idle_work(&vfs), "reclaim waits for the owner's acknowledgement, a request");
        log(&mut vfs).outcomes.clear();
        assert!(idle_work(&vfs), "acknowledged: reclaim resolves the terminal receipt");
        vfs.dirs.prepared.reservations.clear();

        // Acknowledgements the kernel has not taken yet are retried.
        log(&mut vfs).kernel_next = 3;
        assert!(idle_work(&vfs));
        log(&mut vfs).kernel_acked = 2;
        assert!(!idle_work(&vfs));

        // A dead owner's outcomes and its now-unreferenced log are released.
        log(&mut vfs).outcomes.insert(1, outcome());
        let _ = vfs.dirs.purge_owner(PARENT);
        assert!(!vfs.dirs.has_met(PARENT));
        assert!(idle_work(&vfs));
        log(&mut vfs).outcomes.clear();
        assert!(idle_work(&vfs), "an unreferenced dead log is still removable");
        vfs.install_owner_watch(CHILD, 9001, 1);
        vfs.dirs.prepared.reservations.insert(1, reservation(RECEIPT, 1, true));
        assert!(!idle_work(&vfs), "a committed child outlives its parent's log");
    }
}
