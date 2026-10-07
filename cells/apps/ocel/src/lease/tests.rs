// SPDX-License-Identifier: MIT
extern crate std;
use super::*;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use dom_arena::{OcelJsCommand, OcelJsReply, OcelJsRequest, OcelJsResponse};

#[derive(Default)]
pub(crate) struct State {
    pub requests: Vec<Request>,
    pub commands: Vec<OcelJsCommand>,
    pub opens: usize,
    pub failure: Option<Failure>,
    pub missing_broker: bool,
    pub stale_provider: bool,
    pub stale_broker: bool,
    pub fail_sync: bool,
    pub wrong_engine: bool,
    pub release_bad_ack: bool,
    incoming: Vec<u8>,
}
std::thread_local! {
    static STATE: RefCell<Rc<RefCell<State>>> = RefCell::new(Rc::new(RefCell::new(State::default())));
}
pub(crate) fn fresh_state() -> Rc<RefCell<State>> {
    let state = Rc::new(RefCell::new(State::default()));
    STATE.with(|slot| *slot.borrow_mut() = state.clone());
    state
}
#[derive(Clone)]
pub(crate) struct MockTransport { state: Rc<RefCell<State>> }
impl Default for MockTransport {
    fn default() -> Self { STATE.with(|slot| Self { state: slot.borrow().clone() }) }
}
impl Transport for MockTransport {
    fn lookup(&mut self, service: u16) -> Option<usize> {
        let state = self.state.borrow();
        if service == api::syscall::service::OCEL_ACTIVATOR {
            if state.missing_broker { None } else { Some(if state.stale_broker { 99 } else { 10 }) }
        } else { Some(if state.stale_provider { 99 } else { 20 }) }
    }
    fn call(&mut self, peer: usize, request: &[u8], reply: &mut [u8], ticks: u64) -> Result<usize, String> {
        assert!(ticks > 0);
        let mut state = self.state.borrow_mut();
        let bytes = if peer == 10 {
            let request = Request::decode(request).unwrap();
            state.requests.push(request);
            let engine = request.engine();
            match request {
                Request::Acquire { .. } => {
                    if let Some(failure) = state.failure { Response::Error { engine, failure }.encode().to_vec() }
                    else { Response::Ready { engine: if state.wrong_engine { Engine::Pdf } else { engine }, lease: 7, tid: 20 }.encode().to_vec() }
                }
                Request::Release { lease, .. } => {
                    assert_eq!(lease, 7);
                    if state.release_bad_ack { Response::Error { engine, failure: Failure::Failed }.encode().to_vec() }
                    else { Response::Released { engine }.encode().to_vec() }
                }
            }
        } else {
            assert_eq!(peer, 20);
            let request: OcelJsRequest = postcard::from_bytes(request).unwrap();
            let response = match request {
                OcelJsRequest::OpenContext { .. } => { state.opens += 1; OcelJsReply::Ack }
                OcelJsRequest::Begin { .. } => { state.incoming.clear(); OcelJsReply::Ack }
                OcelJsRequest::Chunk { offset, data, .. } => {
                    assert_eq!(offset, state.incoming.len());
                    state.incoming.extend(data);
                    OcelJsReply::Ack
                }
                OcelJsRequest::Commit { .. } => {
                    let command: OcelJsCommand = postcard::from_bytes(&state.incoming).unwrap();
                    let fail = state.fail_sync && matches!(command, OcelJsCommand::SyncDocument { .. });
                    state.commands.push(command);
                    if fail { OcelJsReply::Error { message: String::from("sync rejected") } }
                    else {
                        let data = postcard::to_allocvec(&OcelJsResponse::Success { mutations: Vec::new(), result_repr: String::new() }).unwrap();
                        OcelJsReply::ResponseChunk { offset: 0, total: data.len(), data }
                    }
                }
                OcelJsRequest::Ping => OcelJsReply::Ready,
                OcelJsRequest::ReadResponse { .. } => panic!("small test response must fit one chunk"),
            };
            postcard::to_allocvec(&response).unwrap()
        };
        reply[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

#[test]
fn acquired_lease_releases_exact_token_once() {
    let state = fresh_state();
    let mut lease = Lease::acquire_with(Engine::Pdf, MockTransport::default()).unwrap();
    lease.release().unwrap();
    drop(lease);
    assert_eq!(state.borrow().requests, alloc::vec![Request::Acquire { engine: Engine::Pdf }, Request::Release { engine: Engine::Pdf, lease: 7 }]);
}
#[test]
fn drop_hands_back_lease_and_invalid_registration_cleans_up() {
    let state = fresh_state();
    drop(Lease::acquire_with(Engine::JavaScript, MockTransport::default()).unwrap());
    assert_eq!(state.borrow().requests.len(), 2);
    let state = fresh_state();
    state.borrow_mut().stale_provider = true;
    assert!(Lease::acquire_with(Engine::Pdf, MockTransport::default()).is_err());
    assert_eq!(state.borrow().requests.len(), 2);
    assert!(matches!(state.borrow().requests[1], Request::Release { .. }));
}
#[test]
fn cached_provider_is_checked_before_every_operation() {
    let state = fresh_state();
    let mut lease = Lease::acquire_with(Engine::JavaScript, MockTransport::default()).unwrap();
    state.borrow_mut().stale_provider = true;
    assert!(lease.exchange(&[0], &mut [0u8; 32]).is_err());
    assert_eq!(state.borrow().opens, 0);
    state.borrow_mut().stale_provider = false;
    state.borrow_mut().stale_broker = true;
    assert!(lease.exchange(&[0], &mut [0u8; 32]).is_err());
}
#[test]
fn optional_engine_failures_and_wrong_engine_are_explicit() {
    for failure in [Failure::Unavailable, Failure::Denied, Failure::Busy, Failure::Failed] {
        let state = fresh_state();
        state.borrow_mut().failure = Some(failure);
        let error = Lease::acquire_with(Engine::Pdf, MockTransport::default()).err().unwrap();
        assert!(error.contains("PDF engine is unavailable"));
        assert!(error.contains("No fallback"));
        assert_eq!(state.borrow().requests.len(), 1);
    }
    let state = fresh_state();
    state.borrow_mut().missing_broker = true;
    assert!(Lease::acquire_with(Engine::Pdf, MockTransport::default()).is_err());
    assert!(state.borrow().requests.is_empty());
    let state = fresh_state();
    state.borrow_mut().wrong_engine = true;
    assert!(Lease::acquire_with(Engine::JavaScript, MockTransport::default()).is_err());
}
#[test]
fn failed_release_ack_is_not_reported_as_success() {
    let state = fresh_state();
    let mut lease = Lease::acquire_with(Engine::Pdf, MockTransport::default()).unwrap();
    state.borrow_mut().release_bad_ack = true;
    assert!(lease.release().is_err());
    state.borrow_mut().release_bad_ack = false;
    lease.release().unwrap();
}

struct FakeOperations {
    clock: u64,
    busy: bool,
    pending: bool,
    cancelled: bool,
    submissions: usize,
    takes: usize,
    waits: Vec<u64>,
    // These messages are never accessed by exact-operation calls.
    legacy_messages: Vec<u8>,
}
impl Default for FakeOperations {
    fn default() -> Self { Self { clock: 0, busy: false, pending: false, cancelled: false, submissions: 0, takes: 0, waits: Vec::new(), legacy_messages: alloc::vec![1, 2, 3] } }
}
impl Operations for FakeOperations {
    fn now(&mut self) -> Option<u64> { Some(self.clock) }
    fn submit(&mut self, peer: usize, _: &[u8]) -> Result<usize, IpcSubmitError> {
        assert_eq!(peer, 20); self.submissions += 1;
        if self.busy { Err(IpcSubmitError::Busy) } else { Ok(77) }
    }
    fn take(&mut self, op: usize, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError> {
        assert_eq!(op, 77); self.takes += 1;
        if self.cancelled { Ok(IpcTakeResult::Terminal { status: IpcTerminal::Indeterminate, len: 0 }) }
        else if self.pending { Ok(IpcTakeResult::Pending) }
        else { reply[..2].copy_from_slice(&[9, 8]); Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len: 2 }) }
    }
    fn wait(&mut self, ticks: u64) { self.waits.push(ticks); self.clock += ticks; }
    fn yield_once(&mut self) { self.clock += 1; }
    fn cancel(&mut self, op: usize) { assert_eq!(op, 77); self.cancelled = true; }
}
#[test]
fn busy_admission_and_pending_reply_have_bounded_deadlines() {
    let mut ops = FakeOperations { busy: true, ..FakeOperations::default() };
    assert!(bounded_call(&mut ops, 20, &[1], &mut [0; 4], 5).is_err());
    assert_eq!(ops.submissions, 5);
    assert_eq!(ops.takes, 0);
    let mut ops = FakeOperations { pending: true, ..FakeOperations::default() };
    assert!(bounded_call(&mut ops, 20, &[1], &mut [0; 4], 5).is_err());
    assert_eq!(ops.submissions, 1);
    assert_eq!(ops.waits, alloc::vec![5]);
    assert!(ops.cancelled);
    assert_eq!(ops.takes, 3); // initial pending, deadline pending, cancellation terminal
    assert_eq!(ops.legacy_messages, alloc::vec![1, 2, 3]);
}
#[test]
fn exact_reply_returns_actual_length_without_receiving_legacy_messages() {
    let mut ops = FakeOperations::default();
    let mut reply = [0; 4];
    assert_eq!(bounded_call(&mut ops, 20, &[1], &mut reply, 5).unwrap(), 2);
    assert_eq!(reply, [9, 8, 0, 0]);
    assert!(!ops.cancelled);
    assert_eq!(ops.legacy_messages, alloc::vec![1, 2, 3]);
}
