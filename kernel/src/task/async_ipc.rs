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
    peer_tid: usize,
    peer_cell: u64,
    peer_generation: u64,
    deadline: u64,
    phase: Phase,
    reply: ipc_wire::IpcWireMessage,
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
        id: token, peer_tid: peer, peer_cell: peer_binding.0, peer_generation: peer_binding.1,
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
    let Some(owner) = sched.tasks.get_mut(&header.sender_tid) else { return; };
    if owner.cell_id.0 != header.sender_cell_id || owner.cell_generation != header.sender_generation { return; }
    let Some(op) = owner.async_operations.find_mut(header.async_op) else { return; };
    if op.peer_tid == peer && matches!(op.phase, Phase::Queued) {
        op.phase = Phase::Dispatched;
    }
}

fn terminal(sched: &mut Scheduler, owner_tid: usize, token: usize, peer: usize,
            peer_cell: u64, peer_generation: u64, ptr: usize, len: usize) -> Result<(), Failure> {
    let Some(owner) = sched.tasks.get(&owner_tid) else { return Err(Failure::PeerGone); };
    if !live(owner) { return Err(Failure::PeerGone); }
    let Some(op) = owner.async_operations.find(token) else { return Err(Failure::Invalid); };
    if op.peer_tid != peer || op.peer_cell != peer_cell || op.peer_generation != peer_generation
        || !matches!(op.phase, Phase::Dispatched) { return Err(Failure::Invalid); }
    let view = sched.tasks.get(&peer).map(|task| TaskCopyView::of(task)).ok_or(Failure::PeerGone)?;
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

/// Even a late legacy service reply is intercepted rather than misdelivered
/// through the synchronous client mailbox.
pub fn reply_current(peer: usize, destination: usize, ptr: usize, len: usize)
    -> Option<Result<(), Failure>> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut()?;
    let provider = sched.tasks.get(&peer)?;
    let token = provider.current_async_op;
    if token == 0 || provider.current_caller != Some(destination) { return None; }
    let binding = (provider.cell_id.0, provider.cell_generation);
    Some(match terminal(sched, destination, token, peer, binding.0, binding.1, ptr, len) {
        // Cancellation/reuse is terminal; quarantine the late response.
        Err(Failure::Invalid) if !sched.tasks.get(&destination)
            .and_then(|task| task.async_operations.find(token))
            .is_some_and(|op| matches!(op.phase, Phase::Dispatched)) => Ok(()),
        other => other,
    })
}

pub fn current(peer: usize) -> usize {
    SCHEDULER.lock().as_ref().and_then(|sched| sched.tasks.get(&peer))
        .filter(|task| live(task)).map_or(0, |task| task.current_async_op)
}

pub fn reply(peer: usize, token: usize, ptr: usize, len: usize) -> Result<(), Failure> {
    if token == 0 { return Err(Failure::Invalid); }
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    let provider = sched.tasks.get(&peer).filter(|task| live(task)).ok_or(Failure::PeerGone)?;
    let (cell, generation) = (provider.cell_id.0, provider.cell_generation);
    let owner = sched.tasks.iter().find_map(|(&id, task)| task.async_operations.find(token)
        .map(|_| id)).ok_or(Failure::Invalid)?;
    let result = terminal(sched, owner, token, peer, cell, generation, ptr, len);
    if result.is_ok() {
        if let Some(task) = sched.tasks.get_mut(&peer) {
            if task.current_async_op == token
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
    let op = sched.tasks.get_mut(&owner).ok_or(Failure::Invalid)?
        .async_operations.find_mut(token).ok_or(Failure::Invalid)?;
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
    let task = sched.tasks.get(&owner).ok_or(Failure::Invalid)?;
    let op = task.async_operations.find(token).ok_or(Failure::Invalid)?;
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
        let task = sched.tasks.get_mut(&owner).ok_or(Failure::Invalid)?;
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
        let mut ops = Operations::new();
        for id in 1..=MAX_OPERATIONS {
            ops.push(Operation { id, peer_tid: 5, peer_cell: 2, peer_generation: 7,
                deadline: 10, phase: Phase::Dispatched, reply: test_reply() }).unwrap();
        }
        assert!(matches!(ops.push(Operation { id: 65, peer_tid: 5, peer_cell: 2,
            peer_generation: 7, deadline: 10, phase: Phase::Queued, reply: test_reply() }), Err(Failure::Busy)));
        assert!(!ops.peer_died(5, 2, 8), "new peer generation cannot close old operations");
        assert!(!ops.peer_died(6, 2, 7), "unrelated peer cannot close operations");
        assert!(ops.peer_died(5, 2, 7));
        assert!(ops.has_terminal());
        assert!(!ops.peer_died(5, 2, 7));
        assert!(matches!(ops.remove(1).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PEER_GONE, .. }));
    }
    #[test]
    fn deadline_retains_terminal_results_and_removes_only_queued_work() {
        use alloc::boxed::Box;
        use types::CellId;
        let mut sched = Scheduler::new();
        let mut owner = Box::new(Task::new(901, CellId(2), "async-owner", Vec::new()));
        let mut peer = Box::new(Task::new(902, CellId(3), "async-peer", Vec::new()));
        owner.async_operations.push(Operation { id: 11, peer_tid: 902,
            peer_cell: 3, peer_generation: peer.cell_generation, deadline: 10,
            phase: Phase::Queued, reply: test_reply() }).unwrap();
        owner.async_operations.push(Operation { id: 12, peer_tid: 902,
            peer_cell: 3, peer_generation: peer.cell_generation, deadline: 10,
            phase: Phase::Dispatched, reply: test_reply() }).unwrap();
        let header = ipc_wire::IpcWireHeader { sender_tid: 901, sender_cell_id: 2,
            sender_generation: owner.cell_generation, delivery_id: 44, async_op: 11 };
        super::super::queue_wire_msg(&mut peer,
            ipc_wire::IpcWireMessage::try_new(header, b"queued").unwrap(), 64).unwrap();
        sched.tasks.insert(901, owner);
        sched.tasks.insert(902, peer);
        expire(&mut sched, 9);
        assert_eq!(sched.tasks.get(&902).unwrap().pending_msgs.len(), 1);
        expire(&mut sched, 10);
        assert!(sched.tasks.get(&902).unwrap().pending_msgs.is_empty());
        let operations = &sched.tasks.get(&901).unwrap().async_operations;
        assert!(matches!(operations.find(11).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT, .. }));
        assert!(matches!(operations.find(12).unwrap().phase,
            Phase::Terminal { kind: api::syscall::ipc_status::INDETERMINATE, .. }));
        assert_eq!(operations.len(), 2, "terminal results remain until take");
        expire(&mut sched, 11);
        assert_eq!(sched.tasks.get(&901).unwrap().async_operations.len(), 2);
    }
}
