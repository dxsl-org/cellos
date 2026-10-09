//! Kernel-owned, bounded request/reply operations. All transitions are serialized
//! by SCHEDULER; neither a reply nor a wake depends on caller-owned memory.

use super::{copy_glue::TaskCopyView, ipc_wire, scheduler::Scheduler, tcb::{Task, TaskState}, SCHEDULER};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_OPERATIONS: usize = 64;
/// Accepted operations have a kernel-enforced terminal deadline (30 seconds at
/// the scheduler's 10 ms tick). Service replies that never arrive cannot hold
/// completion reservations forever.
pub const OPERATION_TIMEOUT_TICKS: u64 = 3_000;
static NEXT_OPERATION: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure { Busy, PeerGone, Invalid, TooSmall }

#[derive(Debug)]
enum Phase { Queued, Dispatched, Terminal { kind: u32, len: usize } }

#[derive(Debug)]
pub struct Operation {
    id: usize,
    owner_cell: u64,
    owner_generation: u64,
    peer_tid: usize,
    peer_cell: u64,
    peer_generation: u64,
    deadline: u64,
    phase: Phase,
    reply: ipc_wire::IpcWireMessage,
}

impl Operation {
    fn owned_by(&self, task: &Task) -> bool {
        self.owner_cell == task.cell_id.0 && self.owner_generation == task.cell_generation
    }
}

/// Task-local, kernel-charged storage. Tokens are global monotonic identifiers,
/// never slot indexes; reuse after take cannot alias a late service reply.
#[derive(Default)]
pub struct Operations(Vec<Operation>);

impl Operations {
    pub fn new() -> Self { Self(Vec::new()) }
    pub fn len(&self) -> usize { self.0.len() }
    pub fn find(&self, id: usize) -> Option<&Operation> { self.0.iter().find(|op| op.id == id) }
    fn find_mut(&mut self, id: usize) -> Option<&mut Operation> { self.0.iter_mut().find(|op| op.id == id) }
    fn remove(&mut self, id: usize) -> Option<Operation> {
        let pos = self.0.iter().position(|op| op.id == id)?;
        Some(self.0.remove(pos))
    }
    fn push(&mut self, op: Operation) -> Result<(), Failure> {
        if self.0.len() == MAX_OPERATIONS { return Err(Failure::Busy); }
        if self.0.len() == self.0.capacity() {
            let previous = super::hart_local::current_cell_id();
            super::hart_local::set_current_cell_id(0);
            let reserved = self.0.try_reserve(1);
            super::hart_local::set_current_cell_id(previous);
            reserved.map_err(|_| Failure::Busy)?;
        }
        self.0.push(op);
        Ok(())
    }
    pub fn peer_died(&mut self, tid: usize, cell: u64, generation: u64) -> bool {
        let mut changed = false;
        for op in &mut self.0 {
            if op.peer_tid == tid && op.peer_cell == cell && op.peer_generation == generation
                && !matches!(op.phase, Phase::Terminal { .. }) {
                op.phase = Phase::Terminal { kind: api::syscall::ipc_status::PEER_GONE, len: 0 };
                changed = true;
            }
        }
        changed
    }
    fn has_terminal(&self) -> bool { self.0.iter().any(|op| matches!(op.phase, Phase::Terminal { .. })) }
}

impl Drop for Operations {
    fn drop(&mut self) {
        let rows = core::mem::take(&mut self.0);
        let previous = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        drop(rows);
        super::hart_local::set_current_cell_id(previous);
    }
}

fn live(task: &Task) -> bool { !matches!(task.state, TaskState::Retiring | TaskState::Terminated) }

fn wake(sched: &mut Scheduler, owner: usize) {
    if let Some(task) = sched.tasks.get_mut(&owner) {
        if matches!(task.state, TaskState::WaitIpc { .. }) {
            task.state = TaskState::Ready;
            let priority = sched.push_ready(owner);
            sched.pend_preempt_if_needed(priority);
        }
    }
}

fn new_token() -> Result<usize, Failure> {
    NEXT_OPERATION.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
        |next| (next <= isize::MAX as usize).then_some(next + 1))
        .map_err(|_| Failure::Busy)
}

pub fn submit(owner: usize, peer: usize, ptr: usize, len: usize) -> Result<usize, Failure> {
    if len > ipc_wire::MAX_IPC_WIRE_PAYLOAD { return Err(Failure::Invalid); }
    let (view, binding, peer_binding) = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(Failure::PeerGone)?;
        let task = sched.tasks.get(&owner).filter(|task| live(task)).ok_or(Failure::Invalid)?;
        let target = sched.tasks.get(&peer).filter(|task| live(task)).ok_or(Failure::PeerGone)?;
        (TaskCopyView::of(task), (task.cell_id.0, task.cell_generation),
         (target.cell_id.0, target.cell_generation))
    };
    let token = new_token()?;
    let header = ipc_wire::IpcWireHeader {
        sender_tid: owner, sender_cell_id: binding.0,
        sender_generation: binding.1,
        delivery_id: super::next_delivery_id(), async_op: token,
    };
    // Copy outside SCHEDULER; no peer or user pointer persists in a task slot.
    let wire = ipc_wire::IpcWireMessage::try_from_user(header, &view, ptr, len)
        .map_err(|_| Failure::Invalid)?;
    let reply_header = ipc_wire::IpcWireHeader {
        sender_tid: peer, sender_cell_id: peer_binding.0,
        sender_generation: peer_binding.1, delivery_id: 0, async_op: token,
    };
    let reply = ipc_wire::IpcWireMessage::try_new(
        reply_header, &[0u8; ipc_wire::MAX_IPC_WIRE_PAYLOAD]).map_err(|_| Failure::Busy)?;
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::PeerGone)?;
    let caller = sched.tasks.get(&owner).filter(|task| live(task)
        && (task.cell_id.0, task.cell_generation) == binding).ok_or(Failure::Invalid)?;
    if caller.async_operations.len() >= MAX_OPERATIONS { return Err(Failure::Busy); }
    let receiver = sched.tasks.get(&peer).filter(|task| live(task)
        && (task.cell_id.0, task.cell_generation) == peer_binding).ok_or(Failure::PeerGone)?;
    if super::paused_target_rejects(sched, owner, peer) { return Err(Failure::Busy); }
    if receiver.pending_msgs.len() >= super::tcb::HOTSWAP_MSG_QUEUE_DEPTH { return Err(Failure::Busy); }
    sched.tasks.get_mut(&owner).ok_or(Failure::Invalid)?.async_operations.push(Operation {
        id: token, owner_cell: binding.0, owner_generation: binding.1,
        peer_tid: peer, peer_cell: peer_binding.0, peer_generation: peer_binding.1,
        deadline: (super::system_ticks() as u64).saturating_add(OPERATION_TIMEOUT_TICKS),
        phase: Phase::Queued, reply,
    })?;
    let target = sched.tasks.get_mut(&peer).ok_or(Failure::PeerGone)?;
    let recv_eligible = matches!(target.state, TaskState::Recv { mask, .. } if mask == 0 || mask == owner);
    if super::queue_wire_msg(target, wire, super::tcb::HOTSWAP_MSG_QUEUE_DEPTH).is_err() {
        sched.tasks.get_mut(&owner).unwrap().async_operations.remove(token);
        return Err(Failure::Busy);
    }
    let wake_cause = super::wake_after_ipc_publish(target, recv_eligible);
    if wake_cause.made_runnable() {
        let priority = sched.push_ready(peer);
        sched.pend_preempt_if_needed(priority);
    }
    Ok(token)
}

/// Snapshotting the request is the dispatch boundary. If a copy-out later
/// faults, cancellation stays conservatively indeterminate, not falsely safe.
pub fn dispatch(sched: &mut Scheduler, peer: usize, header: ipc_wire::IpcWireHeader) {
    if header.async_op == 0 { return; }
    let Some(provider) = sched.tasks.get(&peer).filter(|task| live(task)) else { return; };
    let peer_binding = (provider.cell_id.0, provider.cell_generation);
    let Some(owner) = sched.tasks.get_mut(&header.sender_tid).filter(|task| live(task)) else { return; };
    if owner.cell_id.0 != header.sender_cell_id || owner.cell_generation != header.sender_generation { return; }
    let owner_binding = (owner.cell_id.0, owner.cell_generation);
    let Some(op) = owner.async_operations.find_mut(header.async_op) else { return; };
    if (op.owner_cell, op.owner_generation) == owner_binding
        && (op.peer_tid, op.peer_cell, op.peer_generation) == (peer, peer_binding.0, peer_binding.1)
        && matches!(op.phase, Phase::Queued) {
        op.phase = Phase::Dispatched;
    }
}

fn terminal(sched: &mut Scheduler, owner_tid: usize, token: usize, provider: usize,
            exact_peer: bool, ptr: usize, len: usize) -> Result<(), Failure> {
    let Some(owner) = sched.tasks.get(&owner_tid).filter(|task| live(task)) else {
        return Err(Failure::PeerGone);
    };
    let Some(op) = owner.async_operations.find(token) else { return Err(Failure::Invalid); };
    let Some(peer) = sched.tasks.get(&provider).filter(|task| live(task)) else {
        return Err(Failure::PeerGone);
    };
    // Explicit replies may be delegated to a worker of the accepted provider's
    // cell incarnation. Only the implicit bridge requires the served TID too.
    // Authority always comes from scheduler identity, never caller input.
    if !op.owned_by(owner) || (exact_peer && op.peer_tid != provider)
        || op.peer_cell != peer.cell_id.0 || op.peer_generation != peer.cell_generation
        || !matches!(op.phase, Phase::Dispatched) { return Err(Failure::Invalid); }
    let view = TaskCopyView::of(peer);
    let op = sched.tasks.get_mut(&owner_tid).unwrap().async_operations.find_mut(token).unwrap();
    if len > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        op.phase = Phase::Terminal { kind: api::syscall::ipc_status::INDETERMINATE, len: 0 };
        wake(sched, owner_tid);
        return Err(Failure::Invalid);
    }
    let copied = view.read_into(ptr, &mut op.reply.as_mut_slice()[..len]);
    op.phase = Phase::Terminal {
        kind: if copied.is_ok() { api::syscall::ipc_status::REPLY }
            else { api::syscall::ipc_status::INDETERMINATE },
        len: if copied.is_ok() { len } else { 0 },
    };
    wake(sched, owner_tid);
    copied.map_err(|_| Failure::Invalid)
}

/// Only a Send to the exact currently served caller can be a legacy reply.
/// Its token remains as a tombstone until another receive replaces the context:
/// duplicate/cancelled/late replies must not fall through to the raw mailbox.
/// Services sending same-destination events or deferring across receives must
/// capture the token and use explicit replies instead of this bridge.
pub fn reply_current(peer: usize, destination: usize, ptr: usize, len: usize)
    -> Option<Result<(), Failure>> {
    let mut guard = SCHEDULER.lock();
    reply_current_in_sched(guard.as_mut()?, peer, destination, ptr, len)
}

fn reply_current_in_sched(sched: &mut Scheduler, peer: usize, destination: usize,
                          ptr: usize, len: usize) -> Option<Result<(), Failure>> {
    let provider = sched.tasks.get(&peer).filter(|task| live(task))?;
    let token = provider.current_async_op;
    if token == 0 || provider.current_caller != Some(destination) { return None; }
    let owner_binding = (provider.current_caller_cell_id, provider.current_caller_cell_generation);
    let owner_matches = sched.tasks.get(&destination)
        .is_some_and(|task| (task.cell_id.0, task.cell_generation) == owner_binding);
    if !owner_matches { return Some(Ok(())); }
    Some(match terminal(sched, destination, token, peer, true, ptr, len) {
        // Invalid tokens and terminal results are quarantined, never raw Send.
        Err(Failure::Invalid) => Ok(()),
        other => other,
    })
}

pub fn current(peer: usize) -> usize {
    SCHEDULER.lock().as_ref().and_then(|sched| sched.tasks.get(&peer))
        .filter(|task| live(task)).map_or(0, |task| task.current_async_op)
}

pub fn reply(peer: usize, token: usize, ptr: usize, len: usize) -> Result<(), Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    reply_in_sched(sched, peer, token, ptr, len)
}

fn reply_in_sched(sched: &mut Scheduler, peer: usize, token: usize,
                  ptr: usize, len: usize) -> Result<(), Failure> {
    if token == 0 { return Err(Failure::Invalid); }
    let owner = sched.tasks.iter().find_map(|(&id, task)| task.async_operations.find(token)
        .map(|_| id));
    let result = owner.ok_or(Failure::Invalid)
        .and_then(|owner| terminal(sched, owner, token, peer, false, ptr, len));
    // An explicit response ends this request even if cancellation/take already
    // retired its token. Otherwise later one-way events would be mistaken for
    // stale implicit replies. VFS's exact lease cleanup is handled by syscall.
    if let Some(binding) = sched.tasks.get(&peer).filter(|task| live(task))
        .map(|task| (task.cell_id, task.cell_generation)) {
        for task in sched.tasks.values_mut() {
            if (task.cell_id, task.cell_generation) == binding
                && task.current_async_op == token
                && !crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize) {
                task.clear_current_caller_context();
            }
        }
    }
    result
}

pub fn cancel(owner: usize, token: usize) -> Result<(), Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    cancel_in_sched(sched, owner, token)
}

fn cancel_in_sched(sched: &mut Scheduler, owner: usize, token: usize) -> Result<(), Failure> {
    let task = sched.tasks.get(&owner).filter(|task| live(task)).ok_or(Failure::Invalid)?;
    let op = task.async_operations.find(token).ok_or(Failure::Invalid)?;
    if !op.owned_by(task) { return Err(Failure::Invalid); }
    let queued = matches!(op.phase, Phase::Queued);
    let peer = op.peer_tid;
    if queued {
        // No receiver snapshot has begun; remove only the matching token.
        if let Some(receiver) = sched.tasks.get_mut(&peer) {
            if let Some(pos) = receiver.pending_msgs.iter().position(|msg|
                msg.wire_header().is_some_and(|header| header.async_op == token)) {
                receiver.pending_msgs.remove(pos);
            }
        }
    }
    let op = sched.tasks.get_mut(&owner).unwrap().async_operations.find_mut(token).unwrap();
    if !matches!(op.phase, Phase::Terminal { .. }) {
        op.phase = Phase::Terminal { kind: if queued { api::syscall::ipc_status::CANCELLED }
            else { api::syscall::ipc_status::INDETERMINATE }, len: 0 };
        wake(sched, owner);
    }
    Ok(())
}

pub fn take(owner: usize, token: usize, reply_ptr: usize, reply_len: usize,
            status_ptr: usize) -> Result<bool, Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    let task = sched.tasks.get(&owner).filter(|task| live(task)).ok_or(Failure::Invalid)?;
    let op = task.async_operations.find(token).ok_or(Failure::Invalid)?;
    if !op.owned_by(task) { return Err(Failure::Invalid); }
    let Phase::Terminal { kind, len } = &op.phase else { return Ok(false); };
    let data = &op.reply.as_slice()[..*len];
    if data.len() > reply_len { return Err(Failure::TooSmall); }
    let view = TaskCopyView::of(task);
    view.validate_writable(status_ptr, api::syscall::IPC_STATUS_LEN).map_err(|_| Failure::Invalid)?;
    if !data.is_empty() {
        view.validate_writable(reply_ptr, data.len()).map_err(|_| Failure::Invalid)?;
        view.write_bytes(reply_ptr, data).map_err(|_| Failure::Invalid)?;
    }
    let mut status = [0u8; api::syscall::IPC_STATUS_LEN];
    status[..4].copy_from_slice(&api::syscall::IPC_STATUS_VERSION.to_le_bytes());
    status[4..8].copy_from_slice(&kind.to_le_bytes());
    status[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    view.write_bytes(status_ptr, &status).map_err(|_| Failure::Invalid)?;
    sched.tasks.get_mut(&owner).unwrap().async_operations.remove(token);
    Ok(true)
}

pub fn wait(owner: usize, ticks: u64) -> Result<bool, Failure> {
    let deadline = (ticks != 0).then(|| (super::system_ticks() as u64).saturating_add(ticks));
    {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(Failure::Invalid)?;
        let task = sched.tasks.get_mut(&owner).filter(|task| live(task)).ok_or(Failure::Invalid)?;
        if task.async_operations.has_terminal() { return Ok(true); }
        super::arm_ipc_block_handoff(owner);
        task.state = TaskState::WaitIpc { deadline };
    }
    super::yield_cpu();
    let guard = SCHEDULER.lock();
    Ok(guard.as_ref().and_then(|s| s.tasks.get(&owner))
        .is_some_and(|task| task.async_operations.has_terminal()))
}

/// Peer death is an infallible terminal transition; storage was reserved on submit.
pub fn peer_died(sched: &mut Scheduler, peer: usize, cell: u64, generation: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) { Some(next) => next, None => break };
        if sched.tasks.get_mut(&owner).is_some_and(|task|
            task.async_operations.peer_died(peer, cell, generation)) {
            wake(sched, owner);
        }
    }
}

/// Drop queued requests from an owner whose task generation has exited.
/// Dispatched requests retain their service-side grant lease until the
/// service's existing reply/next-public-receive cleanup boundary.
pub fn owner_died(sched: &mut Scheduler, owner: usize, cell: u64, generation: u64) {
    for task in sched.tasks.values_mut() {
        let mut index = 0;
        while index < task.pending_msgs.len() {
            let queued = task.pending_msgs.as_slice()[index].wire_header()
                .is_some_and(|header| header.async_op != 0
                    && header.sender_tid == owner
                    && header.sender_cell_id == cell
                    && header.sender_generation == generation);
            if queued { task.pending_msgs.remove(index); } else { index += 1; }
        }
    }
}

/// Complete queued requests tied to a provider being replaced. Called only
/// after the successful hot-swap commit, under SCHEDULER. The replacement
/// must not receive these wires because it is a different generation.
pub(crate) fn cutover_peer(sched: &mut Scheduler, peer: usize, cell: u64, generation: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) { Some(next) => next, None => break };
        let changed = if let Some(task) = sched.tasks.get_mut(&owner) {
            let mut changed = false;
            for op in &mut task.async_operations.0 {
                if op.peer_tid == peer && op.peer_cell == cell && op.peer_generation == generation
                    && matches!(op.phase, Phase::Queued) {
                    op.phase = Phase::Terminal {
                        kind: api::syscall::ipc_status::PEER_GONE, len: 0,
                    };
                    changed = true;
                }
            }
            changed
        } else { false };
        if changed { wake(sched, owner); }
    }
}

/// Expire accepted work without allocating in the scheduler's timer sweep.
/// An undispatched request is removed from its peer mailbox before reporting
/// PreDispatchTimeout. A dispatched request is conservatively indeterminate.
pub fn expire(sched: &mut Scheduler, now: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) { Some(next) => next, None => break };
        for index in 0..MAX_OPERATIONS {
            let expired = {
                let Some(task) = sched.tasks.get_mut(&owner) else { break };
                let Some(op) = task.async_operations.0.get_mut(index) else { break };
                if now < op.deadline || matches!(op.phase, Phase::Terminal { .. }) {
                    None
                } else {
                    let queued = matches!(op.phase, Phase::Queued);
                    op.phase = Phase::Terminal {
                        kind: if queued { api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT }
                            else { api::syscall::ipc_status::INDETERMINATE },
                        len: 0,
                    };
                    Some((op.id, op.peer_tid, queued))
                }
            };
            if let Some((token, peer, queued)) = expired {
                if queued {
                    if let Some(target) = sched.tasks.get_mut(&peer) {
                        if let Some(pos) = target.pending_msgs.iter().position(|msg|
                            msg.wire_header().is_some_and(|header| header.async_op == token)) {
                            target.pending_msgs.remove(pos);
                        }
                    }
                }
                wake(sched, owner);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_reply() -> ipc_wire::IpcWireMessage {
        ipc_wire::IpcWireMessage::try_new(ipc_wire::IpcWireHeader {
            sender_tid: 5, sender_cell_id: 2, sender_generation: 7,
            delivery_id: 0, async_op: 1,
        }, &[]).unwrap()
    }
    #[test]
    fn bounded_tokens_and_terminal_retention() {
        let mut sched = fixture();
        for id in 1..=MAX_OPERATIONS {
            add_operation(&mut sched, OWNER, PROVIDER, id, Phase::Dispatched);
        }
        let overflow = || Operation {
            id: MAX_OPERATIONS + 1, owner_cell: 1_002, owner_generation: 3,
            peer_tid: PROVIDER, peer_cell: 1_003, peer_generation: 7,
            deadline: 10, phase: Phase::Queued, reply: test_reply(),
        };
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert!(matches!(sched.tasks.get_mut(&OWNER).unwrap().async_operations.push(overflow()),
            Err(Failure::Busy)));
        peer_died(&mut sched, PROVIDER, 1_003, 8);
        peer_died(&mut sched, WORKER, 1_003, 7);
        assert!(!sched.tasks[&OWNER].async_operations.has_terminal(),
            "only the accepted provider incarnation can terminate these operations");
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert!(sched.tasks[&OWNER].async_operations.0.iter().all(|op|
            matches!(op.phase, Phase::Terminal { kind: api::syscall::ipc_status::PEER_GONE, len: 0 })));
        expire(&mut sched, u64::MAX);
        cancel_in_sched(&mut sched, OWNER, 1).unwrap();
        assert!(matches!(sched.tasks.get_mut(&OWNER).unwrap().async_operations.push(overflow()),
            Err(Failure::Busy)), "terminal results still charge the full quota");
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert!(sched.tasks[&PROVIDER].pending_msgs.is_empty());
        // Exercise the storage release performed only after a successful take.
        assert!(matches!(sched.tasks.get_mut(&OWNER).unwrap().async_operations.remove(1).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PEER_GONE, len: 0 }));
        add_operation(&mut sched, OWNER, PROVIDER, MAX_OPERATIONS + 1, Phase::Dispatched);
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 1, 0, 0), Err(Failure::Invalid));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(MAX_OPERATIONS + 1).unwrap().phase,
            Phase::Dispatched), "a retired token cannot alias the reused reservation");
    }
    #[test]
    fn deadline_retains_terminal_results_and_removes_only_queued_work() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Queued);
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Queued);
        add_operation(&mut sched, OWNER, PROVIDER, 13, Phase::Queued);
        sched.tasks.get_mut(&OWNER).unwrap().async_operations.find_mut(13).unwrap().deadline = 11;
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER, sender_cell_id: 1_002, sender_generation: 3,
            delivery_id: 44, async_op: 11,
        };
        for (token, payload) in [(11, b"due".as_slice()), (0, b"raw".as_slice()),
            (13, b"later".as_slice())] {
            super::super::queue_wire_msg(sched.tasks.get_mut(&PROVIDER).unwrap(),
                ipc_wire::IpcWireMessage::try_new(
                    ipc_wire::IpcWireHeader { async_op: token, ..header }, payload).unwrap(), 64).unwrap();
        }
        dispatch(&mut sched, PROVIDER, ipc_wire::IpcWireHeader { async_op: 12, ..header });
        serve(&mut sched, PROVIDER, OWNER, 12);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        expire(&mut sched, 9);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::WaitIpc { .. }));
        assert!(!sched.tasks[&OWNER].async_operations.has_terminal());
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase, Phase::Queued));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase, Phase::Dispatched));
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 3);
        expire(&mut sched, 10);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        super::super::hart_local::ready::remove_from_all(OWNER);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT, len: 0 }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::INDETERMINATE, len: 0 }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(13).unwrap().phase, Phase::Queued));
        let remaining = sched.tasks[&PROVIDER].pending_msgs.as_slice();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].wire_header().unwrap().async_op, 0);
        assert_eq!(remaining[1].wire_header().unwrap().async_op, 13);
        assert_eq!(sched.tasks[&PROVIDER].current_caller, Some(OWNER));
        assert_eq!(sched.tasks[&PROVIDER].current_async_op, 12,
            "expiry cannot release a dispatched service context");
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 11, 0, 0), Err(Failure::Invalid));
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 12, 0, 0), Err(Failure::Invalid));
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        cancel_in_sched(&mut sched, OWNER, 12).unwrap();
        expire(&mut sched, 11);
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        expire(&mut sched, u64::MAX);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT, len: 0 }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::INDETERMINATE, len: 0 }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(13).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT, len: 0 }));
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), 3, "expiry never consumes a result");
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 1);
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.as_slice()[0].wire_header().unwrap().async_op, 0);
        assert!(sched.tasks[&OWNER].pending_msgs.is_empty(), "late replies cannot become raw events");
    }

    const OWNER: usize = 71_101;
    const PROVIDER: usize = 71_102;
    const WORKER: usize = 71_103;
    const FOREIGN_GENERATION: usize = 71_104;
    const FOREIGN_CELL: usize = 71_105;

    fn fixture() -> Scheduler {
        use alloc::boxed::Box;
        use types::CellId;
        let mut sched = Scheduler::new();
        for (tid, cell, generation) in [
            (OWNER, 1_002, 3), (PROVIDER, 1_003, 7), (WORKER, 1_003, 7),
            (FOREIGN_GENERATION, 1_003, 8), (FOREIGN_CELL, 1_004, 7),
        ] {
            let mut task = Box::new(Task::new(tid, CellId(cell), "async-fixture", Vec::new()));
            task.cell_generation = generation;
            sched.tasks.insert(tid, task);
        }
        sched
    }

    fn add_operation(sched: &mut Scheduler, owner: usize, peer: usize, id: usize, phase: Phase) {
        let provider = sched.tasks.get(&peer).unwrap();
        let (peer_cell, peer_generation) = (provider.cell_id.0, provider.cell_generation);
        let task = sched.tasks.get_mut(&owner).unwrap();
        task.async_operations.push(Operation {
            id, owner_cell: task.cell_id.0, owner_generation: task.cell_generation,
            peer_tid: peer, peer_cell, peer_generation, deadline: 10,
            phase, reply: test_reply(),
        }).unwrap();
    }

    fn serve(sched: &mut Scheduler, peer: usize, owner: usize, token: usize) {
        let task = sched.tasks.get(&owner).unwrap();
        let (cell, generation) = (task.cell_id.0, task.cell_generation);
        sched.tasks.get_mut(&peer).unwrap()
            .set_received_request_context(owner, cell, generation, token);
    }

    #[test]
    fn immediate_bridge_settles_only_served_token_and_preserves_reply() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, WORKER, 0, 0), None);
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::REPLY, .. }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Dispatched));
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        expire(&mut sched, 100);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::REPLY, .. }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PEER_GONE, .. }));
        assert!(sched.tasks[&OWNER].pending_msgs.is_empty());
    }

    #[test]
    fn cancelled_and_consumed_late_reply_cannot_complete_successor() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::INDETERMINATE, .. }));
        sched.tasks.get_mut(&OWNER).unwrap().async_operations.remove(11).unwrap();
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Dispatched);
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 11, 0, 0), Err(Failure::Invalid));
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), None,
            "an explicit retired-token reply ends the implicit context");
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Dispatched));
        assert!(sched.tasks[&OWNER].pending_msgs.is_empty());
        serve(&mut sched, PROVIDER, OWNER, 12);
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
    }

    #[test]
    fn dispatch_and_explicit_worker_reply_check_both_incarnations() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Queued);
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 99, 0, 0), Err(Failure::Invalid),
            "a guessed token never authorizes a provider reply");
        assert_eq!(reply_in_sched(&mut sched, WORKER, 11, 0, 0), Err(Failure::Invalid),
            "a genuine queued token cannot reply before dispatch");
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER, sender_cell_id: 1_002, sender_generation: 3,
            delivery_id: 1, async_op: 11,
        };
        sched.tasks.get_mut(&PROVIDER).unwrap().cell_generation = 8;
        dispatch(&mut sched, PROVIDER, header);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase, Phase::Queued));
        sched.tasks.get_mut(&PROVIDER).unwrap().cell_generation = 7;
        dispatch(&mut sched, PROVIDER, ipc_wire::IpcWireHeader { sender_generation: 4, ..header });
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase, Phase::Queued));
        dispatch(&mut sched, PROVIDER, header);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase, Phase::Dispatched));
        serve(&mut sched, PROVIDER, OWNER, 11);
        assert_eq!(reply_in_sched(&mut sched, FOREIGN_GENERATION, 11, 0, 0), Err(Failure::Invalid));
        assert_eq!(reply_in_sched(&mut sched, FOREIGN_CELL, 11, 0, 0), Err(Failure::Invalid));
        serve(&mut sched, WORKER, OWNER, 11);
        assert_eq!(reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0), Some(Ok(())));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase, Phase::Dispatched),
            "the implicit bridge cannot use sibling-worker authority");
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation = 4;
        assert_eq!(reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0), Some(Ok(())));
        assert_eq!(reply_in_sched(&mut sched, WORKER, 11, 0, 0), Err(Failure::Invalid));
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation = 3;
        assert_eq!(reply_in_sched(&mut sched, WORKER, 11, 0, 0), Ok(()));
        assert_eq!(sched.tasks[&PROVIDER].current_async_op, 0,
            "worker completion ends the original receiver context");
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), None,
            "a subsequent one-way event must not be quarantined as duplicate RPC");
        assert_eq!(reply_in_sched(&mut sched, WORKER, 11, 0, 0), Err(Failure::Invalid));
    }

    #[test]
    fn outgoing_operation_terminal_does_not_replace_inbound_context() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        add_operation(&mut sched, PROVIDER, WORKER, 21, Phase::Dispatched);
        cancel_in_sched(&mut sched, PROVIDER, 21).unwrap();
        assert_eq!(sched.tasks[&PROVIDER].current_caller, Some(OWNER));
        assert_eq!(sched.tasks[&PROVIDER].current_async_op, 11);
        assert_eq!(reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0), Some(Ok(())));
    }

    #[test]
    fn peer_death_publishes_terminal_before_waking_only_rpc_waiter() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        add_operation(&mut sched, WORKER, PROVIDER, 12, Phase::Queued);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        sched.tasks.get_mut(&WORKER).unwrap().state = TaskState::Recv {
            mask: PROVIDER, buf_ptr: 0, buf_len: 0, deadline: None,
        };
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        assert!(sched.tasks[&OWNER].async_operations.has_terminal());
        assert!(matches!(sched.tasks[&WORKER].state, TaskState::Recv { mask: PROVIDER, .. }));
        assert!(sched.tasks[&WORKER].async_operations.has_terminal());
        super::super::hart_local::ready::remove_from_all(OWNER);
    }

    #[test]
    fn queued_cancel_removes_only_exact_request_and_is_terminal() {
        let mut sched = fixture();
        for token in [11, 12] {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Queued);
            let header = ipc_wire::IpcWireHeader {
                sender_tid: OWNER, sender_cell_id: 1_002, sender_generation: 3,
                delivery_id: token as u64, async_op: token,
            };
            super::super::queue_wire_msg(sched.tasks.get_mut(&PROVIDER).unwrap(),
                ipc_wire::IpcWireMessage::try_new(header, b"queued").unwrap(), 64).unwrap();
        }
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::CANCELLED, .. }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase, Phase::Queued));
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 1);
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.as_slice()[0].wire_header().unwrap().async_op, 12);
        assert_eq!(reply_in_sched(&mut sched, PROVIDER, 11, 0, 0), Err(Failure::Invalid));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::CANCELLED, .. }));
        expire(&mut sched, 9);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::CANCELLED, len: 0 }));
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase, Phase::Queued));
        expire(&mut sched, 10);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::CANCELLED, len: 0 }),
            "the exact deadline cannot replace an already-terminal cancellation");
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT, len: 0 }));
        assert!(sched.tasks[&PROVIDER].pending_msgs.is_empty());
    }

    #[test]
    fn caller_death_removes_only_exact_queued_identity_and_preserves_dispatch_context() {
        let mut sched = fixture();
        for token in [11, 12, 13, 14, 15, 16] {
            add_operation(&mut sched, OWNER, if token >= 15 { WORKER } else { PROVIDER },
                token, Phase::Queued);
        }
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER, sender_cell_id: 1_002, sender_generation: 3,
            delivery_id: 13, async_op: 13,
        };
        dispatch(&mut sched, PROVIDER, header);
        serve(&mut sched, PROVIDER, OWNER, 13);
        assert!(matches!(sched.tasks[&OWNER].async_operations.find(13).unwrap().phase, Phase::Dispatched));
        let context = |task: &Task| (
            task.current_caller, task.current_caller_cell_id, task.current_caller_cell_generation,
            task.current_caller_request_generation, task.current_async_op,
        );
        let dispatched_context = context(&sched.tasks[&PROVIDER]);
        assert_ne!(dispatched_context.3, 0);
        let queued = [
            (PROVIDER, ipc_wire::IpcWireHeader { delivery_id: 11, async_op: 11, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader { delivery_id: 12, async_op: 12, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader {
                delivery_id: 21, async_op: 21, sender_generation: 4, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader {
                delivery_id: 22, async_op: 22, sender_cell_id: 1_004, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader {
                delivery_id: 23, async_op: 23, sender_tid: FOREIGN_CELL, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader { delivery_id: 24, async_op: 0, ..header }),
            (PROVIDER, ipc_wire::IpcWireHeader { delivery_id: 14, async_op: 14, ..header }),
            (WORKER, ipc_wire::IpcWireHeader { delivery_id: 15, async_op: 15, ..header }),
            (WORKER, ipc_wire::IpcWireHeader { delivery_id: 25, async_op: 0, ..header }),
            (WORKER, ipc_wire::IpcWireHeader { delivery_id: 16, async_op: 16, ..header }),
        ];
        for (peer, wire_header) in queued {
            super::super::queue_wire_msg(sched.tasks.get_mut(&peer).unwrap(),
                ipc_wire::IpcWireMessage::try_new(wire_header, b"preserved").unwrap(), 64).unwrap();
        }
        owner_died(&mut sched, OWNER + 100, 1_002, 3);
        owner_died(&mut sched, OWNER, 0, 3);
        owner_died(&mut sched, OWNER, 1_002, 99);
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 7);
        assert_eq!(sched.tasks[&WORKER].pending_msgs.len(), 3);
        // Death cleanup must use the captured identity even after the caller
        // has been removed, without releasing an already-dispatched context.
        sched.tasks.remove(&OWNER).unwrap();
        owner_died(&mut sched, OWNER, 1_002, 3);
        owner_died(&mut sched, OWNER, 1_002, 3);
        assert_eq!(context(&sched.tasks[&PROVIDER]), dispatched_context);
        for peer in [PROVIDER, WORKER] {
            let remaining: Vec<_> = sched.tasks[&peer].pending_msgs.as_slice().iter()
                .map(|msg| {
                    let h = msg.wire_header().unwrap();
                    (h.sender_tid, h.sender_cell_id, h.sender_generation, h.delivery_id, h.async_op)
                }).collect();
            let expected: Vec<_> = queued.iter().filter(|(tid, h)|
                *tid == peer && !(h.async_op != 0 && h.sender_tid == OWNER
                    && h.sender_cell_id == 1_002 && h.sender_generation == 3))
                .map(|(_, h)| (h.sender_tid, h.sender_cell_id, h.sender_generation, h.delivery_id, h.async_op))
                .collect();
            assert_eq!(remaining, expected, "unrelated generations, cells, TIDs and raw IPC survive in order");
        }
    }
}
