// SPDX-License-Identifier: MPL-2.0
//! Host tests of the real SessionV1 dispatch (`Sessions::request`/`maintain`) for the
//! frontend selectors 23–28 (annex S5-3c §7 Host list). The kernel and IPC are replaced
//! by an in-process model behind the `sys` seam; every request goes through the same
//! header decode, fencing, admission, execution and retained-result path as on target.
use super::*;
use std::cell::RefCell;
use std::vec;
use std::vec::Vec;

/// Kernel/IPC fakes with the exact signatures of the production `sys` re-exports.
pub mod fake {
    use super::MODEL;
    use api::cell_session::{KernelOp, ObjectRefV1, ResultHeaderV1, Status, HEADER_LEN};
    use api::service_binding::ServiceBinding;
    use std::cell::{Cell, RefCell};
    use std::vec::Vec;
    thread_local! {
        pub static SENT: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
        pub static NOW_MS: Cell<u64> = const { Cell::new(1_000) };
        /// Live service registry `(service, tid)` seen by `sys_lookup_service`.
        pub static SERVICES: RefCell<Vec<(u16, usize)>> = const { RefCell::new(Vec::new()) };
        /// The current delivered-request token (`sys_ipc_current`); nonzero lets a poll defer.
        pub static IPC_TOKEN: Cell<isize> = const { Cell::new(0) };
    }
    pub const INPUT_BINDING: ServiceBinding = ServiceBinding { tid: 3, cell_id: 3, generation: 1 };
    pub fn sys_get_time_ms() -> Option<u64> { Some(NOW_MS.with(Cell::get)) }
    pub fn sys_ipc_current() -> isize { IPC_TOKEN.with(Cell::get) }
    pub fn sys_ipc_reply(_token: usize, bytes: &[u8]) -> isize { SENT.with(|s| s.borrow_mut().push(bytes.to_vec())); 0 }
    pub fn sys_try_send(_target: usize, bytes: &[u8]) -> ostd::syscall::SyscallResult {
        SENT.with(|s| s.borrow_mut().push(bytes.to_vec())); ostd::syscall::SyscallResult::Ok(0)
    }
    pub fn sys_lookup_service(service: u16) -> Option<usize> { SERVICES.with(|s| s.borrow().iter().find(|e| e.0 == service).map(|e| e.1)) }
    pub fn sys_lookup_service_bound(_service: u16, _buf: &mut [u8]) -> Option<ServiceBinding> { Some(INPUT_BINDING) }
    pub fn monotonic_ns() -> Result<u64, Status> { Ok(NOW_MS.with(Cell::get) * 1_000_000) }
    pub fn kernel_request(op: KernelOp, request_id: u64, body: &[u8], reply: &mut [u8]) -> Result<ResultHeaderV1, Status> { dispatch(op, request_id, body, reply) }
    pub fn kernel_call(op: KernelOp, _operation: ObjectRefV1, _mutating: bool, body: &[u8], reply: &mut [u8]) -> Result<ResultHeaderV1, Status> { dispatch(op, 1, body, reply) }
    /// Kernel `required_result` (kernel/src/task/cell_session.rs) for the selectors input uses.
    fn required(op: KernelOp, body: &[u8]) -> usize {
        HEADER_LEN + match op as u16 {
            1 | 3 | 4 | 15 | 27 => 16, 2 | 13 | 29 => 32, 10 => 48,
            5 => 8 + body.get(16..20).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).unwrap_or(512).min(512) as usize,
            6 | 11 => 8, 9 => 16, 16 => 72, 17 => 520, 18 => 648, 19 => 48,
            21 => 112, 22 => 168, 23 => 64, 24 => 56, 26 => 80, 30 => 24,
            _ => 0,
        }
    }
    fn dispatch(op: KernelOp, id: u64, body: &[u8], reply: &mut [u8]) -> Result<ResultHeaderV1, Status> {
        // The CellSessionV1 syscall answers a short result buffer header-only, before
        // executing, exactly as `kernel_request` then reports it.
        let required = required(op, body);
        if reply.len() < required {
            let header = ResultHeaderV1 { selector: op as u16, status: Status::BufferTooSmall, total_len: required as u32, request_id: id };
            header.encode(&mut reply[..HEADER_LEN])?;
            return Ok(header);
        }
        let (status, payload) = match MODEL.with(|m| m.borrow_mut().handle(op, body)) { Ok(p) => (Status::Ok, p), Err(s) => (s, Vec::new()) };
        let total = HEADER_LEN + payload.len();
        assert!(total <= reply.len(), "reply buffer too small for {op:?}");
        let header = ResultHeaderV1 { selector: op as u16, status, total_len: total as u32, request_id: id };
        header.encode(&mut reply[..HEADER_LEN])?; reply[HEADER_LEN..total].copy_from_slice(&payload);
        Ok(header)
    }
}

thread_local! { static MODEL: RefCell<Model> = RefCell::new(Model::default()); }
fn model<T>(f: impl FnOnce(&mut Model) -> T) -> T { MODEL.with(|m| f(&mut m.borrow_mut())) }

struct Boot { reference: Ref, controller: Principal, operation: Ref, state: u32 }
struct Prep { child: ChildRefV1, operation: Ref, preparer: Principal, owner: Principal, frontend: Ref, committed: bool, aborted: bool, exited: bool }
struct Receipt { reference: Ref, child: ChildRefV1, frontend: Ref, state: u32, writers: [Ref; 2] }
/// The slice of kernel CellSessionV1 behavior input relies on, with kind-6 receipts.
#[derive(Default)]
pub struct Model {
    next: u64, operations: Vec<(Ref, u32)>, boots: Vec<Boot>, prepared: Vec<Prep>, receipts: Vec<Receipt>,
    writes: Vec<(Ref, Vec<u8>)>, closed: Vec<Ref>, acknowledged: Vec<Ref>,
    /// Endpoints whose ring is gone: WriteStream answers Revoked.
    broken: Vec<Ref>,
}
fn endpoint(boot: Ref, n: u64) -> Ref { Ref { id: boot.id * 10 + n, generation: 1 } }
/// The stdio endpoints (slots 0..=3) and input writers (4, 5) a shell creates for a foreground child.
fn child_endpoint(child: ChildRefV1, n: u64) -> Ref { Ref { id: child.root_tid * 100 + n, generation: 1 } }
impl Model {
    fn handle(&mut self, op: KernelOp, body: &[u8]) -> Result<Vec<u8>, Status> {
        let mut r = Reader::new(body); let mut out = vec![0u8; 512]; let mut w = Writer::new(&mut out);
        match op {
            KernelOp::QueryOperation => {
                let operation = r.reference()?; r.finish()?;
                let state = self.operations.iter().find(|o| o.0 == operation).ok_or(Status::Stale)?.1;
                w.u32(state)?; w.u32(0)?; w.u32(0)?; w.u32(0)?;
            }
            KernelOp::ResolveBootstrap => {
                let reference = r.reference()?; let action = r.u32()?; r.u32()?; let session = r.reference()?; r.finish()?;
                let b = self.boots.iter_mut().find(|b| b.reference == reference).ok_or(Status::Stale)?;
                match action { 1 if b.state == 0 => b.state = 1, 2 => b.state = 3, 3 if b.state == 1 && session != ZERO => b.state = 2, 0 => {}, _ => return Err(Status::Denied) }
                w.u32(b.state)?; w.u32(0)?; w.principal(b.controller)?; w.reference(b.operation)?;
                for n in 0..8 { w.reference(endpoint(b.reference, n))?; }
            }
            KernelOp::InspectPreparedGrant => {
                let prepared = r.reference()?; let kind = r.u16()?; let slot = r.u16()?; r.u32()?; r.u32()?; r.u32()?; r.finish()?;
                let p = self.prepared.iter().find(|p| p.child.prepared_ref() == prepared && !p.committed && !p.aborted).ok_or(Status::Stale)?;
                if (kind, slot) == (1, 0) {
                    // A shell-prepared foreground child: its completed stdio projection.
                    w.child(p.child)?; w.reference(p.operation)?; w.bytes(&[7; 32])?;
                    w.u16(1)?; w.u16(1)?; w.u16(0)?; w.u16(0)?; w.u64(0)?; w.reference(child_endpoint(p.child, 0))?; w.u64(0)?;
                    w.u32(160)?; w.u32(0)?; w.u32(160)?; w.u32(0)?;
                    for slot in 0..4u16 { w.u16(1)?; w.u16(if slot == 0 || slot == 3 { 1 } else { 2 })?; w.u16(slot)?; w.u16(0)?; w.u64(0)?; w.reference(child_endpoint(p.child, slot as u64))?; w.u64(0)?; }
                    let len = w.position(); out.truncate(len); return Ok(out);
                }
                if (kind, slot) != (6, 8) || p.frontend == ZERO { return Err(Status::Denied); }
                w.child(p.child)?; w.reference(p.operation)?; w.bytes(&[7; 32])?;
                w.u16(6)?; w.u16(0)?; w.u16(8)?; w.u16(0)?; w.u64(api::syscall::service::INPUT as u64)?; w.reference(p.frontend)?; w.u64(0)?;
                w.u32(32)?; w.u32(0)?; w.u32(32)?; w.u32(0)?; w.principal(p.preparer)?; w.principal(p.owner)?;
            }
            KernelOp::RegisterBindReceipt => {
                let prepared = r.reference()?; let kind = r.u16()?; let slot = r.u16()?; r.u32()?; let object = r.reference()?;
                let (e, n) = (r.u64()?, r.u64()?); let digest = r.take(32)?; let seal_len = r.u32()?; r.u32()?;
                let seal_bytes = r.take(seal_len as usize)?; r.finish()?;
                let p = self.prepared.iter().find(|p| p.child.prepared_ref() == prepared).ok_or(Status::Stale)?;
                let child = p.child;
                let writers = if kind == 1 {
                    // Foreground seal: session, lease, operation, child, expected, next, 4 endpoints, 2 writers.
                    let mut seal = Reader::new(seal_bytes); seal.reference()?; let lease = seal.reference()?; seal.reference()?; let sealed = seal.child()?;
                    let (expected, next) = (seal.u64()?, seal.u64()?); for _ in 0..4 { seal.reference()?; } let writers = [seal.reference()?, seal.reference()?]; seal.finish()?;
                    if (slot, seal_len) != (0, 192) || digest != [7; 32] || lease != object || sealed != child || (expected, next) != (e, n) { return Err(Status::Denied); }
                    writers
                } else {
                    let mut seal = Reader::new(seal_bytes); let sealed = seal.reference()?; let sealed_child = seal.child()?; seal.finish()?;
                    if (kind, slot, e, n, seal_len) != (6, 8, 0, 0, 48) || digest != [7; 32] || sealed != object || object != p.frontend || sealed_child != child { return Err(Status::Denied); }
                    if self.receipts.iter().any(|x| x.frontend == object && x.state <= 1) { return Err(Status::Denied); }
                    [ZERO; 2]
                };
                self.next += 1; let reference = Ref { id: 90_000 + self.next, generation: 1 };
                self.receipts.push(Receipt { reference, child, frontend: if kind == 1 { ZERO } else { object }, state: 0, writers });
                w.reference(reference)?;
            }
            KernelOp::ResolveBindReceipt => {
                let reference = r.reference()?; let action = r.u32()?; r.u32()?; r.finish()?;
                let x = self.receipts.iter_mut().find(|x| x.reference == reference).ok_or(Status::Stale)?;
                if action == 1 && x.state == 0 { x.state = 2; for p in self.prepared.iter_mut().filter(|p| p.child == x.child) { p.aborted = true; } }
                if action == 2 { self.acknowledged.push(reference); }
                w.u32(x.state)?; w.u32(0)?; w.child(x.child)?; w.reference(x.writers[0])?; w.reference(x.writers[1])?;
            }
            KernelOp::QueryReceiptChild => {
                let reference = r.reference()?; r.finish()?;
                let x = self.receipts.iter().find(|x| x.reference == reference).ok_or(Status::Stale)?;
                let exited = self.prepared.iter().any(|p| p.child == x.child && p.exited);
                w.child(x.child)?; w.u32(if exited { 3 } else { 1 })?; w.u32(0)?; w.i32(0)?; w.u32(0)?; w.u64(u64::from(exited))?;
            }
            KernelOp::RestoreSessionInput => {
                let reference = r.reference()?; let terminal = r.u64()?; r.u64()?; r.finish()?;
                if terminal == 0 || !self.receipts.iter().any(|x| x.reference == reference) { return Err(Status::InvalidChild); }
                w.reference(endpoint(BOOT, 0))?; w.reference(endpoint(BOOT, 3))?;
            }
            KernelOp::WriteStream => {
                let target = r.reference()?; let count = r.u32()? as usize; r.u32()?; r.u64()?; let bytes = r.take(count)?; r.finish()?;
                if self.broken.contains(&target) { return Err(Status::Revoked); }
                self.writes.push((target, bytes.to_vec())); w.u32(count as u32)?; w.u32(0)?;
            }
            KernelOp::CloseEndpoint => { let target = r.reference()?; r.finish()?; self.closed.push(target); }
            _ => return Err(Status::Unsupported),
        }
        let len = w.position(); out.truncate(len); Ok(out)
    }
    /// What `CommitChild` does to a bound kind-6 receipt.
    fn commit(&mut self, child: ChildRefV1) {
        for p in self.prepared.iter_mut().filter(|p| p.child == child) { p.committed = true; }
        for x in self.receipts.iter_mut().filter(|x| x.child == child && x.state == 0) { x.state = 1; }
    }
    /// The committed child reached its terminal state.
    fn exit(&mut self, child: ChildRefV1) { for p in self.prepared.iter_mut().filter(|p| p.child == child) { p.exited = true; } }
    fn set_operation(&mut self, operation: Ref, state: u32) {
        if let Some(o) = self.operations.iter_mut().find(|o| o.0 == operation) { o.1 = state; } else { self.operations.push((operation, state)); }
    }
}

const TID: usize = 41;
const HOLDER: Principal = Principal { cell_id: 10, generation: 1 };
const OTHER: Principal = Principal { cell_id: 11, generation: 1 };
const SHELL: Principal = Principal { cell_id: 20, generation: 1 };
const H_OP: Ref = Ref { id: 1000, generation: 1 };
const O_OP: Ref = Ref { id: 1100, generation: 1 };
const S_OP: Ref = Ref { id: 2000, generation: 1 };
const BOOT: Ref = Ref { id: 3000, generation: 1 };
const CHILD: ChildRefV1 = ChildRefV1 { principal: SHELL, root_tid: 77, publication_generation: 5 };

struct Harness { sessions: Sessions }
struct Attached { frontend: Ref, session: Ref, attachment: Ref, resume: Ref, receipt: Ref }
impl Harness {
    fn new() -> Self {
        model(|m| {
            *m = Model::default();
            for op in [H_OP, O_OP, S_OP] { m.set_operation(op, 0); }
            m.boots.push(Boot { reference: BOOT, controller: SHELL, operation: S_OP, state: 0 });
            m.prepared.push(Prep { child: CHILD, operation: H_OP, preparer: HOLDER, owner: HOLDER, frontend: ZERO, committed: false, aborted: false, exited: false });
        });
        fake::SERVICES.with(|s| s.borrow_mut().clear());
        Self { sessions: Sessions::new() }
    }
    /// One SessionV1 record on `lane` (the header operation the SDK stamps), through the real dispatch.
    fn call(&mut self, p: Principal, lane: Ref, selector: u16, body: &[u8]) -> (Status, Vec<u8>) { self.call_as(p, TID, lane, selector, body) }
    /// [`Self::call`] from the attested sender task `tid`.
    fn call_as(&mut self, p: Principal, tid: usize, lane: Ref, selector: u16, body: &[u8]) -> (Status, Vec<u8>) {
        // The next ID of the service's fence for this lane: rejections before fencing
        // (unknown selector refs, admission) consume no ID, exactly as on target.
        let id = self.sessions.fences.iter().find(|f| f.caller == p && f.operation == lane).map_or(1, |f| f.next);
        let mut record = vec![0u8; HEADER_LEN + body.len()];
        RequestHeaderV1 { selector, total_len: record.len() as u32, request_id: id }.encode(&mut record[..HEADER_LEN]).unwrap();
        record[HEADER_LEN..].copy_from_slice(body);
        fake::SENT.with(|s| s.borrow_mut().clear());
        let identity = CallerIdentity { flags: 0, cell_id: p.cell_id, generation: p.generation, sender_tid: tid as u64 };
        self.sessions.request(&record, Some(identity), tid);
        let envelope = fake::SENT.with(|s| s.borrow_mut().pop()).expect("a reply");
        let api::ipc::InputResponse::SessionV1(result) = api::ipc::decode(&envelope).unwrap() else { panic!("wrong response") };
        let header = ResultHeaderV1::decode(result).unwrap();
        assert_eq!((header.selector, header.request_id), (selector, id));
        (header.status, result[HEADER_LEN..].to_vec())
    }
    fn ok(&mut self, p: Principal, lane: Ref, selector: u16, body: &[u8]) -> Vec<u8> {
        let (status, body) = self.call(p, lane, selector, body); assert_eq!(status, Status::Ok, "selector {selector}"); body
    }
    fn status(&mut self, p: Principal, lane: Ref, selector: u16, body: &[u8]) -> Status { self.call(p, lane, selector, body).0 }
    fn register(&mut self, p: Principal, lane: Ref) -> Ref {
        let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: lane, kind: fe::FRONTEND_KIND_GUI, rows: 30, cols: 100 }.encode(&mut body).unwrap();
        Ref::from_bytes(&self.ok(p, lane, 23, &body)).unwrap()
    }
    fn bind(&mut self, frontend: Ref, child: ChildRefV1) -> (Status, Vec<u8>) {
        let mut body = [0; 48]; fe::BindFrontendChildV1 { frontend, child }.encode(&mut body).unwrap();
        self.call(HOLDER, H_OP, 24, &body)
    }
    fn create(&mut self, p: Principal, frontend: Ref) -> (Status, Vec<u8>) {
        let mut body = [0; 56]; fe::CreateForFrontendV1 { operation: S_OP, bootstrap: BOOT, frontend, rows: 0, cols: 0 }.encode(&mut body).unwrap();
        self.call(p, S_OP, 25, &body)
    }
    fn query(&mut self, frontend: Ref) -> fe::FrontendStatusV1 { fe::FrontendStatusV1::decode(&self.ok(HOLDER, H_OP, 26, &frontend.to_bytes())).unwrap() }
    /// Register → prepare (model) → bind → commit (model) → CreateForFrontend → QueryFrontend.
    fn attach(&mut self) -> Attached {
        let frontend = self.register(HOLDER, H_OP);
        assert_eq!(self.query(frontend).state, fe::FRONTEND_STATE_PENDING);
        model(|m| m.prepared[0].frontend = frontend);
        let (status, receipt) = self.bind(frontend, CHILD); assert_eq!(status, Status::Ok);
        let receipt = Ref::from_bytes(&receipt).unwrap();
        assert_eq!(self.query(frontend).state, fe::FRONTEND_STATE_BOUND);
        model(|m| m.commit(CHILD));
        let (status, result) = self.create(SHELL, frontend); assert_eq!(status, Status::Ok);
        let result = fe::CreateForFrontendResultV1::decode(&result).unwrap();
        assert_eq!((result.controller, result.epoch, result.mode), (SHELL, 1, 2));
        assert_eq!(result.endpoints, [endpoint(BOOT, 0), endpoint(BOOT, 1), endpoint(BOOT, 2), endpoint(BOOT, 3)]);
        let status = self.query(frontend);
        assert_eq!((status.state, status.session, status.attachment), (fe::FRONTEND_STATE_ATTACHED, result.session, result.attachment));
        assert_ne!(status.resume, ZERO);
        Attached { frontend, session: result.session, attachment: result.attachment, resume: status.resume, receipt }
    }
    fn session(&self, reference: Ref) -> &Session { self.sessions.sessions.iter().find(|s| s.reference == reference).unwrap() }
    /// Lost reasons queued for the reader. Every queued control frame must also pass the
    /// SDK's event validator, or the shell's `Event::decode` would refuse it.
    fn lost(&self, reference: Ref) -> Vec<u32> {
        let controls = &self.session(reference).controls;
        for e in controls { assert_eq!(api::services::session::validate_event(u16::from_le_bytes([e.bytes[16], e.bytes[17]]), &e.bytes[24..]), Ok(()), "control frame {:?}", &e.bytes[16..]); }
        controls.iter().filter(|e| e.bytes[16..18] == 8u16.to_le_bytes()).map(|e| u32::from_le_bytes(e.bytes[24..28].try_into().unwrap())).collect()
    }
    fn submit(&mut self, p: Principal, lane: Ref, session: Ref, attachment: Ref, epoch: u64) -> Status {
        let mut body = Vec::new();
        body.extend_from_slice(&session.to_bytes()); body.extend_from_slice(&attachment.to_bytes()); body.extend_from_slice(&epoch.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes()); body.extend_from_slice(&0u16.to_le_bytes()); body.extend_from_slice(&25u32.to_le_bytes());
        let mut frame = [0u8; 25]; frame[16..18].copy_from_slice(&2u16.to_le_bytes()); frame[18..20].copy_from_slice(&1u16.to_le_bytes()); frame[24] = b'a';
        body.extend_from_slice(&frame);
        self.status(p, lane, 13, &body)
    }
    fn release(&mut self, frontend: Ref) -> Status { self.status(HOLDER, H_OP, 27, &frontend.to_bytes()) }
    fn reattach(&mut self, p: Principal, lane: Ref, frontend: Ref, resume: Ref) -> (Status, Vec<u8>) {
        let mut body = [0; 32]; fe::ReattachFrontendV1 { frontend, resume }.encode(&mut body).unwrap();
        self.call(p, lane, 28, &body)
    }
}

#[test]
fn register_bind_create_attaches_the_shell_as_controller_of_a_gui_session() {
    let mut h = Harness::new();
    let a = h.attach();
    let s = h.session(a.session);
    assert_eq!((s.controller, s.reader, s.kind, s.rows, s.cols), (SHELL, SHELL, fe::FRONTEND_KIND_GUI, 30, 100));
    assert_eq!(s.attachment.as_ref().map(|x| (x.reference, x.frontend)), Some((a.attachment, HOLDER)));
    // QueryCapabilities (controller) reports the GUI kind and the frontend's registered size.
    let caps = h.ok(SHELL, S_OP, 12, &a.session.to_bytes());
    assert_eq!((caps[0], u16::from_le_bytes([caps[2], caps[3]]), u16::from_le_bytes([caps[4], caps[5]])), (1, 30, 100));
    // The holder's requests on the session are fenced on its registered operation.
    assert_eq!(h.sessions.operation_for(13, &a.session.to_bytes(), HOLDER), Ok(H_OP));
    assert_eq!(h.sessions.operation_for(13, &a.session.to_bytes(), SHELL), Ok(S_OP));
    assert_ne!(a.receipt, ZERO);
    // QueryFrontend is a nonmutating read: no retained outcome, no fence advance.
    let next = |h: &Harness| h.sessions.fences.iter().find(|f| f.caller == HOLDER && f.operation == H_OP).map(|f| f.next);
    let before = next(&h);
    assert_eq!(h.query(a.frontend).state, fe::FRONTEND_STATE_ATTACHED);
    assert_eq!(next(&h), before);
}

#[test]
fn wrong_holder_and_wrong_child_are_refused() {
    let mut h = Harness::new();
    let frontend = h.register(HOLDER, H_OP);
    model(|m| m.prepared[0].frontend = frontend);
    // Another Cell cannot bind, query or release the holder's frontend.
    let mut body = [0; 48]; fe::BindFrontendChildV1 { frontend, child: CHILD }.encode(&mut body).unwrap();
    assert_eq!(h.status(OTHER, H_OP, 24, &body), Status::Denied);
    assert_eq!(h.status(OTHER, H_OP, 26, &frontend.to_bytes()), Status::Denied);
    assert_eq!(h.status(OTHER, H_OP, 27, &frontend.to_bytes()), Status::Denied);
    // A child prepared by someone else, or under an operation the holder does not own.
    model(|m| m.prepared[0].preparer = OTHER);
    assert_eq!(h.bind(frontend, CHILD).0, Status::Denied);
    model(|m| { m.prepared[0].preparer = HOLDER; m.prepared[0].owner = OTHER; });
    assert_eq!(h.bind(frontend, CHILD).0, Status::Denied);
    model(|m| { m.prepared[0].owner = HOLDER; m.prepared[0].operation = O_OP; });
    assert_eq!(h.bind(frontend, CHILD).0, Status::Denied);
    // A context naming a different frontend, or a different child than the one named.
    model(|m| { m.prepared[0].operation = H_OP; m.prepared[0].frontend = Ref { id: 4242, generation: 1 }; });
    assert_eq!(h.bind(frontend, CHILD).0, Status::InvalidChild);
    model(|m| m.prepared[0].frontend = frontend);
    let impostor = ChildRefV1 { principal: OTHER, ..CHILD };
    assert_eq!(h.bind(frontend, impostor).0, Status::InvalidChild);
    assert_eq!(h.query(frontend).state, fe::FRONTEND_STATE_PENDING);
    // The right child binds; only that exact committed child may create the session.
    assert_eq!(h.bind(frontend, CHILD).0, Status::Ok);
    assert_eq!(h.create(SHELL, frontend).0, Status::InvalidChild, "bound but not yet committed");
    model(|m| m.commit(CHILD));
    assert_eq!(h.create(OTHER, frontend).0, Status::Denied);
    assert_eq!(h.create(Principal { generation: 2, ..SHELL }, frontend).0, Status::Denied);
    assert_eq!(h.create(SHELL, frontend).0, Status::Ok);
}

#[test]
fn second_bind_and_second_create_are_refused() {
    let mut h = Harness::new();
    let a = h.attach();
    assert_eq!(h.bind(a.frontend, CHILD).0, Status::Denied);
    assert_eq!(h.create(SHELL, a.frontend).0, Status::Denied);
    assert_eq!(h.sessions.sessions.iter().filter(|s| !s.closed).count(), 1);
}

#[test]
fn ssh_kind_stays_unsupported_and_create_admits_no_gui_kind() {
    let mut h = Harness::new();
    for (kind, expected) in [(fe::FRONTEND_KIND_SSH, Status::Unsupported), (fe::FRONTEND_KIND_SERIAL, Status::Unsupported), (3, Status::Malformed)] {
        let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: H_OP, kind, rows: 24, cols: 80 }.encode(&mut body).unwrap();
        assert_eq!(h.status(HOLDER, H_OP, 23, &body), expected, "kind {kind}");
    }
    let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: H_OP, kind: 1, rows: 0, cols: 80 }.encode(&mut body).unwrap();
    assert_eq!(h.status(HOLDER, H_OP, 23, &body), Status::Malformed);
    body[17] = 1;
    assert_eq!(h.status(HOLDER, H_OP, 23, &body), Status::InvalidFlags);
    // Create (1) itself still refuses Gui: it is reachable only through sel 25.
    let mut create = [0; 40]; let mut w = Writer::new(&mut create);
    w.reference(S_OP).unwrap(); w.reference(BOOT).unwrap(); w.u8(1).unwrap(); w.u8(0).unwrap(); w.u16(24).unwrap(); w.u16(80).unwrap(); w.u16(0).unwrap();
    assert_eq!(h.status(SHELL, S_OP, 1, &create), Status::Unsupported);
    // Selectors past 31 (the trust selectors) stay unknown.
    assert_eq!(h.status(HOLDER, H_OP, 32, &H_OP.to_bytes()), Status::UnknownOperation);
    assert!(h.sessions.frontends.is_empty());
}

#[test]
fn frontends_are_limited_to_eight_per_holder_and_sixty_four_system_wide() {
    let mut h = Harness::new();
    for _ in 0..8 { h.register(HOLDER, H_OP); }
    let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: H_OP, kind: 1, rows: 24, cols: 80 }.encode(&mut body).unwrap();
    assert_eq!(h.status(HOLDER, H_OP, 23, &body), Status::QuotaExceeded);
    for n in 0..7u64 {
        let p = Principal { cell_id: 100 + n, generation: 1 }; let lane = Ref { id: 5000 + n, generation: 1 };
        model(|m| m.set_operation(lane, 0));
        for _ in 0..8 { h.register(p, lane); }
    }
    assert_eq!(h.sessions.frontends.len(), 64);
    let lane = Ref { id: 6000, generation: 1 }; model(|m| m.set_operation(lane, 0));
    let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: lane, kind: 1, rows: 24, cols: 80 }.encode(&mut body).unwrap();
    assert_eq!(h.status(Principal { cell_id: 200, generation: 1 }, lane, 23, &body), Status::QuotaExceeded);
    // A cancelled holder operation cannot register at all.
    let dead = Ref { id: 6001, generation: 1 }; model(|m| m.set_operation(dead, 1));
    let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: dead, kind: 1, rows: 24, cols: 80 }.encode(&mut body).unwrap();
    assert_eq!(h.status(Principal { cell_id: 201, generation: 1 }, dead, 23, &body), Status::Cancelled);
}

#[test]
fn release_detaches_with_lost_four_and_keeps_the_controller_session() {
    let mut h = Harness::new();
    let a = h.attach();
    assert_eq!(h.submit(HOLDER, H_OP, a.session, a.attachment, 0), Status::Ok);
    assert_eq!(h.release(a.frontend), Status::Ok);
    assert_eq!(h.lost(a.session), vec![fe::LOST_FRONTEND]);
    let s = h.session(a.session);
    assert!(!s.closed && s.detached && s.attachment.is_none() && !s.draining && !s.eof && s.resume == a.resume);
    assert_eq!(s.typed.len(), 1, "queued input still belongs to the reader");
    // The reader keeps its lease while detached; no EOF is ever sent while Detached.
    assert!(h.sessions.reader(h.sessions.index(a.session).unwrap(), SHELL, s.lease).is_ok());
    h.sessions.maintain(); h.sessions.maintain();
    assert!(model(|m| m.closed.is_empty()));
    assert!(model(|m| m.writes.iter().any(|(target, bytes)| *target == endpoint(BOOT, 7) && bytes[16..18] == 8u16.to_le_bytes() && bytes[24..28] == 4u32.to_le_bytes())));
    // The released frontend is gone and its attachment is stale for its holder.
    assert_eq!(h.status(HOLDER, H_OP, 26, &a.frontend.to_bytes()), Status::Stale);
    assert_eq!(h.submit(HOLDER, H_OP, a.session, a.attachment, 0), Status::Stale);
    assert!(model(|m| m.acknowledged.contains(&a.receipt)));
    // A detached sel-25 session cannot be self-attached by its controller either.
    let mut body = Vec::new(); body.extend_from_slice(&a.session.to_bytes()); body.extend_from_slice(&SHELL.to_bytes());
    for r in [S_OP, endpoint(BOOT, 0), endpoint(BOOT, 4)] { body.extend_from_slice(&r.to_bytes()); }
    assert_eq!(h.status(SHELL, S_OP, 2, &body), Status::Denied);
}

#[test]
fn reattach_binds_a_new_frontend_resizes_notifies_and_stales_the_old_attachment() {
    let mut h = Harness::new();
    let a = h.attach();
    assert_eq!(h.release(a.frontend), Status::Ok);
    let mut body = [0; 24]; fe::RegisterFrontendV1 { operation: H_OP, kind: 1, rows: 40, cols: 120 }.encode(&mut body).unwrap();
    let fresh = Ref::from_bytes(&h.ok(HOLDER, H_OP, 23, &body)).unwrap();
    // Wrong holder: another Cell with its own pending frontend presenting the resume bytes.
    let foreign = h.register(OTHER, O_OP);
    assert_eq!(h.reattach(OTHER, O_OP, foreign, a.resume).0, Status::WrongOwner);
    // Unknown resume, or someone else's frontend.
    assert_eq!(h.reattach(HOLDER, H_OP, fresh, Ref { id: 999, generation: 1 }).0, Status::Stale);
    assert_eq!(h.reattach(HOLDER, H_OP, foreign, a.resume).0, Status::Denied);
    let (status, result) = h.reattach(HOLDER, H_OP, fresh, a.resume);
    assert_eq!(status, Status::Ok);
    let result = fe::ReattachResultV1::decode(&result).unwrap();
    assert_eq!(result.session, a.session); assert_ne!(result.attachment, a.attachment);
    // Resize to the new frontend's size precedes the FrontendAttached notice.
    let s = h.session(a.session);
    let kinds: Vec<u16> = s.controls.iter().map(|e| u16::from_le_bytes([e.bytes[16], e.bytes[17]])).collect();
    assert_eq!(kinds, vec![8, 5, 8]);
    assert_eq!(&s.controls[1].bytes[24..28], &[40, 0, 120, 0]);
    assert_eq!(h.lost(a.session), vec![fe::LOST_FRONTEND, fe::FRONTEND_ATTACHED]);
    assert!((s.rows, s.cols) == (40, 120) && !s.detached);
    // The new frontend reports Attached with a reissued resume; the old one is stale.
    let status = h.query(fresh);
    assert_eq!((status.state, status.session, status.attachment), (fe::FRONTEND_STATE_ATTACHED, a.session, result.attachment));
    assert!(status.resume != a.resume && status.resume != ZERO);
    let spare = h.register(HOLDER, H_OP);
    assert_eq!(h.reattach(HOLDER, H_OP, spare, a.resume).0, Status::Stale);
    assert_eq!(h.submit(HOLDER, H_OP, a.session, a.attachment, 0), Status::Stale);
    assert_eq!(h.submit(HOLDER, H_OP, a.session, result.attachment, 0), Status::Ok);
    // At most one live attachment: the current resume cannot attach a second frontend.
    let third = h.register(HOLDER, H_OP);
    assert_eq!(h.reattach(HOLDER, H_OP, third, status.resume).0, Status::WouldBlock);
}

#[test]
fn holder_death_revokes_resume_sends_lost_four_then_eof() {
    let mut h = Harness::new();
    let a = h.attach();
    let pending = h.register(HOLDER, H_OP);
    model(|m| m.set_operation(H_OP, 1));
    h.sessions.maintain();
    let s = h.session(a.session);
    assert!(s.orphaned && s.detached && s.attachment.is_none() && s.resume == ZERO && s.draining && !s.closed);
    assert_eq!(h.lost(a.session), vec![fe::LOST_FRONTEND]);
    assert!(h.sessions.frontends.is_empty(), "attached and pending records die with the holder: {pending:?}");
    // Lost(4) is written to the reader's control ring before EOF closes its stdin producer.
    let writes = model(|m| m.writes.clone());
    assert!(writes.iter().any(|(t, b)| *t == endpoint(BOOT, 7) && b[16..18] == 8u16.to_le_bytes()));
    assert!(model(|m| m.closed.contains(&endpoint(BOOT, 6))));
    h.sessions.maintain();
    assert_eq!(h.session(a.session).controls.back().map(|e| u16::from_le_bytes([e.bytes[16], e.bytes[17]])), Some(7));
    assert!(model(|m| m.writes.iter().any(|(t, b)| *t == endpoint(BOOT, 7) && b[16..18] == 7u16.to_le_bytes())));
}

#[test]
fn holder_death_while_detached_ends_input_without_a_second_lost() {
    let mut h = Harness::new();
    let a = h.attach();
    assert_eq!(h.release(a.frontend), Status::Ok);
    // A removed holder operation (kernel Stale) is proof of death, like a cancelled one.
    model(|m| m.operations.retain(|o| o.0 != H_OP));
    h.sessions.maintain();
    let s = h.session(a.session);
    assert!(s.orphaned && s.resume == ZERO && s.draining);
    assert_eq!(h.lost(a.session), vec![fe::LOST_FRONTEND]);
}

#[test]
fn attachment_principal_rule_applies_only_to_sel_25_sessions() {
    let mut h = Harness::new();
    let a = h.attach();
    // Sel-25: the holder submits with epoch 0 (current epoch substituted); any other epoch is refused.
    assert_eq!(h.submit(HOLDER, H_OP, a.session, a.attachment, 0), Status::Ok);
    assert_eq!(h.submit(HOLDER, H_OP, a.session, a.attachment, 1), Status::WrongEpoch);
    // The controller is not the frontend of its sel-25 session.
    assert_eq!(h.submit(SHELL, S_OP, a.session, a.attachment, 0), Status::Denied);
    // Kind 0: a self-attached serial session (sel 1 + sel 2) keeps frontend == controller and exact epochs.
    const SERIAL: Principal = Principal { cell_id: 30, generation: 1 };
    let (op, boot) = (Ref { id: 7000, generation: 1 }, Ref { id: 7100, generation: 1 });
    model(|m| { m.set_operation(op, 0); m.boots.push(Boot { reference: boot, controller: SERIAL, operation: op, state: 0 }); });
    let mut create = [0; 40]; let mut w = Writer::new(&mut create);
    w.reference(op).unwrap(); w.reference(boot).unwrap(); w.u8(0).unwrap(); w.u8(0).unwrap(); w.u16(24).unwrap(); w.u16(80).unwrap(); w.u16(0).unwrap();
    let session = Ref::from_bytes(&h.ok(SERIAL, op, 1, &create)[..16]).unwrap();
    let mut body = Vec::new(); body.extend_from_slice(&session.to_bytes()); body.extend_from_slice(&SERIAL.to_bytes());
    for r in [op, endpoint(boot, 0), endpoint(boot, 4)] { body.extend_from_slice(&r.to_bytes()); }
    let attachment = Ref::from_bytes(&h.ok(SERIAL, op, 2, &body)).unwrap();
    assert_eq!(h.submit(SERIAL, op, session, attachment, 0), Status::WrongEpoch);
    assert_eq!(h.submit(SERIAL, op, session, attachment, 1), Status::Ok);
    // A non-controller cannot use a kind-0 attachment, and cannot be named as its frontend.
    assert_eq!(h.submit(HOLDER, H_OP, session, attachment, 1), Status::Denied);
    let caps = h.ok(SERIAL, op, 12, &session.to_bytes());
    assert_eq!(caps[0], fe::FRONTEND_KIND_SERIAL);
}

#[test]
fn holder_detach_marks_the_frontend_lost_and_controller_death_closes_it() {
    let mut h = Harness::new();
    let a = h.attach();
    let mut body = Vec::new(); body.extend_from_slice(&a.session.to_bytes()); body.extend_from_slice(&a.attachment.to_bytes());
    assert_eq!(h.status(OTHER, O_OP, 10, &body), Status::Denied);
    assert_eq!(h.status(HOLDER, H_OP, 10, &body), Status::Ok);
    assert_eq!(h.lost(a.session), vec![fe::LOST_FRONTEND]);
    assert!(!h.session(a.session).closed);
    assert_eq!(h.query(a.frontend).state, fe::FRONTEND_STATE_LOST);
    // Reattach on a fresh frontend, then the controller dies: the frontend reports Closed.
    let fresh = h.register(HOLDER, H_OP);
    assert_eq!(h.reattach(HOLDER, H_OP, fresh, a.resume).0, Status::Ok);
    model(|m| m.boots[0].state = 3);
    h.sessions.maintain();
    assert!(h.sessions.index(a.session).is_err());
    let status = h.query(fresh);
    assert_eq!((status.state, status.session, status.resume), (fe::FRONTEND_STATE_CLOSED, ZERO, ZERO));
    assert_eq!(h.release(fresh), Status::Ok);
    assert_eq!(h.release(a.frontend), Status::Ok);
    assert!(h.sessions.frontends.is_empty());
}

#[test]
fn unbound_frontends_expire_with_the_holder_operation_and_release_revokes_an_unused_receipt() {
    let mut h = Harness::new();
    let frontend = h.register(HOLDER, H_OP);
    model(|m| m.prepared[0].frontend = frontend);
    let receipt = Ref::from_bytes(&h.bind(frontend, CHILD).1).unwrap();
    assert_eq!(h.release(frontend), Status::Ok);
    assert!(model(|m| m.prepared[0].aborted && m.receipts.iter().any(|x| x.reference == receipt && x.state == 2)));
    let pending = h.register(HOLDER, H_OP);
    assert_eq!(h.sessions.frontends.len(), 1);
    model(|m| m.set_operation(H_OP, 1));
    h.sessions.maintain();
    assert!(h.sessions.frontends.iter().all(|f| f.reference != pending));
}

#[path = "session_trust_tests.rs"]
mod trust_tests;
