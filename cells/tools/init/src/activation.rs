//! Init's fixed-target activation endpoint. Readiness uses exact operations,
//! so waiting for a provider never consumes init's requests or death records.
use api::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use api::ipc::{VfsRequest, VfsResponse};
use api::syscall::{service, ProcessInfo};
use ocel_service_proto::{Engine, Failure, Request, Response, FRAME_BYTES};
use ostd::ipc::{self, IpcTakeResult, IpcTerminal};
use ostd::syscall::{self, SyscallResult};

use crate::activation_state::{Effects, Owner, Phase, Principal, Provider, State};
use crate::service_table::now_ticks;

const PAYLOAD_BYTES: usize = 64;
const MESSAGE_BYTES: usize = PAYLOAD_BYTES + CALLER_IDENTITY_LEN;
const DEFERRED_MESSAGES: usize = 32;
const READY_TICKS: u64 = 300;
const LEGACY_TICKS: u64 = 50;
// A finite polling backstop also bounds a broken/stalled scheduler clock.
const POLL_LIMIT: usize = 100_000;

#[derive(Clone, Copy)]
pub(crate) struct Message {
    pub sender: usize,
    pub bytes: [u8; MESSAGE_BYTES],
    pub operation: Option<ipc::IpcOpId>,
    pub identity: Option<CallerIdentity>,
    // A deferred request must attest/watch while its receive context is current.
    watched: Option<Owner>,
    watch_prepared: bool,
}

impl Message {
    pub fn receive(mask: usize, blocking: bool) -> Option<Self> {
        let mut bytes = [0u8; MESSAGE_BYTES];
        let result = if blocking { syscall::sys_recv_attested(mask, &mut bytes) }
            else { syscall::sys_try_recv_attested(mask, &mut bytes) };
        // Capture before another receive can replace the kernel's request context.
        let operation = ipc::current();
        match result {
            SyscallResult::Ok(sender) if sender != 0 && sender != usize::MAX => Some(Self {
                sender, operation, identity: CallerIdentity::from_recv_buf(&bytes), bytes,
                watched: None, watch_prepared: false,
            }),
            _ => None,
        }
    }

    fn principal(&self) -> Option<Principal> {
        let identity = self.identity?;
        (identity.sender_tid == self.sender as u64 && identity.cell_id != 0 && identity.generation != 0)
            .then_some(Principal { cell_id: identity.cell_id, generation: identity.generation })
    }

    pub fn reason(&self) -> u64 { u64::from_le_bytes(self.bytes[..8].try_into().unwrap()) }
}

pub(crate) struct Activator {
    state: State,
    deferred: [Option<Message>; DEFERRED_MESSAGES],
    deferred_len: usize,
    // Legacy VFS cannot cancel a dispatched operation. Do not confuse a late
    // grant-read reply with another read on that same provider incarnation.
    poisoned_vfs: Option<usize>,
}

impl Activator {
    pub const fn new() -> Self {
        Self { state: State::new(), deferred: [None; DEFERRED_MESSAGES], deferred_len: 0, poisoned_vfs: None }
    }

    pub fn next_message(&mut self) -> Option<Message> {
        if self.deferred_len == 0 { return Message::receive(0, true); }
        let message = self.deferred[0].take();
        self.deferred.copy_within(1..self.deferred_len, 0);
        self.deferred_len -= 1;
        self.deferred[self.deferred_len] = None;
        message
    }

    fn watch(&self, principal: Principal, sender: usize) -> Option<Owner> {
        if sender == 0 || task_alive(sender) != Some(true) { return None; }
        if !matches!(syscall::sys_notify_on_exit(sender), SyscallResult::Ok(0)) { return None; }
        Some(Owner { principal, root_tid: sender, watch: sender as u64 })
    }

    fn defer(&mut self, mut message: Message) {
        if matches!(Request::decode(&message.bytes[..FRAME_BYTES]), Some(Request::Acquire { .. })) {
            message.watch_prepared = true;
            message.watched = message.principal().and_then(|principal| self.watch(principal, message.sender));
        }
        // The exchange checks capacity BEFORE receiving, so no message is lost.
        self.deferred[self.deferred_len] = Some(message);
        self.deferred_len += 1;
    }

    /// Returns true for an activation frame or a known demand-engine death.
    /// Only the kernel-written ABSENCE of a trailer plus independently observed
    /// death can reclaim resources. An ordinary caller cannot forge an exit.
    pub fn handle(&mut self, mut message: Message) -> bool {
        if let Some(request) = Request::decode(&message.bytes[..FRAME_BYTES]) {
            let engine = request.engine();
            let Some(principal) = message.principal() else {
                self.cancel_provisional(&message);
                deliver(&message, Response::Error { engine, failure: Failure::Denied });
                return true;
            };
            let result = match request {
                Request::Acquire { .. } => self.acquire(&mut message, principal, engine),
                Request::Release { lease, .. } => {
                    match self.state.release(principal, engine, lease) {
                        Ok(effects) => { self.apply(effects); Ok(Response::Released { engine }) }
                        Err(failure) => Err(failure),
                    }
                }
            };
            if let Err(failure) = result { deliver(&message, Response::Error { engine, failure }); }
            else if let Ok(response @ Response::Released { .. }) = result { deliver(&message, response); }
            return true;
        }
        self.cancel_provisional(&message);
        if message.identity.is_some() || task_alive(message.sender) != Some(false) { return false; }
        if let Some(effects) = self.state.engine_dead(message.sender) {
            self.apply(effects);
            if self.state.watches_root(message.sender) {
                let effects = self.state.owner_dead(message.sender);
                self.apply(effects);
            }
            ostd::io::println("Init: demand engine exited — leases revoked, no restart.");
            return true;
        }
        if self.state.watches_root(message.sender) {
            let effects = self.state.owner_dead(message.sender);
            self.apply(effects);
        }
        false
    }

    fn cancel_provisional(&self, _message: &Message) {}

    fn acquire(&mut self, message: &mut Message, principal: Principal, engine: Engine) -> Result<Response, Failure> {
        // Revocation happens before preflight, including an independently
        // replaced service registry row. Never return an old provider's lease.
        if let Some(provider) = self.state.provider(engine) {
            if task_alive(provider.tid) == Some(false) {
                if let Some(effects) = self.state.engine_dead(provider.tid) { self.apply(effects); }
            } else if provider.phase == Phase::Ready && syscall::sys_lookup_service(service_id(engine)) != Some(provider.tid) {
                let effects = self.state.retire(engine);
                self.apply(effects);
            }
        }
        if let Err(failure) = self.state.preflight(principal, engine) {
            self.cancel_provisional(message);
            return Err(failure);
        }
        let installed = self.state.owner(principal);
        let owner = match installed {
            Some(owner) => {
                self.cancel_provisional(message);
                owner
            }
            None => {
                let watched = if message.watch_prepared { message.watched } else { self.watch(principal, message.sender) };
                let Some(owner) = watched else { return Err(Failure::Denied); };
                message.watched = Some(owner);
                owner
            }
        };
        let result = self.acquire_watched(message, owner, engine);
        // Failed first demand never retains an otherwise-unused owner watch.
        if result.is_err() { self.cancel_provisional(message); }
        result
    }

    fn acquire_watched(&mut self, message: &Message, owner: Owner, engine: Engine) -> Result<Response, Failure> {
        if task_alive(owner.root_tid) != Some(true) { return Err(Failure::Denied); }
        if self.state.provider(engine).is_none() { self.start_engine(engine)?; }
        let provider = self.state.provider(engine).ok_or(Failure::Failed)?;
        if provider.phase != Phase::Ready || task_alive(provider.tid) != Some(true)
            || syscall::sys_lookup_service(service_id(engine)) != Some(provider.tid)
        {
            let effects = self.state.retire(engine);
            self.apply(effects);
            return Err(Failure::Failed);
        }
        let root_alive = task_alive(owner.root_tid);
        if root_alive != Some(true) {
            // A dead root revokes its demand; an uncertain liveness query must
            // not revoke existing demand. Either case retires an unused start.
            let effects = if root_alive == Some(false) { self.state.owner_dead(owner.root_tid) }
                else { self.state.unused() };
            self.apply(effects);
            return Err(Failure::Denied);
        }
        let (lease, newly_allocated) = match self.state.acquire(owner, engine) {
            Ok(lease) => lease,
            Err(failure) => {
                let effects = self.state.unused();
                self.apply(effects);
                return Err(failure);
            }
        };
        let response = Response::Ready { engine, lease: lease.token, tid: provider.tid as u64 };
        if !deliver(message, response) {
            if newly_allocated {
                if let Ok(effects) = self.state.release(owner.principal, engine, lease.token) { self.apply(effects); }
            }
            // Reply cancellation is not a reason to revoke previously delivered
            // demand or another owner's share of this engine.
            return Err(Failure::Failed);
        }
        Ok(response)
    }

    fn start_engine(&mut self, engine: Engine) -> Result<(), Failure> {
        let tid = match syscall::sys_spawn_from_path_raw(engine.path()) {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => self.spawn_via_vfs(engine)?,
        };
        let provider = match self.state.start(engine, tid) {
            Ok(provider) => provider,
            Err(failure) => { let _ = syscall::sys_force_exit(tid); return Err(failure); }
        };
        let ready = matches!(syscall::sys_notify_on_exit(tid), SyscallResult::Ok(0))
            && readiness(provider)
            && task_alive(tid) == Some(true)
            && matches!(syscall::sys_register_service(service_id(engine), tid), SyscallResult::Ok(0))
            && syscall::sys_lookup_service(service_id(engine)) == Some(tid)
            && task_alive(tid) == Some(true);
        if !ready || !self.state.ready(provider) {
            let effects = self.state.retire(engine);
            self.apply(effects);
            return Err(Failure::Failed);
        }
        ostd::io::println(match engine {
            Engine::JavaScript => "Init: demand QuickJS ready (Tier 2).",
            Engine::Pdf => "Init: demand PDF ready (Tier 2).",
        });
        Ok(())
    }

    fn apply(&mut self, effects: Effects) {
        let _ = &effects.cancel;
        for provider in effects.retire.into_iter().flatten() {
            if matches!(syscall::sys_force_exit(provider.tid), SyscallResult::Ok(0))
                || task_alive(provider.tid) == Some(false)
            {
                self.state.terminated(provider);
                ostd::io::println("Init: unused demand engine stopped.");
            }
            // Failed termination retains Retiring until authenticated death.
        }
    }

    /// Same Stat -> shared Grant -> ReadFileGrant -> SpawnFromElf route as
    /// ostd::sys_spawn_from_path, with bounded waits and retained init messages.
    fn spawn_via_vfs(&mut self, engine: Engine) -> Result<usize, Failure> {
        let vfs = syscall::sys_lookup_service(service::VFS).ok_or(Failure::Unavailable)?;
        if self.poisoned_vfs == Some(vfs) { return Err(Failure::Unavailable); }
        let stat = self.vfs_exchange(vfs, &VfsRequest::Stat(engine.path()))?;
        let size = match api::ipc::decode::<VfsResponse<'_>>(&stat[..PAYLOAD_BYTES]) {
            Ok(VfsResponse::Stat { size, is_dir: false }) if size > 0 => usize::try_from(size).map_err(|_| Failure::Failed)?,
            _ => return Err(Failure::Unavailable),
        };
        let grant = syscall::sys_grant_alloc(size).ok_or(Failure::Busy)?;
        if !syscall::sys_grant_share(grant, vfs, 2) {
            syscall::sys_grant_free(grant);
            return Err(Failure::Denied);
        }
        let read = self.vfs_exchange(vfs, &VfsRequest::ReadFileGrant { path: engine.path(), grant, max: size });
        let attempt = match read {
            Ok(bytes) => match api::ipc::decode::<VfsResponse<'_>>(&bytes[..PAYLOAD_BYTES]) {
                Ok(VfsResponse::GrantDone { bytes }) if bytes > 0 && bytes <= size => {
                    match syscall::sys_spawn_from_elf(grant, bytes, engine.path()) {
                        SyscallResult::Ok(tid) if tid != 0 => Ok(tid),
                        _ => Err(Failure::Failed),
                    }
                }
                _ => Err(Failure::Failed),
            },
            Err(failure) => Err(failure),
        };
        syscall::sys_grant_free(grant);
        attempt
    }

    fn vfs_exchange(&mut self, peer: usize, request: &VfsRequest<'_>) -> Result<[u8; MESSAGE_BYTES], Failure> {
        let mut send = [0u8; 512];
        let bytes = api::ipc::encode(request, &mut send).map_err(|_| Failure::Failed)?;
        let started = now_ticks();
        let mut admitted = false;
        for _ in 0..POLL_LIMIT {
            if now_ticks().wrapping_sub(started) >= LEGACY_TICKS { break; }
            if !admitted {
                admitted = matches!(syscall::sys_try_send(peer, bytes), SyscallResult::Ok(0));
            } else {
                // A masked receive can still yield ANY pending death record.
                // If full, leave the next message in the kernel and fail safely.
                if self.deferred_len == DEFERRED_MESSAGES { break; }
                if let Some(message) = Message::receive(peer, false) {
                    if message.sender == peer && message.principal().is_some() { return Ok(message.bytes); }
                    self.defer(message);
                }
            }
            ostd::task::yield_now();
        }
        if admitted { self.poisoned_vfs = Some(peer); }
        Err(Failure::Unavailable)
    }
}

pub(crate) fn service_id(engine: Engine) -> u16 {
    match engine { Engine::JavaScript => service::OCEL_JS, Engine::Pdf => service::OCEL_PDF }
}

fn deliver(message: &Message, response: Response) -> bool {
    let frame = response.encode();
    if let Some(operation) = message.operation { return ipc::reply(operation, &frame).is_ok(); }
    let started = now_ticks();
    for _ in 0..POLL_LIMIT {
        if matches!(syscall::sys_try_send(message.sender, &frame), SyscallResult::Ok(0)) { return true; }
        if now_ticks().wrapping_sub(started) >= LEGACY_TICKS { break; }
        ostd::task::yield_now();
    }
    false
}

fn readiness(provider: Provider) -> bool {
    let mut send = [0u8; 16];
    let encoded = match provider.engine {
        Engine::JavaScript => api::ipc::encode(&dom_arena::OcelJsRequest::Ping, &mut send),
        Engine::Pdf => api::ipc::encode(&ocel_pdf_proto::Request::Ping, &mut send),
    };
    let Ok(encoded) = encoded else { return false; };
    let Ok(operation) = ipc::submit(provider.tid, encoded) else { return false; };
    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    let started = now_ticks();
    for _ in 0..POLL_LIMIT {
        match ipc::take(operation, &mut reply) {
            Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len }) => {
                return match provider.engine {
                    Engine::JavaScript => matches!(api::ipc::decode::<dom_arena::OcelJsReply>(&reply[..len]), Ok(dom_arena::OcelJsReply::Ready)),
                    Engine::Pdf => matches!(api::ipc::decode::<ocel_pdf_proto::Response>(&reply[..len]), Ok(ocel_pdf_proto::Response::Ready)),
                };
            }
            Ok(IpcTakeResult::Pending) => {
                if now_ticks().wrapping_sub(started) >= READY_TICKS { break; }
                // A finite one-tick park never touches legacy receive messages.
                ipc::wait(1);
            }
            Ok(IpcTakeResult::Terminal { .. }) => return false,
            Err(_) => break,
        }
    }
    let _ = ipc::cancel(operation);
    let _ = ipc::take(operation, &mut reply);
    false
}

/// Absence from a truncated process table is NOT proof of death. Most images
/// fit the stack buffer; larger tables grow only for a liveness query, with a
/// hard bound and fail-closed uncertainty instead of forged cleanup authority.
pub(crate) fn task_alive(tid: usize) -> Option<bool> {
    let mut stack = [ProcessInfo::default(); 64];
    let count = syscall::sys_get_procs(&mut stack).ok()?;
    if let Some(row) = stack.iter().take(count).find(|row| row.id == tid) { return Some(row.state != 3); }
    if count < stack.len() { return Some(false); }
    let mut rows = alloc::vec![ProcessInfo::default(); 128];
    loop {
        let count = syscall::sys_get_procs(&mut rows).ok()?;
        if let Some(row) = rows.iter().take(count).find(|row| row.id == tid) { return Some(row.state != 3); }
        if count < rows.len() { return Some(false); }
        if rows.len() >= 8192 { return None; }
        rows.resize(rows.len() * 2, ProcessInfo::default());
    }
}
