//! Kernel-owned, bounded request/reply operations. All transitions are serialized
//! by SCHEDULER; neither a reply nor a wake depends on caller-owned memory.

use super::{
    copy_glue::TaskCopyView,
    tcb::{Task, TaskState},
    SCHEDULER,
};
use crate::ipc::ipc_wire;
use crate::sched::scheduler::Scheduler;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_OPERATIONS: usize = 64;
/// Accepted operations have a kernel-enforced terminal deadline (30 seconds at
/// the scheduler's 10 ms tick). Service replies that never arrive cannot hold
/// completion reservations forever.
pub const OPERATION_TIMEOUT_TICKS: u64 = 3_000;
static NEXT_OPERATION: AtomicUsize = AtomicUsize::new(1);

/// Phase 03e admission headroom: the last `PEER_QUEUE_HEADROOM` slots of a
/// receiver's mailbox admit an operation only from a Cell incarnation that
/// holds no queued operation wire there (all its threads count as one holder).
/// One Cell can then hold at most `HOTSWAP_MSG_QUEUE_DEPTH - PEER_QUEUE_HEADROOM`
/// queued wires and, while no raw or kernel-inserted records are queued at the
/// receiver, a Cell holding none is refused for capacity only when at least
/// `PEER_QUEUE_HEADROOM + 1` other Cells each hold one (Spec 20 §2.6).
const PEER_QUEUE_HEADROOM: usize = 8;

/// Whether `receiver`'s mailbox admits one more operation wire from the Cell
/// incarnation `sender`. Identity is the kernel-written wire header; the scan
/// is bounded by the mailbox depth and allocates nothing.
fn peer_queue_admits(receiver: &Task, sender: (u64, u64), headroom: usize) -> bool {
    let depth = super::tcb::HOTSWAP_MSG_QUEUE_DEPTH;
    let len = receiver.pending_msgs.len();
    if len >= depth {
        return false;
    }
    len < depth - headroom
        || !receiver.pending_msgs.iter().any(|message| {
            message.wire_header().is_some_and(|header| {
                header.async_op != 0 && (header.sender_cell_id, header.sender_generation) == sender
            })
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Busy,
    PeerGone,
    Invalid,
    TooSmall,
}

#[derive(Debug)]
enum Phase {
    Queued,
    Dispatched,
    Terminal { kind: u32, len: usize },
}

#[derive(Debug)]
pub struct Operation {
    id: usize,
    owner_cell: u64,
    owner_generation: u64,
    pub(crate) owner_tid: usize,
    pub(crate) consumer_tid: usize,
    peer_tid: usize,
    peer_cell: u64,
    peer_generation: u64,
    deadline: u64,
    phase: Phase,
    /// Phase 03d one-shot completion notification: set once a multi-source
    /// `WaitCompletion` has reported this terminal. Never consulted by
    /// `IpcWait`/`IpcTake`, which keep their level/consume semantics.
    notified: bool,
    reply: ipc_wire::IpcWireMessage,
    #[cfg(feature = "p01-trace")]
    _p01_obligation: ObservedOperation,
}

impl Operation {
    fn owned_by(&self, task: &Task) -> bool {
        self.owner_cell == task.cell_id.0 && self.owner_generation == task.cell_generation
    }

    pub(crate) fn can_consume(&self, task: &Task) -> bool {
        self.owned_by(task) && (self.consumer_tid == task.id || self.owner_tid == task.id)
    }
}

#[cfg(feature = "p01-trace")]
#[derive(Debug)]
struct ObservedOperation(Option<crate::p01_resources::Obligation>);

#[cfg(feature = "p01-trace")]
impl Drop for ObservedOperation {
    fn drop(&mut self) {
        // This last field drops after reply storage, including a push rollback.
        // Terminal phase alone never releases the operation's retained storage.
        crate::p01_resources::finish(self.0.take(), 1);
    }
}

/// Task-local, kernel-charged storage. Tokens are global monotonic identifiers,
/// never slot indexes; reuse after take cannot alias a late service reply.
#[derive(Default)]
pub struct Operations(Vec<Operation>);

impl Operations {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    #[cfg(feature = "p01-trace")]
    fn observe_owner(&self, owner: crate::p01_measurement::Owner) {
        crate::memory::heap::p01_owner::runtime_vec(
            &self.0,
            owner,
            crate::memory::heap::p01_owner::OPERATIONS,
        );
    }
    pub fn find(&self, id: usize) -> Option<&Operation> {
        self.0.iter().find(|op| op.id == id)
    }
    fn find_mut(&mut self, id: usize) -> Option<&mut Operation> {
        self.0.iter_mut().find(|op| op.id == id)
    }
    fn remove(&mut self, id: usize) -> Option<Operation> {
        let pos = self.0.iter().position(|op| op.id == id)?;
        Some(self.0.swap_remove(pos))
    }
    fn push(&mut self, op: Operation) -> Result<(), Failure> {
        if self.0.len() == MAX_OPERATIONS {
            return Err(Failure::Busy);
        }
        if self.0.len() == self.0.capacity() {
            let previous = crate::sched::hart_local::current_cell_id();
            crate::sched::hart_local::set_current_cell_id(0);
            let reserved = self.0.try_reserve(1);
            crate::sched::hart_local::set_current_cell_id(previous);
            reserved.map_err(|_| Failure::Busy)?;
        }
        self.0.push(op);
        Ok(())
    }
    pub fn peer_died(&mut self, tid: usize, cell: u64, generation: u64) -> bool {
        let mut changed = false;
        for op in &mut self.0 {
            if op.peer_tid == tid
                && op.peer_cell == cell
                && op.peer_generation == generation
                && !matches!(op.phase, Phase::Terminal { .. })
            {
                op.phase = Phase::Terminal {
                    kind: api::syscall::ipc_status::PEER_GONE,
                    len: 0,
                };
                #[cfg(feature = "p01-trace")]
                crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
                changed = true;
            }
        }
        changed
    }
    fn has_terminal(&self) -> bool {
        self.0
            .iter()
            .any(|op| matches!(op.phase, Phase::Terminal { .. }))
    }
    fn notifiable(op: &Operation, waiter: Waiter) -> bool {
        !op.notified
            && matches!(op.phase, Phase::Terminal { .. })
            && (op.owner_cell, op.owner_generation) == (waiter.cell, waiter.generation)
            && (op.consumer_tid == waiter.tid || op.owner_tid == waiter.tid)
    }
}

/// Identity a notification is scoped to: always read from the waiting task's
/// own scheduler record, never from caller input.
#[derive(Clone, Copy)]
struct Waiter {
    tid: usize,
    cell: u64,
    generation: u64,
}

impl Waiter {
    fn of(task: &Task) -> Self {
        Self {
            tid: task.id,
            cell: task.cell_id.0,
            generation: task.cell_generation,
        }
    }
}

/// One consumed IPC completion notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IpcNotification {
    /// Exact operation token; the terminal kind/bytes stay behind `IpcTake`.
    pub token: usize,
    /// Operation records examined to choose it (bounded by `MAX_OPERATIONS`).
    pub scanned: usize,
}

/// Whether `task` owns a terminal operation that still owes its one-shot
/// notification. Caller holds `SCHEDULER`, the lock every terminal
/// transition also holds, so this check and a park decision are atomic.
pub(crate) fn has_pending_notification(task: &Task) -> bool {
    let waiter = Waiter::of(task);
    task.async_operations
        .0
        .iter()
        .any(|op| Operations::notifiable(op, waiter))
}

/// Consume the notification of the oldest submission (lowest token) among
/// `task`'s un-notified terminal operations. The operation itself stays
/// retained and charged until `take`. No allocation; one pass over at most
/// `MAX_OPERATIONS` records. Caller holds `SCHEDULER`.
pub(crate) fn take_notification(task: &mut Task) -> Option<IpcNotification> {
    let waiter = Waiter::of(task);
    let ops = &mut task.async_operations.0;
    let scanned = ops.len();
    let op = ops
        .iter_mut()
        .filter(|op| Operations::notifiable(op, waiter))
        .min_by_key(|op| op.id)?;
    op.notified = true;
    Some(IpcNotification {
        token: op.id,
        scanned,
    })
}

/// Re-arm a consumed notification whose record could not be copied out, so the
/// next wait reports it again. Only the waiter's own retained terminal can be
/// re-armed; a token taken meanwhile, or another incarnation's, is left alone.
/// Caller holds `SCHEDULER`.
pub(crate) fn restore_notification(task: &mut Task, token: usize) -> bool {
    let waiter = Waiter::of(task);
    let Some(op) = task.async_operations.find_mut(token) else {
        return false;
    };
    let restorable = op.notified
        && matches!(op.phase, Phase::Terminal { .. })
        && (op.owner_cell, op.owner_generation) == (waiter.cell, waiter.generation)
        && (op.consumer_tid == waiter.tid || op.owner_tid == waiter.tid);
    if restorable {
        op.notified = false;
    }
    restorable
}

impl Drop for Operations {
    fn drop(&mut self) {
        let rows = core::mem::take(&mut self.0);
        let previous = crate::sched::hart_local::current_cell_id();
        crate::sched::hart_local::set_current_cell_id(0);
        drop(rows);
        crate::sched::hart_local::set_current_cell_id(previous);
    }
}

fn live(task: &Task) -> bool {
    !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
}

/// Owner of every live operation token, and every task whose request context
/// was set to a token (architecture remediation Phase 10).
///
/// These two tables answer what the reply path used to find by scanning every
/// task. Owned by the [`Scheduler`] and only touched under `SCHEDULER`. A task
/// holds at most one token at a time, so `holders` keeps at most one row per
/// task; a row whose task has since moved on is stale and is checked against
/// `current_async_op` before use. Capacity for both is reserved when an
/// operation is accepted, so delivery and removal do not allocate.
pub struct AsyncIndex {
    /// `(token, owner tid)`, sorted by token.
    owners: Vec<(usize, usize)>,
    /// `(token, tid)`: the last token delivered as `tid`'s request context.
    holders: Vec<(usize, usize)>,
}

impl AsyncIndex {
    pub const fn new() -> Self {
        Self {
            owners: Vec::new(),
            holders: Vec::new(),
        }
    }

    /// Reserve room for one more operation, charged to the kernel like the
    /// operation record itself.
    fn reserve(&mut self) -> Result<(), Failure> {
        let previous = crate::sched::hart_local::current_cell_id();
        crate::sched::hart_local::set_current_cell_id(0);
        let reserved = self
            .owners
            .try_reserve(1)
            .and_then(|()| self.holders.try_reserve(self.owners.len() + 1));
        crate::sched::hart_local::set_current_cell_id(previous);
        reserved.map_err(|_| Failure::Busy)
    }

    fn insert(&mut self, token: usize, owner: usize) {
        let at = self.owners.partition_point(|&(t, _)| t < token);
        self.owners.insert(at, (token, owner));
    }

    fn owner(&self, token: usize) -> Option<usize> {
        let at = self.owners.binary_search_by_key(&token, |&(t, _)| t).ok()?;
        Some(self.owners[at].1)
    }

    fn remove(&mut self, token: usize) {
        if let Ok(at) = self.owners.binary_search_by_key(&token, |&(t, _)| t) {
            self.owners.remove(at);
        }
    }

    /// Record that `tid`'s request context now names `token` (0: none).
    ///
    /// Submit reserved a row per accepted operation, so this normally does not
    /// allocate; a token delivered twice (a wire that outlived its operation)
    /// can still need a row, which is then charged to the kernel, never to the
    /// receiving Cell.
    pub(crate) fn note_holder(&mut self, tid: usize, token: usize) {
        if let Some(at) = self.holders.iter().position(|&(_, t)| t == tid) {
            self.holders.swap_remove(at);
        }
        if token != 0 {
            if self.holders.len() == self.holders.capacity() {
                let previous = crate::sched::hart_local::current_cell_id();
                crate::sched::hart_local::set_current_cell_id(0);
                self.holders.reserve(1);
                crate::sched::hart_local::set_current_cell_id(previous);
            }
            self.holders.push((token, tid));
        }
    }
}

impl Default for AsyncIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Accept `op` into its owner's list and the index; fails without side
/// effect when either is full.
fn push_operation(sched: &mut Scheduler, owner: usize, op: Operation) -> Result<(), Failure> {
    sched.async_index.reserve()?;
    let token = op.id;
    sched
        .tasks
        .get_mut(&owner)
        .ok_or(Failure::Invalid)?
        .async_operations
        .push(op)?;
    sched.async_index.insert(token, owner);
    Ok(())
}

/// Remove `token` from `owner`'s list and the owner index.
fn remove_operation(sched: &mut Scheduler, owner: usize, token: usize) -> Option<Operation> {
    let op = sched
        .tasks
        .get_mut(&owner)?
        .async_operations
        .remove(token)?;
    sched.async_index.remove(token);
    Some(op)
}

/// Index upkeep when `task` leaves the task table: its operations die with it,
/// and it no longer holds a request context.
pub(crate) fn forget_task(sched: &mut Scheduler, task: &Task) {
    for op in &task.async_operations.0 {
        sched.async_index.remove(op.id);
    }
    sched.async_index.note_holder(task.id, 0);
}

/// The live owner of `token`, from the index. Test kernels cross-check it
/// against the scan it replaced.
fn owner_of(sched: &Scheduler, token: usize) -> Option<usize> {
    let owner = sched.async_index.owner(token).filter(|owner| {
        sched
            .tasks
            .get(owner)
            .is_some_and(|task| task.async_operations.find(token).is_some())
    });
    #[cfg(any(feature = "test-hooks", test))]
    assert_eq!(
        owner,
        sched
            .tasks
            .iter()
            .find_map(|(&id, task)| task.async_operations.find(token).map(|_| id)),
        "async owner index disagrees with the task scan for token {token}"
    );
    owner
}

/// Make the owner runnable after one of its operations turned terminal: an
/// `IpcWait` park, or a completion wait whose mask names the IPC source.
fn wake(sched: &mut Scheduler, owner: usize) {
    if let Some(task) = sched.tasks.get_mut(&owner) {
        let waiting = match task.state {
            TaskState::WaitIpc { .. } => true,
            TaskState::WaitCompletion { source, .. } => source & api::completion::source::IPC != 0,
            _ => false,
        };
        if waiting {
            task.state = TaskState::Ready;
            let priority = sched.push_ready(owner);
            sched.pend_preempt_if_needed(priority);
        }
    }
}

fn new_token() -> Result<usize, Failure> {
    NEXT_OPERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            (next <= isize::MAX as usize).then_some(next + 1)
        })
        .map_err(|_| Failure::Busy)
}

pub fn submit(owner: usize, peer: usize, ptr: usize, len: usize) -> Result<usize, Failure> {
    if len > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        return Err(Failure::Invalid);
    }
    #[cfg(feature = "p01-trace")]
    let heap_owner;
    let (view, binding, peer_binding) = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(Failure::PeerGone)?;
        let task = sched
            .tasks
            .get(&owner)
            .filter(|task| live(task))
            .ok_or(Failure::Invalid)?;
        let target = sched
            .tasks
            .get(&peer)
            .filter(|task| live(task))
            .ok_or(Failure::PeerGone)?;
        // Refuse a full or headroom-reserved queue before the request copy and
        // the 4 KiB reply buffer are allocated. The authoritative check repeats
        // in the commit hold below, where the push happens.
        if !peer_queue_admits(
            target,
            (task.cell_id.0, task.cell_generation),
            PEER_QUEUE_HEADROOM,
        ) {
            return Err(Failure::Busy);
        }
        #[cfg(feature = "p01-trace")]
        {
            heap_owner = crate::p01_resources::owner(task);
        }
        (
            TaskCopyView::of(task),
            (task.cell_id.0, task.cell_generation),
            (target.cell_id.0, target.cell_generation),
        )
    };
    let token = new_token()?;
    let header = ipc_wire::IpcWireHeader {
        sender_tid: owner,
        sender_cell_id: binding.0,
        sender_generation: binding.1,
        delivery_id: super::next_delivery_id(),
        async_op: token,
    };
    // Copy outside SCHEDULER; no peer or user pointer persists in a task slot.
    let wire = ipc_wire::IpcWireMessage::try_from_user(
        header,
        &view,
        ptr,
        len,
        #[cfg(feature = "p01-trace")]
        heap_owner,
    )
    .map_err(|_| Failure::Invalid)?;
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::ipc_bytes(len);
    let reply_header = ipc_wire::IpcWireHeader {
        sender_tid: peer,
        sender_cell_id: peer_binding.0,
        sender_generation: peer_binding.1,
        delivery_id: 0,
        async_op: token,
    };
    let reply =
        ipc_wire::IpcWireMessage::try_new(reply_header, &[0u8; ipc_wire::MAX_IPC_WIRE_PAYLOAD])
            .map_err(|_| Failure::Busy)?;
    #[cfg(feature = "p01-trace")]
    reply.observe_owner(heap_owner, crate::memory::heap::p01_owner::REPLY);
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::PeerGone)?;
    let caller = sched
        .tasks
        .get(&owner)
        .filter(|task| live(task) && (task.cell_id.0, task.cell_generation) == binding)
        .ok_or(Failure::Invalid)?;
    if caller.async_operations.len() >= MAX_OPERATIONS {
        return Err(Failure::Busy);
    }
    let receiver = sched
        .tasks
        .get(&peer)
        .filter(|task| live(task) && (task.cell_id.0, task.cell_generation) == peer_binding)
        .ok_or(Failure::PeerGone)?;
    if super::paused_target_rejects(sched, owner, peer) {
        return Err(Failure::Busy);
    }
    if !peer_queue_admits(receiver, binding, PEER_QUEUE_HEADROOM) {
        return Err(Failure::Busy);
    }
    #[cfg(feature = "p01-trace")]
    let p01_obligation = ObservedOperation(crate::p01_resources::operation_obligation(
        crate::p01_resources::owner(caller),
    ));
    #[cfg(feature = "p01-trace")]
    let _operations = crate::memory::heap::p01_owner::creation(
        Some(heap_owner),
        crate::memory::heap::p01_owner::OPERATIONS,
    );
    let pushed = push_operation(
        sched,
        owner,
        Operation {
            id: token,
            owner_cell: binding.0,
            owner_generation: binding.1,
            owner_tid: owner,
            consumer_tid: owner,
            peer_tid: peer,
            peer_cell: peer_binding.0,
            peer_generation: peer_binding.1,
            deadline: (super::system_ticks() as u64).saturating_add(OPERATION_TIMEOUT_TICKS),
            phase: Phase::Queued,
            notified: false,
            reply,
            #[cfg(feature = "p01-trace")]
            _p01_obligation: p01_obligation,
        },
    );
    #[cfg(feature = "p01-trace")]
    drop(_operations);
    #[cfg(feature = "p01-trace")]
    sched
        .tasks
        .get(&owner)
        .unwrap()
        .async_operations
        .observe_owner(heap_owner);
    pushed?;
    let target = sched.tasks.get_mut(&peer).ok_or(Failure::PeerGone)?;
    let recv_eligible =
        matches!(target.state, TaskState::Recv { mask, .. } if mask == 0 || mask == owner);
    if super::queue_wire_msg(target, wire, super::tcb::HOTSWAP_MSG_QUEUE_DEPTH).is_err() {
        remove_operation(sched, owner, token);
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
    if header.async_op == 0 {
        return;
    }
    let Some(provider) = sched.tasks.get(&peer).filter(|task| live(task)) else {
        return;
    };
    let peer_binding = (provider.cell_id.0, provider.cell_generation);
    let Some(owner) = sched
        .tasks
        .get_mut(&header.sender_tid)
        .filter(|task| live(task))
    else {
        return;
    };
    if owner.cell_id.0 != header.sender_cell_id || owner.cell_generation != header.sender_generation
    {
        return;
    }
    let owner_binding = (owner.cell_id.0, owner.cell_generation);
    let Some(op) = owner.async_operations.find_mut(header.async_op) else {
        return;
    };
    if (op.owner_cell, op.owner_generation) == owner_binding
        && (op.peer_tid, op.peer_cell, op.peer_generation) == (peer, peer_binding.0, peer_binding.1)
        && matches!(op.phase, Phase::Queued)
    {
        op.phase = Phase::Dispatched;
    }
}

fn terminal(
    sched: &mut Scheduler,
    owner_tid: usize,
    token: usize,
    provider: usize,
    exact_peer: bool,
    ptr: usize,
    len: usize,
) -> Result<(), Failure> {
    let Some(owner) = sched.tasks.get(&owner_tid).filter(|task| live(task)) else {
        return Err(Failure::PeerGone);
    };
    let Some(op) = owner.async_operations.find(token) else {
        return Err(Failure::Invalid);
    };
    let Some(peer) = sched.tasks.get(&provider).filter(|task| live(task)) else {
        return Err(Failure::PeerGone);
    };
    // Explicit replies may be delegated to a worker of the accepted provider's
    // cell incarnation. Only the implicit bridge requires the served TID too.
    // Authority always comes from scheduler identity, never caller input.
    if !op.owned_by(owner)
        || (exact_peer && op.peer_tid != provider)
        || op.peer_cell != peer.cell_id.0
        || op.peer_generation != peer.cell_generation
        || !matches!(op.phase, Phase::Dispatched)
    {
        return Err(Failure::Invalid);
    }
    let view = TaskCopyView::of(peer);
    let op = sched
        .tasks
        .get_mut(&owner_tid)
        .unwrap()
        .async_operations
        .find_mut(token)
        .unwrap();
    if len > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        op.phase = Phase::Terminal {
            kind: api::syscall::ipc_status::INDETERMINATE,
            len: 0,
        };
        #[cfg(feature = "p01-trace")]
        crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
        wake(sched, owner_tid);
        return Err(Failure::Invalid);
    }
    let copied = view.read_into(ptr, &mut op.reply.as_mut_slice()[..len]);
    #[cfg(feature = "bench-counters")]
    if copied.is_ok() {
        crate::bench_counters::ipc_bytes(len);
    }
    op.phase = Phase::Terminal {
        kind: if copied.is_ok() {
            api::syscall::ipc_status::REPLY
        } else {
            api::syscall::ipc_status::INDETERMINATE
        },
        len: if copied.is_ok() { len } else { 0 },
    };
    #[cfg(feature = "p01-trace")]
    crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
    wake(sched, owner_tid);
    copied.map_err(|_| Failure::Invalid)
}

/// Only a Send to the exact currently served caller can be a legacy reply.
/// Its token remains as a tombstone until another receive replaces the context:
/// duplicate/cancelled/late replies must not fall through to the raw mailbox.
/// Services sending same-destination events or deferring across receives must
/// capture the token and use explicit replies instead of this bridge.
pub fn reply_current(
    peer: usize,
    destination: usize,
    ptr: usize,
    len: usize,
) -> Option<Result<(), Failure>> {
    let mut guard = SCHEDULER.lock();
    reply_current_in_sched(guard.as_mut()?, peer, destination, ptr, len)
}

fn reply_current_in_sched(
    sched: &mut Scheduler,
    peer: usize,
    destination: usize,
    ptr: usize,
    len: usize,
) -> Option<Result<(), Failure>> {
    let provider = sched.tasks.get(&peer).filter(|task| live(task))?;
    let token = provider.current_async_op;
    if token == 0 || provider.caller.current != Some(destination) {
        return None;
    }
    let owner_binding = (provider.caller.cell_id, provider.caller.cell_generation);
    let owner_matches = sched
        .tasks
        .get(&destination)
        .is_some_and(|task| (task.cell_id.0, task.cell_generation) == owner_binding);
    if !owner_matches {
        return Some(Ok(()));
    }
    Some(
        match terminal(sched, destination, token, peer, true, ptr, len) {
            // Invalid tokens and terminal results are quarantined, never raw Send.
            Err(Failure::Invalid) => Ok(()),
            other => other,
        },
    )
}

pub fn current(peer: usize) -> usize {
    SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&peer))
        .filter(|task| live(task))
        .map_or(0, |task| task.current_async_op)
}

pub fn reply(peer: usize, token: usize, ptr: usize, len: usize) -> Result<(), Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    reply_in_sched(sched, peer, token, ptr, len)
}

fn reply_in_sched(
    sched: &mut Scheduler,
    peer: usize,
    token: usize,
    ptr: usize,
    len: usize,
) -> Result<(), Failure> {
    if token == 0 {
        return Err(Failure::Invalid);
    }
    let owner = owner_of(sched, token);
    let result = owner
        .ok_or(Failure::Invalid)
        .and_then(|owner| terminal(sched, owner, token, peer, false, ptr, len));
    // An explicit response ends this request even if cancellation/take already
    // retired its token. Otherwise later one-way events would be mistaken for
    // stale implicit replies. VFS's exact lease cleanup is handled by syscall.
    if let Some(binding) = sched
        .tasks
        .get(&peer)
        .filter(|task| live(task))
        .map(|task| (task.cell_id, task.cell_generation))
    {
        #[cfg(any(feature = "test-hooks", test))]
        for (&id, task) in sched.tasks.iter() {
            assert!(
                task.current_async_op != token || sched.async_index.holders.contains(&(token, id)),
                "task {id} holds async token {token} unrecorded by the index"
            );
        }
        // Visit the recorded holders of `token`; drop rows that are cleared
        // here or no longer hold it, keep the rest for a later reply.
        let holders = &mut sched.async_index.holders;
        let mut at = 0;
        while at < holders.len() {
            let (held, tid) = holders[at];
            if held != token {
                at += 1;
                continue;
            }
            let keep = match sched.tasks.get_mut(&tid) {
                Some(task) if task.current_async_op == token => {
                    let clear = (task.cell_id, task.cell_generation) == binding
                        && !crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize);
                    if clear {
                        task.clear_current_caller_context();
                    }
                    !clear
                }
                _ => false,
            };
            if keep {
                at += 1;
            } else {
                holders.swap_remove(at);
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
    let task = sched
        .tasks
        .get(&owner)
        .filter(|task| live(task))
        .ok_or(Failure::Invalid)?;
    let op = task.async_operations.find(token).ok_or(Failure::Invalid)?;
    if !op.owned_by(task) {
        return Err(Failure::Invalid);
    }
    let queued = matches!(op.phase, Phase::Queued);
    let peer = op.peer_tid;
    if queued {
        // No receiver snapshot has begun; remove only the matching token.
        if let Some(receiver) = sched.tasks.get_mut(&peer) {
            if let Some(pos) = receiver.pending_msgs.iter().position(|msg| {
                msg.wire_header()
                    .is_some_and(|header| header.async_op == token)
            }) {
                receiver.pending_msgs.remove(pos);
            }
        }
    }
    let op = sched
        .tasks
        .get_mut(&owner)
        .unwrap()
        .async_operations
        .find_mut(token)
        .unwrap();
    if !matches!(op.phase, Phase::Terminal { .. }) {
        op.phase = Phase::Terminal {
            kind: if queued {
                api::syscall::ipc_status::CANCELLED
            } else {
                api::syscall::ipc_status::INDETERMINATE
            },
            len: 0,
        };
        #[cfg(feature = "p01-trace")]
        crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
        wake(sched, owner);
    }
    Ok(())
}

pub fn take(
    consumer: usize,
    token: usize,
    reply_ptr: usize,
    reply_len: usize,
    status_ptr: usize,
) -> Result<bool, Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    let caller = sched
        .tasks
        .get(&consumer)
        .filter(|task| live(task))
        .ok_or(Failure::Invalid)?;
    let caller_cell = (caller.cell_id.0, caller.cell_generation);

    let owner_tid = owner_of(sched, token)
        .filter(|owner| {
            let task = &sched.tasks[owner];
            (task.cell_id.0, task.cell_generation) == caller_cell
        })
        .ok_or(Failure::Invalid)?;
    let op_ref = sched.tasks[&owner_tid]
        .async_operations
        .find(token)
        .unwrap();
    if !op_ref.can_consume(caller) {
        return Err(Failure::Invalid);
    }
    let Phase::Terminal { kind, len } = &op_ref.phase else {
        return Ok(false);
    };
    let data = &op_ref.reply.as_slice()[..*len];
    if data.len() > reply_len {
        return Err(Failure::TooSmall);
    }
    let view = TaskCopyView::of(caller);
    view.validate_writable(status_ptr, api::syscall::IPC_STATUS_LEN)
        .map_err(|_| Failure::Invalid)?;
    if !data.is_empty() {
        view.validate_writable(reply_ptr, data.len())
            .map_err(|_| Failure::Invalid)?;
        view.write_bytes(reply_ptr, data)
            .map_err(|_| Failure::Invalid)?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::ipc_bytes(data.len());
    }
    let mut status = [0u8; api::syscall::IPC_STATUS_LEN];
    status[..4].copy_from_slice(&api::syscall::IPC_STATUS_VERSION.to_le_bytes());
    status[4..8].copy_from_slice(&kind.to_le_bytes());
    status[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    view.write_bytes(status_ptr, &status)
        .map_err(|_| Failure::Invalid)?;
    remove_operation(sched, owner_tid, token);
    Ok(true)
}

pub fn transfer_consumption(
    caller_tid: usize,
    token: usize,
    target_tid: usize,
) -> Result<(), Failure> {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(Failure::Invalid)?;
    let caller = sched
        .tasks
        .get(&caller_tid)
        .filter(|t| live(t))
        .ok_or(Failure::Invalid)?;
    let target = sched
        .tasks
        .get(&target_tid)
        .filter(|t| live(t))
        .ok_or(Failure::Invalid)?;
    if (caller.cell_id, caller.cell_generation) != (target.cell_id, target.cell_generation) {
        return Err(Failure::Invalid);
    }
    let caller_cell = (caller.cell_id.0, caller.cell_generation);
    let owner = owner_of(sched, token)
        .filter(|owner| {
            let task = &sched.tasks[owner];
            (task.cell_id.0, task.cell_generation) == caller_cell
        })
        .ok_or(Failure::Invalid)?;
    let op = sched
        .tasks
        .get_mut(&owner)
        .unwrap()
        .async_operations
        .find_mut(token)
        .unwrap();
    if op.consumer_tid != caller_tid && op.owner_tid != caller_tid {
        return Err(Failure::Invalid);
    }
    op.consumer_tid = target_tid;
    Ok(())
}

/// Drop or indeterminate all operations owned by an exiting thread.
/// If take won before exit, the operation was already removed, ensuring
/// single-winner serialization against take.
pub fn on_owner_exits(sched: &mut Scheduler, owner_tid: usize) {
    let to_cancel: alloc::vec::Vec<(usize, usize, bool)> = match sched.tasks.get(&owner_tid) {
        Some(task) => task
            .async_operations
            .0
            .iter()
            .filter_map(|op| {
                if op.owner_tid == owner_tid {
                    match op.phase {
                        Phase::Queued => Some((op.id, op.peer_tid, true)),
                        Phase::Dispatched => Some((op.id, op.peer_tid, false)),
                        Phase::Terminal { .. } => None,
                    }
                } else {
                    None
                }
            })
            .collect(),
        None => return,
    };

    for &(token, peer, queued) in &to_cancel {
        if queued {
            if let Some(receiver) = sched.tasks.get_mut(&peer) {
                if let Some(pos) = receiver.pending_msgs.iter().position(|msg| {
                    msg.wire_header()
                        .is_some_and(|header| header.async_op == token)
                }) {
                    receiver.pending_msgs.remove(pos);
                }
            }
        }
    }

    if let Some(task) = sched.tasks.get_mut(&owner_tid) {
        for (token, _, queued) in to_cancel {
            if let Some(op) = task.async_operations.find_mut(token) {
                op.phase = Phase::Terminal {
                    kind: if queued {
                        api::syscall::ipc_status::CANCELLED
                    } else {
                        api::syscall::ipc_status::INDETERMINATE
                    },
                    len: 0,
                };
            }
        }
    }
}

pub fn wait(owner: usize, ticks: u64) -> Result<bool, Failure> {
    let deadline = (ticks != 0).then(|| (super::system_ticks() as u64).saturating_add(ticks));
    {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(Failure::Invalid)?;
        let task = sched
            .tasks
            .get_mut(&owner)
            .filter(|task| live(task))
            .ok_or(Failure::Invalid)?;
        if task.async_operations.has_terminal() {
            return Ok(true);
        }
        super::arm_ipc_block_handoff(owner);
        task.state = TaskState::WaitIpc { deadline };
    }
    super::yield_cpu();
    let guard = SCHEDULER.lock();
    Ok(guard
        .as_ref()
        .and_then(|s| s.tasks.get(&owner))
        .is_some_and(|task| task.async_operations.has_terminal()))
}

/// Peer death is an infallible terminal transition; storage was reserved on submit.
pub fn peer_died(sched: &mut Scheduler, peer: usize, cell: u64, generation: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) {
            Some(next) => next,
            None => break,
        };
        if sched
            .tasks
            .get_mut(&owner)
            .is_some_and(|task| task.async_operations.peer_died(peer, cell, generation))
        {
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
            let queued = task.pending_msgs.as_slice()[index]
                .wire_header()
                .is_some_and(|header| {
                    header.async_op != 0
                        && header.sender_tid == owner
                        && header.sender_cell_id == cell
                        && header.sender_generation == generation
                });
            if queued {
                task.pending_msgs.remove(index);
            } else {
                index += 1;
            }
        }
    }
}

/// Complete queued requests tied to a provider being replaced. Called only
/// after the successful hot-swap commit, under SCHEDULER. The replacement
/// must not receive these wires because it is a different generation.
pub(crate) fn cutover_peer(sched: &mut Scheduler, peer: usize, cell: u64, generation: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) {
            Some(next) => next,
            None => break,
        };
        let changed = if let Some(task) = sched.tasks.get_mut(&owner) {
            let mut changed = false;
            for op in &mut task.async_operations.0 {
                if op.peer_tid == peer
                    && op.peer_cell == cell
                    && op.peer_generation == generation
                    && matches!(op.phase, Phase::Queued)
                {
                    op.phase = Phase::Terminal {
                        kind: api::syscall::ipc_status::PEER_GONE,
                        len: 0,
                    };
                    #[cfg(feature = "p01-trace")]
                    crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
                    changed = true;
                }
            }
            changed
        } else {
            false
        };
        if changed {
            wake(sched, owner);
        }
    }
}

/// Expire accepted work without allocating in the scheduler's timer sweep.
/// An undispatched request is removed from its peer mailbox before reporting
/// PreDispatchTimeout. A dispatched request is conservatively indeterminate.
pub fn expire(sched: &mut Scheduler, now: u64) {
    let mut after = 0usize;
    while let Some((&owner, _)) = sched.tasks.range(after..).next() {
        after = match owner.checked_add(1) {
            Some(next) => next,
            None => break,
        };
        for index in 0..MAX_OPERATIONS {
            let expired = {
                let Some(task) = sched.tasks.get_mut(&owner) else {
                    break;
                };
                let Some(op) = task.async_operations.0.get_mut(index) else {
                    break;
                };
                if now < op.deadline || matches!(op.phase, Phase::Terminal { .. }) {
                    None
                } else {
                    let queued = matches!(op.phase, Phase::Queued);
                    op.phase = Phase::Terminal {
                        kind: if queued {
                            api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT
                        } else {
                            api::syscall::ipc_status::INDETERMINATE
                        },
                        len: 0,
                    };
                    #[cfg(feature = "p01-trace")]
                    crate::p01_resources::reservation_terminal(&mut op._p01_obligation.0);
                    Some((op.id, op.peer_tid, queued))
                }
            };
            if let Some((token, peer, queued)) = expired {
                if queued {
                    if let Some(target) = sched.tasks.get_mut(&peer) {
                        if let Some(pos) = target.pending_msgs.iter().position(|msg| {
                            msg.wire_header()
                                .is_some_and(|header| header.async_op == token)
                        }) {
                            target.pending_msgs.remove(pos);
                        }
                    }
                }
                wake(sched, owner);
            }
        }
    }
}

#[cfg(not(cellos_posture = "release"))]
/// Boot self-test seam for the multi-source wait: record one accepted,
/// already-dispatched operation from `owner` to `peer` without a user copy.
///
/// Synthetic self-test tasks have no address space, so they cannot go through
/// [`submit`]. Identity is still read from the scheduler records exactly as
/// `submit` reads it, and the token comes from the same global counter, so the
/// operation is indistinguishable from a real one to every terminal path.
/// Caller holds `SCHEDULER`.
pub(super) fn selftest_accept_dispatched(
    sched: &mut Scheduler,
    owner: usize,
    peer: usize,
) -> Option<usize> {
    let (peer_cell, peer_generation) = sched
        .tasks
        .get(&peer)
        .map(|task| (task.cell_id.0, task.cell_generation))?;
    let token = new_token().ok()?;
    let reply = ipc_wire::IpcWireMessage::try_new(
        ipc_wire::IpcWireHeader {
            sender_tid: peer,
            sender_cell_id: peer_cell,
            sender_generation: peer_generation,
            delivery_id: 0,
            async_op: token,
        },
        &[],
    )
    .ok()?;
    let task = sched.tasks.get(&owner)?;
    let (owner_cell, owner_generation) = (task.cell_id.0, task.cell_generation);
    push_operation(
        sched,
        owner,
        Operation {
            id: token,
            owner_cell,
            owner_generation,
            owner_tid: owner,
            consumer_tid: owner,
            peer_tid: peer,
            peer_cell,
            peer_generation,
            deadline: u64::MAX,
            phase: Phase::Dispatched,
            notified: false,
            reply,
            #[cfg(feature = "p01-trace")]
            _p01_obligation: ObservedOperation(None),
        },
    )
    .ok()?;
    Some(token)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_reply() -> ipc_wire::IpcWireMessage {
        ipc_wire::IpcWireMessage::try_new(
            ipc_wire::IpcWireHeader {
                sender_tid: 5,
                sender_cell_id: 2,
                sender_generation: 7,
                delivery_id: 0,
                async_op: 1,
            },
            &[],
        )
        .unwrap()
    }
    #[test]
    fn bounded_tokens_and_terminal_retention() {
        let mut sched = fixture();
        for id in 1..=MAX_OPERATIONS {
            add_operation(&mut sched, OWNER, PROVIDER, id, Phase::Dispatched);
        }
        let overflow = || Operation {
            id: MAX_OPERATIONS + 1,
            owner_cell: 1_002,
            owner_generation: 3,
            owner_tid: OWNER,
            consumer_tid: OWNER,
            peer_tid: PROVIDER,
            peer_cell: 1_003,
            peer_generation: 7,
            deadline: 10,
            phase: Phase::Queued,
            notified: false,
            reply: test_reply(),
            #[cfg(feature = "p01-trace")]
            _p01_obligation: ObservedOperation(None),
        };
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert!(matches!(
            sched
                .tasks
                .get_mut(&OWNER)
                .unwrap()
                .async_operations
                .push(overflow()),
            Err(Failure::Busy)
        ));
        peer_died(&mut sched, PROVIDER, 1_003, 8);
        peer_died(&mut sched, WORKER, 1_003, 7);
        assert!(
            !sched.tasks[&OWNER].async_operations.has_terminal(),
            "only the accepted provider incarnation can terminate these operations"
        );
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert!(sched.tasks[&OWNER]
            .async_operations
            .0
            .iter()
            .all(|op| matches!(
                op.phase,
                Phase::Terminal {
                    kind: api::syscall::ipc_status::PEER_GONE,
                    len: 0
                }
            )));
        expire(&mut sched, u64::MAX);
        cancel_in_sched(&mut sched, OWNER, 1).unwrap();
        assert!(
            matches!(
                sched
                    .tasks
                    .get_mut(&OWNER)
                    .unwrap()
                    .async_operations
                    .push(overflow()),
                Err(Failure::Busy)
            ),
            "terminal results still charge the full quota"
        );
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert!(sched.tasks[&PROVIDER].pending_msgs.is_empty());
        // Exercise the storage release performed only after a successful take.
        assert!(matches!(
            sched
                .tasks
                .get_mut(&OWNER)
                .unwrap()
                .async_operations
                .remove(1)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PEER_GONE,
                len: 0
            }
        ));
        add_operation(
            &mut sched,
            OWNER,
            PROVIDER,
            MAX_OPERATIONS + 1,
            Phase::Dispatched,
        );
        assert_eq!(sched.tasks[&OWNER].async_operations.len(), MAX_OPERATIONS);
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 1, 0, 0),
            Err(Failure::Invalid)
        );
        assert!(
            matches!(
                sched.tasks[&OWNER]
                    .async_operations
                    .find(MAX_OPERATIONS + 1)
                    .unwrap()
                    .phase,
                Phase::Dispatched
            ),
            "a retired token cannot alias the reused reservation"
        );
    }
    #[test]
    fn deadline_retains_terminal_results_and_removes_only_queued_work() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Queued);
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Queued);
        add_operation(&mut sched, OWNER, PROVIDER, 13, Phase::Queued);
        sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .find_mut(13)
            .unwrap()
            .deadline = 11;
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER,
            sender_cell_id: 1_002,
            sender_generation: 3,
            delivery_id: 44,
            async_op: 11,
        };
        for (token, payload) in [
            (11, b"due".as_slice()),
            (0, b"raw".as_slice()),
            (13, b"later".as_slice()),
        ] {
            super::super::queue_wire_msg(
                sched.tasks.get_mut(&PROVIDER).unwrap(),
                ipc_wire::IpcWireMessage::try_new(
                    ipc_wire::IpcWireHeader {
                        async_op: token,
                        ..header
                    },
                    payload,
                )
                .unwrap(),
                64,
            )
            .unwrap();
        }
        dispatch(
            &mut sched,
            PROVIDER,
            ipc_wire::IpcWireHeader {
                async_op: 12,
                ..header
            },
        );
        serve(&mut sched, PROVIDER, OWNER, 12);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        expire(&mut sched, 9);
        assert!(matches!(
            sched.tasks[&OWNER].state,
            TaskState::WaitIpc { .. }
        ));
        assert!(!sched.tasks[&OWNER].async_operations.has_terminal());
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Queued
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Dispatched
        ));
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 3);
        expire(&mut sched, 10);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        crate::sched::hart_local::ready::remove_from_all(OWNER);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT,
                len: 0
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::INDETERMINATE,
                len: 0
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(13).unwrap().phase,
            Phase::Queued
        ));
        let remaining = sched.tasks[&PROVIDER].pending_msgs.as_slice();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].wire_header().unwrap().async_op, 0);
        assert_eq!(remaining[1].wire_header().unwrap().async_op, 13);
        assert_eq!(sched.tasks[&PROVIDER].caller.current, Some(OWNER));
        assert_eq!(
            sched.tasks[&PROVIDER].current_async_op, 12,
            "expiry cannot release a dispatched service context"
        );
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 11, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 12, 0, 0),
            Err(Failure::Invalid)
        );
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        cancel_in_sched(&mut sched, OWNER, 12).unwrap();
        expire(&mut sched, 11);
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        expire(&mut sched, u64::MAX);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT,
                len: 0
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::INDETERMINATE,
                len: 0
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(13).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT,
                len: 0
            }
        ));
        assert_eq!(
            sched.tasks[&OWNER].async_operations.len(),
            3,
            "expiry never consumes a result"
        );
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 1);
        assert_eq!(
            sched.tasks[&PROVIDER].pending_msgs.as_slice()[0]
                .wire_header()
                .unwrap()
                .async_op,
            0
        );
        assert!(
            sched.tasks[&OWNER].pending_msgs.is_empty(),
            "late replies cannot become raw events"
        );
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
            (OWNER, 1_002, 3),
            (PROVIDER, 1_003, 7),
            (WORKER, 1_003, 7),
            (FOREIGN_GENERATION, 1_003, 8),
            (FOREIGN_CELL, 1_004, 7),
        ] {
            let mut task = Box::new(Task::new(tid, CellId(cell), "async-fixture", Vec::new()));
            task.cell_generation = generation;
            sched.tasks.insert(tid, task);
        }
        sched
    }

    fn operation(
        sched: &Scheduler,
        owner: usize,
        peer: usize,
        id: usize,
        phase: Phase,
    ) -> Operation {
        let provider = &sched.tasks[&peer];
        let task = &sched.tasks[&owner];
        Operation {
            id,
            owner_cell: task.cell_id.0,
            owner_generation: task.cell_generation,
            owner_tid: owner,
            consumer_tid: owner,
            peer_tid: peer,
            peer_cell: provider.cell_id.0,
            peer_generation: provider.cell_generation,
            deadline: 10,
            phase,
            notified: false,
            reply: test_reply(),
            #[cfg(feature = "p01-trace")]
            _p01_obligation: ObservedOperation(None),
        }
    }

    fn add_operation(sched: &mut Scheduler, owner: usize, peer: usize, id: usize, phase: Phase) {
        let op = operation(sched, owner, peer, id, phase);
        push_operation(sched, owner, op).unwrap();
    }

    fn serve(sched: &mut Scheduler, peer: usize, owner: usize, token: usize) {
        let task = sched.tasks.get(&owner).unwrap();
        let (cell, generation) = (task.cell_id.0, task.cell_generation);
        let held = sched
            .tasks
            .get_mut(&peer)
            .unwrap()
            .set_received_request_context(owner, cell, generation, token);
        if let Some(token) = held {
            sched.async_index.note_holder(peer, token);
        }
    }

    #[test]
    fn immediate_bridge_settles_only_served_token_and_preserves_reply() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, WORKER, 0, 0),
            None
        );
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::REPLY,
                ..
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Dispatched
        ));
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        expire(&mut sched, 100);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::REPLY,
                ..
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PEER_GONE,
                ..
            }
        ));
        assert!(sched.tasks[&OWNER].pending_msgs.is_empty());
    }

    #[test]
    fn cancelled_and_consumed_late_reply_cannot_complete_successor() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::INDETERMINATE,
                ..
            }
        ));
        sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .remove(11)
            .unwrap();
        add_operation(&mut sched, OWNER, PROVIDER, 12, Phase::Dispatched);
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 11, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            None,
            "an explicit retired-token reply ends the implicit context"
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Dispatched
        ));
        assert!(sched.tasks[&OWNER].pending_msgs.is_empty());
        serve(&mut sched, PROVIDER, OWNER, 12);
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
    }

    #[test]
    fn dispatch_and_explicit_worker_reply_check_both_incarnations() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Queued);
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 99, 0, 0),
            Err(Failure::Invalid),
            "a guessed token never authorizes a provider reply"
        );
        assert_eq!(
            reply_in_sched(&mut sched, WORKER, 11, 0, 0),
            Err(Failure::Invalid),
            "a genuine queued token cannot reply before dispatch"
        );
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER,
            sender_cell_id: 1_002,
            sender_generation: 3,
            delivery_id: 1,
            async_op: 11,
        };
        sched.tasks.get_mut(&PROVIDER).unwrap().cell_generation = 8;
        dispatch(&mut sched, PROVIDER, header);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Queued
        ));
        sched.tasks.get_mut(&PROVIDER).unwrap().cell_generation = 7;
        dispatch(
            &mut sched,
            PROVIDER,
            ipc_wire::IpcWireHeader {
                sender_generation: 4,
                ..header
            },
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Queued
        ));
        dispatch(&mut sched, PROVIDER, header);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Dispatched
        ));
        serve(&mut sched, PROVIDER, OWNER, 11);
        assert_eq!(
            reply_in_sched(&mut sched, FOREIGN_GENERATION, 11, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(
            reply_in_sched(&mut sched, FOREIGN_CELL, 11, 0, 0),
            Err(Failure::Invalid)
        );
        serve(&mut sched, WORKER, OWNER, 11);
        assert_eq!(
            reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert!(
            matches!(
                sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
                Phase::Dispatched
            ),
            "the implicit bridge cannot use sibling-worker authority"
        );
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation = 4;
        assert_eq!(
            reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0),
            Some(Ok(()))
        );
        assert_eq!(
            reply_in_sched(&mut sched, WORKER, 11, 0, 0),
            Err(Failure::Invalid)
        );
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation = 3;
        assert_eq!(reply_in_sched(&mut sched, WORKER, 11, 0, 0), Ok(()));
        assert_eq!(
            sched.tasks[&PROVIDER].current_async_op, 0,
            "worker completion ends the original receiver context"
        );
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            None,
            "a subsequent one-way event must not be quarantined as duplicate RPC"
        );
        assert_eq!(
            reply_in_sched(&mut sched, WORKER, 11, 0, 0),
            Err(Failure::Invalid)
        );
    }

    #[test]
    fn outgoing_operation_terminal_does_not_replace_inbound_context() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 11);
        add_operation(&mut sched, PROVIDER, WORKER, 21, Phase::Dispatched);
        cancel_in_sched(&mut sched, PROVIDER, 21).unwrap();
        assert_eq!(sched.tasks[&PROVIDER].caller.current, Some(OWNER));
        assert_eq!(sched.tasks[&PROVIDER].current_async_op, 11);
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
    }

    #[test]
    fn peer_death_publishes_terminal_before_waking_only_rpc_waiter() {
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 11, Phase::Dispatched);
        add_operation(&mut sched, WORKER, PROVIDER, 12, Phase::Queued);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        sched.tasks.get_mut(&WORKER).unwrap().state = TaskState::Recv {
            mask: PROVIDER,
            buf_ptr: 0,
            buf_len: 0,
            deadline: None,
        };
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        assert!(sched.tasks[&OWNER].async_operations.has_terminal());
        assert!(matches!(
            sched.tasks[&WORKER].state,
            TaskState::Recv { mask: PROVIDER, .. }
        ));
        assert!(sched.tasks[&WORKER].async_operations.has_terminal());
        crate::sched::hart_local::ready::remove_from_all(OWNER);
    }

    #[test]
    fn queued_cancel_removes_only_exact_request_and_is_terminal() {
        let mut sched = fixture();
        for token in [11, 12] {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Queued);
            let header = ipc_wire::IpcWireHeader {
                sender_tid: OWNER,
                sender_cell_id: 1_002,
                sender_generation: 3,
                delivery_id: token as u64,
                async_op: token,
            };
            super::super::queue_wire_msg(
                sched.tasks.get_mut(&PROVIDER).unwrap(),
                ipc_wire::IpcWireMessage::try_new(header, b"queued").unwrap(),
                64,
            )
            .unwrap();
        }
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        cancel_in_sched(&mut sched, OWNER, 11).unwrap();
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::CANCELLED,
                ..
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Queued
        ));
        assert_eq!(sched.tasks[&PROVIDER].pending_msgs.len(), 1);
        assert_eq!(
            sched.tasks[&PROVIDER].pending_msgs.as_slice()[0]
                .wire_header()
                .unwrap()
                .async_op,
            12
        );
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 11, 0, 0),
            Err(Failure::Invalid)
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::CANCELLED,
                ..
            }
        ));
        expire(&mut sched, 9);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::CANCELLED,
                len: 0
            }
        ));
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Queued
        ));
        expire(&mut sched, 10);
        assert!(
            matches!(
                sched.tasks[&OWNER].async_operations.find(11).unwrap().phase,
                Phase::Terminal {
                    kind: api::syscall::ipc_status::CANCELLED,
                    len: 0
                }
            ),
            "the exact deadline cannot replace an already-terminal cancellation"
        );
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT,
                len: 0
            }
        ));
        assert!(sched.tasks[&PROVIDER].pending_msgs.is_empty());
    }

    fn queue_op_wire(sched: &mut Scheduler, owner: usize, peer: usize, token: usize) {
        let task = &sched.tasks[&owner];
        let header = ipc_wire::IpcWireHeader {
            sender_tid: owner,
            sender_cell_id: task.cell_id.0,
            sender_generation: task.cell_generation,
            delivery_id: token as u64,
            async_op: token,
        };
        super::super::queue_wire_msg(
            sched.tasks.get_mut(&peer).unwrap(),
            ipc_wire::IpcWireMessage::try_new(header, b"op").unwrap(),
            super::super::tcb::HOTSWAP_MSG_QUEUE_DEPTH,
        )
        .unwrap();
    }

    /// Phase 03e G1. A masked receive snapshots the first record from its
    /// mask, dispatches it, copies out without `SCHEDULER`, then commits the
    /// removal. Another caller's queued request *ahead* of it may be cancelled
    /// in between; the commit must still remove exactly the delivered record.
    #[test]
    fn masked_receive_commit_removes_the_delivered_record_after_a_concurrent_cancel() {
        let mut sched = fixture();
        add_operation(&mut sched, FOREIGN_CELL, PROVIDER, 21, Phase::Queued);
        queue_op_wire(&mut sched, FOREIGN_CELL, PROVIDER, 21);
        for token in [11, 12] {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Queued);
            queue_op_wire(&mut sched, OWNER, PROVIDER, token);
        }

        // Snapshot hold: `Recv { mask: OWNER }` picks the first OWNER record.
        let mailbox = &sched.tasks[&PROVIDER].pending_msgs;
        let pos = mailbox.iter().position(|m| m.sender_tid == OWNER).unwrap();
        let header = mailbox.as_slice()[pos].wire_header().unwrap();
        assert_eq!((pos, header.async_op), (1, 11));
        dispatch(&mut sched, PROVIDER, header);

        // Between the holds, the foreign caller cancels its queued request.
        cancel_in_sched(&mut sched, FOREIGN_CELL, 21).unwrap();

        // Commit hold.
        let removed = sched
            .tasks
            .get_mut(&PROVIDER)
            .unwrap()
            .pending_msgs
            .remove_delivered(pos, OWNER, Some(header.delivery_id))
            .and_then(|m| m.wire_header())
            .map(|h| h.async_op);
        assert_eq!(
            removed,
            Some(11),
            "the commit removed a record it never delivered"
        );

        // The next masked receive must deliver 12, never 11 a second time.
        let next = sched.tasks[&PROVIDER]
            .pending_msgs
            .iter()
            .find(|m| m.sender_tid == OWNER)
            .and_then(|m| m.wire_header())
            .map(|h| h.async_op);
        assert_eq!(next, Some(12), "operation 11 would be delivered twice");
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(12).unwrap().phase,
            Phase::Queued
        ));
    }

    /// Records without a wire (interrupt-produced inline data) keep the
    /// position rule; a wire whose delivery id is gone is never guessed at.
    #[test]
    fn remove_delivered_keeps_the_inline_rule_and_never_guesses_a_wire() {
        let mut sched = fixture();
        let task = sched.tasks.get_mut(&PROVIDER).unwrap();
        super::super::queue_pending_msg(task, OWNER, b"inline", 64).unwrap();
        assert!(task
            .pending_msgs
            .remove_delivered(0, WORKER, None)
            .is_none());
        assert!(task.pending_msgs.remove_delivered(0, OWNER, None).is_some());
        assert!(task.pending_msgs.is_empty());

        queue_op_wire(&mut sched, OWNER, PROVIDER, 31);
        queue_op_wire(&mut sched, OWNER, PROVIDER, 32);
        let mailbox = &mut sched.tasks.get_mut(&PROVIDER).unwrap().pending_msgs;
        // Delivery id 30 was removed elsewhere (e.g. its owner died): no removal.
        assert!(mailbox.remove_delivered(0, OWNER, Some(30)).is_none());
        assert_eq!(mailbox.len(), 2);
        let removed = mailbox.remove_delivered(0, OWNER, Some(32)).unwrap();
        assert_eq!(removed.wire_header().unwrap().async_op, 32);
        assert_eq!(mailbox.as_slice()[0].wire_header().unwrap().async_op, 31);
    }

    /// Phase 03e guard: a receive arm that passed `None` for a delivered wire
    /// would leave it queued for a second delivery; debug builds stop there.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "a wire was committed without its delivery id")]
    fn a_wire_committed_without_its_delivery_id_is_caught() {
        let mut sched = fixture();
        queue_op_wire(&mut sched, OWNER, PROVIDER, 41);
        let mailbox = &mut sched.tasks.get_mut(&PROVIDER).unwrap().pending_msgs;
        let _ = mailbox.remove_delivered(0, OWNER, None);
    }

    fn terminal_kind(sched: &Scheduler, owner: usize, token: usize) -> Option<u32> {
        match sched.tasks[&owner].async_operations.find(token)?.phase {
            Phase::Terminal { kind, .. } => Some(kind),
            _ => None,
        }
    }

    fn wire_queued(sched: &Scheduler, peer: usize, token: usize) -> bool {
        sched.tasks[&peer]
            .pending_msgs
            .iter()
            .any(|m| m.wire_header().is_some_and(|h| h.async_op == token))
    }

    /// Phase 03e A2. Every terminal producer, applied before and after the
    /// receive snapshot (`dispatch`). A definite "not executed" outcome exists
    /// only for a request no receive ever snapshotted, and its wire is gone,
    /// so no later receive can deliver it.
    #[test]
    fn definite_not_executed_outcomes_only_for_never_snapshotted_requests() {
        use api::syscall::ipc_status::{CANCELLED, INDETERMINATE, PEER_GONE, PRE_DISPATCH_TIMEOUT};
        #[derive(Clone, Copy, Debug)]
        enum Producer {
            Cancel,
            Expire,
            OwnerExit,
            PeerDied,
            Cutover,
        }
        for producer in [
            Producer::Cancel,
            Producer::Expire,
            Producer::OwnerExit,
            Producer::PeerDied,
            Producer::Cutover,
        ] {
            for snapshotted in [false, true] {
                let mut sched = fixture();
                add_operation(&mut sched, OWNER, PROVIDER, 51, Phase::Queued);
                queue_op_wire(&mut sched, OWNER, PROVIDER, 51);
                if snapshotted {
                    let header = sched.tasks[&PROVIDER].pending_msgs.as_slice()[0]
                        .wire_header()
                        .unwrap();
                    dispatch(&mut sched, PROVIDER, header);
                }
                match producer {
                    Producer::Cancel => cancel_in_sched(&mut sched, OWNER, 51).unwrap(),
                    Producer::Expire => expire(&mut sched, 10),
                    Producer::OwnerExit => on_owner_exits(&mut sched, OWNER),
                    Producer::PeerDied => peer_died(&mut sched, PROVIDER, 1_003, 7),
                    Producer::Cutover => cutover_peer(&mut sched, PROVIDER, 1_003, 7),
                }
                let kind = terminal_kind(&sched, OWNER, 51);
                let expected = match (producer, snapshotted) {
                    (Producer::Cancel | Producer::OwnerExit, false) => Some(CANCELLED),
                    (Producer::Expire, false) => Some(PRE_DISPATCH_TIMEOUT),
                    (Producer::Cancel | Producer::Expire | Producer::OwnerExit, true) => {
                        Some(INDETERMINATE)
                    }
                    (Producer::PeerDied, _) | (Producer::Cutover, false) => Some(PEER_GONE),
                    // Stays dispatched until the old provider exits (`peer_died`).
                    (Producer::Cutover, true) => None,
                };
                assert_eq!(kind, expected, "{producer:?} snapshotted={snapshotted}");
                if matches!(kind, Some(CANCELLED | PRE_DISPATCH_TIMEOUT)) {
                    assert!(!snapshotted);
                    assert!(
                        !wire_queued(&sched, PROVIDER, 51),
                        "{producer:?}: a definite outcome left a deliverable wire"
                    );
                }
                // A later snapshot of anything still queued never revives a terminal.
                if let Some(header) = sched.tasks[&PROVIDER]
                    .pending_msgs
                    .iter()
                    .find_map(|m| m.wire_header())
                {
                    dispatch(&mut sched, PROVIDER, header);
                    assert_eq!(terminal_kind(&sched, OWNER, 51), kind);
                }
            }
        }
    }

    /// Phase 03e A4. Hot-swap cutover ends queued work at once, keeps a
    /// dispatched request until the old provider exits, and the replacement
    /// incarnation can settle neither; its own new operation completes.
    #[test]
    fn hotswap_cutover_ends_queued_work_and_the_replacement_cannot_settle_old_tokens() {
        use api::syscall::ipc_status::{PEER_GONE, REPLY};
        let mut sched = fixture();
        add_operation(&mut sched, OWNER, PROVIDER, 61, Phase::Queued);
        queue_op_wire(&mut sched, OWNER, PROVIDER, 61);
        add_operation(&mut sched, OWNER, PROVIDER, 62, Phase::Dispatched);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        cutover_peer(&mut sched, PROVIDER, 1_003, 7);
        assert_eq!(terminal_kind(&sched, OWNER, 61), Some(PEER_GONE));
        assert_eq!(terminal_kind(&sched, OWNER, 62), None);
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        for token in [61, 62] {
            assert_eq!(
                reply_in_sched(&mut sched, FOREIGN_GENERATION, token, 0, 0),
                Err(Failure::Invalid)
            );
        }
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert_eq!(terminal_kind(&sched, OWNER, 62), Some(PEER_GONE));
        add_operation(&mut sched, OWNER, FOREIGN_GENERATION, 63, Phase::Dispatched);
        assert_eq!(
            reply_in_sched(&mut sched, FOREIGN_GENERATION, 63, 0, 0),
            Ok(())
        );
        assert_eq!(terminal_kind(&sched, OWNER, 63), Some(REPLY));
        assert_eq!(terminal_kind(&sched, OWNER, 61), Some(PEER_GONE));
    }

    /// Phase 03e A6. Retained terminals stay charged to the owner's operation
    /// quota until taken or until the owner is reaped, no terminal transition
    /// allocates, and the fixed reply storage (one `IPC_BUF_SIZE` buffer per
    /// operation, allocated at acceptance as `submit` does) is freed by take
    /// and by reaping the exited owner.
    #[test]
    fn retained_terminals_stay_charged_and_terminal_transitions_do_not_allocate() {
        use api::syscall::ipc_status::CANCELLED;
        let _guard = crate::TEST_STATE_LOCK.lock();
        assert_eq!(ipc_wire::MAX_IPC_WIRE_PAYLOAD, api::ipc::IPC_BUF_SIZE);
        let full_reply = || {
            ipc_wire::IpcWireMessage::try_new(
                ipc_wire::IpcWireHeader {
                    sender_tid: PROVIDER,
                    sender_cell_id: 1_003,
                    sender_generation: 7,
                    delivery_id: 0,
                    async_op: 1,
                },
                &[0; ipc_wire::MAX_IPC_WIRE_PAYLOAD],
            )
            .unwrap()
        };
        let charged = |sched: &Scheduler| sched.tasks[&OWNER].async_operations.len();
        let mut sched = fixture();
        let ids = 1_000..1_000 + MAX_OPERATIONS;
        for id in ids.clone() {
            let mut op = operation(&sched, OWNER, PROVIDER, id, Phase::Dispatched);
            op.reply = full_reply();
            op.deadline = if id % 4 == 2 { 0 } else { 10 };
            assert_eq!(push_operation(&mut sched, OWNER, op), Ok(()));
        }
        assert_eq!(charged(&sched), MAX_OPERATIONS);

        let before = alloc_probe::allocations();
        for id in ids.clone() {
            match id % 4 {
                0 => settle(&mut sched, PROVIDER, id),
                1 => cancel_in_sched(&mut sched, OWNER, id).unwrap(),
                _ => {}
            }
        }
        expire(&mut sched, 5);
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert_eq!(
            alloc_probe::allocations(),
            before,
            "a terminal transition allocated"
        );
        assert!(ids
            .clone()
            .all(|id| terminal_kind(&sched, OWNER, id).is_some()));
        assert_eq!(charged(&sched), MAX_OPERATIONS, "terminals stay charged");

        let refused = operation(&sched, OWNER, PROVIDER, 2_000, Phase::Queued);
        let mut admitted = operation(&sched, OWNER, PROVIDER, 2_001, Phase::Queued);
        admitted.reply = full_reply();
        let ops = &mut sched.tasks.get_mut(&OWNER).unwrap().async_operations;
        assert_eq!(
            ops.push(refused),
            Err(Failure::Busy),
            "a full set of retained terminals admitted more"
        );
        let freed = alloc_probe::freed_bytes();
        assert!(ops.remove(1_000).is_some(), "take releases one slot");
        assert!(alloc_probe::freed_bytes() - freed >= ipc_wire::MAX_IPC_WIRE_PAYLOAD);
        assert_eq!(charged(&sched), MAX_OPERATIONS - 1);
        let ops = &mut sched.tasks.get_mut(&OWNER).unwrap().async_operations;
        assert_eq!(ops.push(admitted), Ok(()));
        assert_eq!(charged(&sched), MAX_OPERATIONS);

        // Owner exit terminalises what is left but keeps it charged until the
        // task is reaped; reaping frees every retained reply buffer.
        on_owner_exits(&mut sched, OWNER);
        assert_eq!(charged(&sched), MAX_OPERATIONS);
        assert_eq!(terminal_kind(&sched, OWNER, 2_001), Some(CANCELLED));
        let freed = alloc_probe::freed_bytes();
        drop(sched.tasks.remove(&OWNER));
        assert!(
            alloc_probe::freed_bytes() - freed >= MAX_OPERATIONS * ipc_wire::MAX_IPC_WIRE_PAYLOAD,
            "reaping the owner did not free its retained reply storage"
        );
    }

    struct AdmissionModel {
        max_held: usize,
        /// Provider receives from a wire's admission until (and including) the
        /// receive that dispatches it, read back from the dequeued record (B3).
        max_receives_to_dispatch: usize,
        fresh_refused: usize,
        /// Fewest other Cells holding a queued wire at any fresh refusal.
        min_others_at_refusal: Option<usize>,
        /// A Cell holding no queued wire refused while at most
        /// `PEER_QUEUE_HEADROOM` other Cells held one (CP1 §5 B2).
        b2_violations: usize,
        received: usize,
    }

    /// Sustained competition at one receiver. Cell 1 floods (`flood_percent`
    /// of all attempts, any number outstanding); every other Cell behaves like
    /// a synchronous caller and submits only while it holds no queued wire.
    /// One step in `receive_one_in` is a provider receive; the rest submit.
    fn admission_model(
        cells: u64,
        headroom: usize,
        flood_percent: u64,
        receive_one_in: u64,
        steps: usize,
    ) -> AdmissionModel {
        use super::super::tcb::HOTSWAP_MSG_QUEUE_DEPTH as DEPTH;
        let mut sched = fixture();
        let receiver = sched.tasks.get_mut(&PROVIDER).unwrap();
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut model = AdmissionModel {
            max_held: 0,
            max_receives_to_dispatch: 0,
            fresh_refused: 0,
            min_others_at_refusal: None,
            b2_violations: 0,
            received: 0,
        };
        let held = |receiver: &Task, cell: u64| {
            receiver
                .pending_msgs
                .iter()
                .filter(|m| m.wire_header().is_some_and(|h| h.sender_cell_id == cell))
                .count()
        };
        for step in 0..steps {
            let roll = next();
            if roll % receive_one_in == 0 {
                if receiver.pending_msgs.is_empty() {
                    continue;
                }
                // A mask-0 receive takes the head; the record carries the
                // receive count at its admission in `delivery_id`.
                let admitted_at = receiver
                    .pending_msgs
                    .remove(0)
                    .wire_header()
                    .unwrap()
                    .delivery_id;
                model.received += 1;
                model.max_receives_to_dispatch = model
                    .max_receives_to_dispatch
                    .max(model.received - admitted_at as usize);
                continue;
            }
            let cell = if (roll >> 8) % 100 < flood_percent {
                1
            } else {
                1 + (roll >> 16) % cells
            };
            let sender = (5_000 + cell, 1);
            if cell != 1 && held(receiver, sender.0) > 0 {
                continue;
            }
            if peer_queue_admits(receiver, sender, headroom) {
                let header = ipc_wire::IpcWireHeader {
                    sender_tid: cell as usize,
                    sender_cell_id: sender.0,
                    sender_generation: sender.1,
                    delivery_id: model.received as u64,
                    async_op: step + 1,
                };
                super::super::queue_wire_msg(
                    receiver,
                    ipc_wire::IpcWireMessage::try_new(header, &[]).unwrap(),
                    DEPTH,
                )
                .unwrap();
                model.max_held = model.max_held.max(held(receiver, sender.0));
            } else if held(receiver, sender.0) == 0 {
                model.fresh_refused += 1;
                let others = (1..=cells)
                    .filter(|other| held(receiver, 5_000 + other) > 0)
                    .count();
                model.min_others_at_refusal = Some(
                    model
                        .min_others_at_refusal
                        .map_or(others, |min| min.min(others)),
                );
                if others <= PEER_QUEUE_HEADROOM {
                    model.b2_violations += 1;
                }
            }
        }
        model
    }

    impl core::fmt::Display for AdmissionModel {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                f,
                "max_held={} max_receives_to_dispatch={} fresh_refused={} \
                 min_others_at_refusal={:?} b2_violations={} received={}",
                self.max_held,
                self.max_receives_to_dispatch,
                self.fresh_refused,
                self.min_others_at_refusal,
                self.b2_violations,
                self.received
            )
        }
    }

    /// Phase 03e A7: CP1 §5 bounds B1-B3 under sustained competition, with a
    /// crowded run that exercises fresh refusals at the B2 limit, and the
    /// depth-only rule (headroom 0, the pre-03e behaviour) violating B2.
    #[test]
    fn peer_queue_headroom_bounds_monopolization() {
        use super::super::tcb::HOTSWAP_MSG_QUEUE_DEPTH as DEPTH;
        let check = |model: &AdmissionModel, label: &str| {
            assert!(model.received > 10_000, "{label}: the model did not run");
            assert!(
                model.max_held <= DEPTH - PEER_QUEUE_HEADROOM,
                "B1 {label}: one Cell held {} wires",
                model.max_held
            );
            assert_eq!(model.b2_violations, 0, "B2 {label}");
            assert!(
                model.max_receives_to_dispatch <= DEPTH,
                "B3 {label}: a wire waited {} receives",
                model.max_receives_to_dispatch
            );
        };
        for cells in [2, 9, 10] {
            let model = admission_model(cells, PEER_QUEUE_HEADROOM, 70, 3, 100_000);
            std::println!("A7 cells={cells} headroom={PEER_QUEUE_HEADROOM} {model}");
            check(&model, &std::format!("{cells} Cells"));
            if cells <= PEER_QUEUE_HEADROOM as u64 + 1 {
                assert_eq!(model.fresh_refused, 0, "{cells} Cells");
            }
        }
        // A crowd of 39 synchronous Cells against a slower provider fills the
        // queue: fresh Cells are refused, each time behind more than
        // `PEER_QUEUE_HEADROOM` other Cells. The exact limit is pinned by
        // `peer_queue_headroom_refuses_a_fresh_cell_only_behind_nine_others`.
        let crowd = admission_model(40, PEER_QUEUE_HEADROOM, 70, 6, 400_000);
        std::println!("A7 crowd cells=40 headroom={PEER_QUEUE_HEADROOM} {crowd}");
        check(&crowd, "crowd");
        assert!(
            crowd.fresh_refused > 0,
            "the crowd run never filled the queue"
        );
        assert!(crowd.min_others_at_refusal > Some(PEER_QUEUE_HEADROOM));
        let depth_only = admission_model(2, 0, 70, 3, 100_000);
        std::println!("A7 cells=2 headroom=0 {depth_only}");
        assert!(depth_only.max_held > DEPTH - PEER_QUEUE_HEADROOM);
        assert!(
            depth_only.b2_violations > 0,
            "the model must detect monopolization under the depth-only rule"
        );
    }

    /// Phase 03e A7 boundary (CP1 §5 B1/B2 at the limit): one Cell fills its
    /// 56-wire share, 8 distinct fresh Cells take the headroom (each then
    /// refused a second wire), and only then, behind exactly 9 other Cells,
    /// is a fresh Cell refused; one receive admits it again.
    #[test]
    fn peer_queue_headroom_refuses_a_fresh_cell_only_behind_nine_others() {
        use super::super::tcb::HOTSWAP_MSG_QUEUE_DEPTH as DEPTH;
        let mut sched = fixture();
        let receiver = sched.tasks.get_mut(&PROVIDER).unwrap();
        let mut token = 0usize;
        let mut push = |receiver: &mut Task, sender: (u64, u64)| {
            token += 1;
            let header = ipc_wire::IpcWireHeader {
                sender_tid: sender.0 as usize,
                sender_cell_id: sender.0,
                sender_generation: sender.1,
                delivery_id: token as u64,
                async_op: token,
            };
            super::super::queue_wire_msg(
                receiver,
                ipc_wire::IpcWireMessage::try_new(header, &[]).unwrap(),
                DEPTH,
            )
            .unwrap();
        };
        let flooder = (5_001, 1);
        let mut share = 0;
        while peer_queue_admits(receiver, flooder, PEER_QUEUE_HEADROOM) {
            push(receiver, flooder);
            share += 1;
        }
        assert_eq!(share, DEPTH - PEER_QUEUE_HEADROOM, "B1");
        // A different incarnation of the same Cell is a different holder.
        assert!(peer_queue_admits(receiver, (5_001, 2), PEER_QUEUE_HEADROOM));
        for index in 0..PEER_QUEUE_HEADROOM as u64 {
            let fresh = (6_000 + index, 1);
            assert!(peer_queue_admits(receiver, fresh, PEER_QUEUE_HEADROOM));
            push(receiver, fresh);
            assert!(!peer_queue_admits(receiver, fresh, PEER_QUEUE_HEADROOM));
        }
        assert_eq!(receiver.pending_msgs.len(), DEPTH);
        let ninth = (7_000, 1);
        assert!(
            !peer_queue_admits(receiver, ninth, PEER_QUEUE_HEADROOM),
            "a full queue with 9 other holders refuses a fresh Cell"
        );
        drop(receiver.pending_msgs.remove(0));
        assert!(peer_queue_admits(receiver, ninth, PEER_QUEUE_HEADROOM));
        assert!(!peer_queue_admits(receiver, flooder, PEER_QUEUE_HEADROOM));
    }

    #[test]
    fn caller_death_removes_only_exact_queued_identity_and_preserves_dispatch_context() {
        let mut sched = fixture();
        for token in [11, 12, 13, 14, 15, 16] {
            add_operation(
                &mut sched,
                OWNER,
                if token >= 15 { WORKER } else { PROVIDER },
                token,
                Phase::Queued,
            );
        }
        let header = ipc_wire::IpcWireHeader {
            sender_tid: OWNER,
            sender_cell_id: 1_002,
            sender_generation: 3,
            delivery_id: 13,
            async_op: 13,
        };
        dispatch(&mut sched, PROVIDER, header);
        serve(&mut sched, PROVIDER, OWNER, 13);
        assert!(matches!(
            sched.tasks[&OWNER].async_operations.find(13).unwrap().phase,
            Phase::Dispatched
        ));
        let context = |task: &Task| {
            (
                task.caller.current,
                task.caller.cell_id,
                task.caller.cell_generation,
                task.caller.request_generation,
                task.current_async_op,
            )
        };
        let dispatched_context = context(&sched.tasks[&PROVIDER]);
        assert_ne!(dispatched_context.3, 0);
        let queued = [
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 11,
                    async_op: 11,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 12,
                    async_op: 12,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 21,
                    async_op: 21,
                    sender_generation: 4,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 22,
                    async_op: 22,
                    sender_cell_id: 1_004,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 23,
                    async_op: 23,
                    sender_tid: FOREIGN_CELL,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 24,
                    async_op: 0,
                    ..header
                },
            ),
            (
                PROVIDER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 14,
                    async_op: 14,
                    ..header
                },
            ),
            (
                WORKER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 15,
                    async_op: 15,
                    ..header
                },
            ),
            (
                WORKER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 25,
                    async_op: 0,
                    ..header
                },
            ),
            (
                WORKER,
                ipc_wire::IpcWireHeader {
                    delivery_id: 16,
                    async_op: 16,
                    ..header
                },
            ),
        ];
        for (peer, wire_header) in queued {
            super::super::queue_wire_msg(
                sched.tasks.get_mut(&peer).unwrap(),
                ipc_wire::IpcWireMessage::try_new(wire_header, b"preserved").unwrap(),
                64,
            )
            .unwrap();
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
            let remaining: Vec<_> = sched.tasks[&peer]
                .pending_msgs
                .as_slice()
                .iter()
                .map(|msg| {
                    let h = msg.wire_header().unwrap();
                    (
                        h.sender_tid,
                        h.sender_cell_id,
                        h.sender_generation,
                        h.delivery_id,
                        h.async_op,
                    )
                })
                .collect();
            let expected: Vec<_> = queued
                .iter()
                .filter(|(tid, h)| {
                    *tid == peer
                        && !(h.async_op != 0
                            && h.sender_tid == OWNER
                            && h.sender_cell_id == 1_002
                            && h.sender_generation == 3)
                })
                .map(|(_, h)| {
                    (
                        h.sender_tid,
                        h.sender_cell_id,
                        h.sender_generation,
                        h.delivery_id,
                        h.async_op,
                    )
                })
                .collect();
            assert_eq!(
                remaining, expected,
                "unrelated generations, cells, TIDs and raw IPC survive in order"
            );
        }
    }
    #[test]
    fn deterministic_two_hart_publication_and_wake_races() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();

        // 1. Race 1: Pre-park publication race
        // Provider publishes reply before caller on another hart arms WaitIpc.
        add_operation(&mut sched, OWNER, PROVIDER, 101, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 101);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        // When caller subsequently executes wait, has_terminal is true immediately,
        // so caller does not block or lose the wakeup.
        assert!(sched.tasks[&OWNER].async_operations.has_terminal());
        assert!(matches!(
            sched.tasks[&OWNER]
                .async_operations
                .find(101)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::REPLY,
                ..
            }
        ));

        // 2. Race 2: Cross-hart parked wake race
        // Caller is parked in WaitIpc when Provider settles the call.
        add_operation(&mut sched, OWNER, PROVIDER, 102, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 102);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        // Caller must be transitioned to Ready and pushed to the ready queue.
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        assert!(sched.tasks[&OWNER].async_operations.has_terminal());
        crate::sched::hart_local::ready::remove_from_all(OWNER);

        // 3. Race 3: Remote peer-death vs parked caller race
        // Caller is parked in WaitIpc when Provider crashes on another hart.
        add_operation(&mut sched, OWNER, PROVIDER, 103, Phase::Dispatched);
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitIpc { deadline: None };
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        // Caller must be transitioned to Ready with PeerGone terminal.
        assert!(matches!(sched.tasks[&OWNER].state, TaskState::Ready));
        assert!(matches!(
            sched.tasks[&OWNER]
                .async_operations
                .find(103)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::PEER_GONE,
                ..
            }
        ));
        crate::sched::hart_local::ready::remove_from_all(OWNER);

        // 4. Race 4: Remote deadline expiration vs reply race
        // Case A: Reply wins lock first, deadline cannot overwrite it.
        add_operation(&mut sched, OWNER, WORKER, 104, Phase::Dispatched);
        serve(&mut sched, WORKER, OWNER, 104);
        assert_eq!(
            reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0),
            Some(Ok(()))
        );
        expire(&mut sched, u64::MAX);
        assert!(matches!(
            sched.tasks[&OWNER]
                .async_operations
                .find(104)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::REPLY,
                ..
            }
        ));

        // Case B: Expire wins lock first, subsequent reply is rejected.
        add_operation(&mut sched, OWNER, WORKER, 105, Phase::Dispatched);
        serve(&mut sched, WORKER, OWNER, 105);
        expire(&mut sched, 100);
        assert!(matches!(
            sched.tasks[&OWNER]
                .async_operations
                .find(105)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::INDETERMINATE,
                ..
            }
        ));
        assert_eq!(
            reply_current_in_sched(&mut sched, WORKER, OWNER, 0, 0),
            Some(Ok(()))
        );
        // Terminal remains Indeterminate, not overwritten by late reply.
        assert!(matches!(
            sched.tasks[&OWNER]
                .async_operations
                .find(105)
                .unwrap()
                .phase,
            Phase::Terminal {
                kind: api::syscall::ipc_status::INDETERMINATE,
                ..
            }
        ));
    }

    // ---- Multi-source completion wait: IPC source (Phase 03d CP1 §5, §9) ----

    use super::super::completion_wait::{
        select_locked, select_source, Selection, INITIAL_ROTOR, SELECTION_ORDER,
    };
    const IPC: u32 = api::completion::source::IPC;
    const NET_RX: u32 = api::completion::source::NET_RX;
    const TIMER: u32 = api::completion::source::TIMER;
    const ALL_SOURCES: u32 = NET_RX | IPC | TIMER;

    /// Thread-local allocation counter for the wait-path measurement (M2).
    mod alloc_probe {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        std::thread_local! {
            static COUNT: Cell<usize> = const { Cell::new(0) };
            static FREED: Cell<usize> = const { Cell::new(0) };
        }

        pub struct Counting;

        fn bump() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }

        // SAFETY: every call forwards unchanged to `System`; the counter is a
        // const-initialised thread-local `Cell` that never allocates.
        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                bump();
                unsafe { System.alloc(layout) }
            }
            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                bump();
                unsafe { System.alloc_zeroed(layout) }
            }
            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                bump();
                unsafe { System.realloc(ptr, layout, new_size) }
            }
            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                let _ = FREED.try_with(|freed| freed.set(freed.get() + layout.size()));
                unsafe { System.dealloc(ptr, layout) }
            }
        }

        #[global_allocator]
        static GLOBAL: Counting = Counting;

        pub fn allocations() -> usize {
            COUNT.with(|count| count.get())
        }

        /// Bytes this thread returned to the allocator so far.
        pub fn freed_bytes() -> usize {
            FREED.with(|freed| freed.get())
        }
    }

    fn settle(sched: &mut Scheduler, peer: usize, token: usize) {
        assert_eq!(reply_in_sched(sched, peer, token, 0, 0), Ok(()));
    }

    fn notify(sched: &mut Scheduler) -> Option<usize> {
        take_notification(sched.tasks.get_mut(&OWNER).unwrap()).map(|n| n.token)
    }

    fn select(sched: &mut Scheduler, mask: u32, net_rx: bool, timer: bool) -> Option<Selection> {
        select_locked(sched.tasks.get_mut(&OWNER).unwrap(), mask, net_rx, timer)
    }

    fn park(sched: &mut Scheduler, mask: u32) {
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::WaitCompletion {
            source: mask,
            deadline: None,
        };
    }

    fn kind(sched: &Scheduler, token: usize) -> Option<u32> {
        match sched.tasks[&OWNER].async_operations.find(token)?.phase {
            Phase::Terminal { kind, .. } => Some(kind),
            _ => None,
        }
    }

    #[test]
    fn ipc_notification_is_one_shot_retained_and_cleared_by_take() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        add_operation(&mut sched, OWNER, PROVIDER, 21, Phase::Dispatched);
        add_operation(&mut sched, OWNER, PROVIDER, 22, Phase::Dispatched);
        assert!(!has_pending_notification(&sched.tasks[&OWNER]));
        assert_eq!(notify(&mut sched), None);

        settle(&mut sched, PROVIDER, 22);
        assert!(has_pending_notification(&sched.tasks[&OWNER]));
        assert_eq!(notify(&mut sched), Some(22));
        assert_eq!(notify(&mut sched), None, "a terminal is reported once");
        assert!(
            sched.tasks[&OWNER].async_operations.has_terminal(),
            "IpcWait keeps its level view of the retained result"
        );
        assert_eq!(kind(&sched, 22), Some(api::syscall::ipc_status::REPLY));
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 22, 0, 0),
            Err(Failure::Invalid),
            "a duplicate reply cannot re-arm the notification"
        );
        assert_eq!(notify(&mut sched), None);

        settle(&mut sched, PROVIDER, 21);
        sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .remove(21);
        assert_eq!(
            notify(&mut sched),
            None,
            "take before the wait leaves no record"
        );
    }

    #[test]
    fn ipc_notifications_follow_submission_order_with_a_bounded_scan() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        let first = 30;
        for token in first..first + MAX_OPERATIONS {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Dispatched);
        }
        for token in (first..first + MAX_OPERATIONS).rev() {
            settle(&mut sched, PROVIDER, token);
        }
        let mut max_scanned = 0;
        for expected in first..first + MAX_OPERATIONS {
            let n = take_notification(sched.tasks.get_mut(&OWNER).unwrap()).unwrap();
            assert_eq!(n.token, expected, "oldest submission first");
            max_scanned = max_scanned.max(n.scanned);
        }
        assert_eq!(notify(&mut sched), None);
        assert!(max_scanned <= MAX_OPERATIONS);
        std::println!("P03D-M1 max_scanned={max_scanned} bound={MAX_OPERATIONS}");
    }

    #[test]
    fn source_rotation_reaches_every_ready_source_within_four_selections() {
        const IRQ: u32 = api::completion::source::IRQ;
        let order = [NET_RX, IPC, IRQ, TIMER];
        assert_eq!(SELECTION_ORDER, order);
        assert_eq!(select_source(0, 0), None);
        for rotor in 0..4u8 {
            for ready in 1..16u32 {
                let ready_bits = (0..4)
                    .filter(|i| ready & (1 << i) != 0)
                    .fold(0, |acc, i| acc | order[i]);
                let expected = (1..=4)
                    .map(|step| (rotor as usize + step) % 4)
                    .find(|&i| ready_bits & order[i] != 0)
                    .unwrap();
                assert_eq!(
                    select_source(rotor, ready_bits),
                    Some((order[expected], expected as u8))
                );
            }
        }
        assert_eq!(
            select_source(INITIAL_ROTOR, NET_RX | IPC | IRQ | TIMER),
            Some((NET_RX, 0)),
            "a fresh task starts at NET_RX"
        );
        let mut rotor = INITIAL_ROTOR;
        let mut last = [0usize; 4];
        let mut counts = [0usize; 4];
        for round in 1..=400 {
            let (_, index) = select_source(rotor, NET_RX | IPC | IRQ | TIMER).unwrap();
            let index = index as usize;
            assert!(
                round - last[index] <= 4,
                "a ready source waits at most four selections"
            );
            last[index] = round;
            counts[index] += 1;
            rotor = index as u8;
        }
        assert_eq!(counts, [100, 100, 100, 100]);
    }

    #[test]
    fn sustained_multi_producer_selection_meets_the_stated_bound() {
        use alloc::collections::BTreeMap;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        // Peer A (PROVIDER) answers constantly, newest first, to try to overtake;
        // peer B (WORKER) answers rarely, oldest first. NET_RX and TIMER are
        // ready on every call, the adversarial case for the IPC source.
        let mut next = 10_000usize;
        for i in 0..MAX_OPERATIONS {
            let peer = if i % 2 == 0 { PROVIDER } else { WORKER };
            add_operation(&mut sched, OWNER, peer, next, Phase::Dispatched);
            next += 1;
        }
        let dispatched = |sched: &Scheduler, peer: usize| -> Vec<usize> {
            sched.tasks[&OWNER]
                .async_operations
                .0
                .iter()
                .filter(|op| op.peer_tid == peer && matches!(op.phase, Phase::Dispatched))
                .map(|op| op.id)
                .collect()
        };
        let mut armed: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        let (mut returns, mut ipc_records) = (0usize, 0usize);
        let (mut max_returns, mut max_overtakes, mut max_scanned) = (0usize, 0usize, 0usize);
        let (mut last_net, mut last_timer, mut max_net_gap, mut max_timer_gap) = (0, 0, 0, 0);
        while returns < 10_000 {
            for token in dispatched(&sched, PROVIDER).into_iter().rev().take(2) {
                settle(&mut sched, PROVIDER, token);
                armed.insert(token, (returns, ipc_records));
            }
            if returns % 37 == 0 {
                if let Some(&token) = dispatched(&sched, WORKER).first() {
                    settle(&mut sched, WORKER, token);
                    armed.insert(token, (returns, ipc_records));
                }
            }
            let chosen = select(&mut sched, ALL_SOURCES, true, true).unwrap();
            returns += 1;
            if chosen.source == IPC {
                let token = chosen.result as usize;
                let (armed_at, records_at) = armed.remove(&token).expect("only armed tokens");
                max_returns = max_returns.max(returns - armed_at);
                max_overtakes = max_overtakes.max(ipc_records - records_at);
                max_scanned = max_scanned.max(chosen.scanned);
                ipc_records += 1;
                let peer = sched.tasks[&OWNER]
                    .async_operations
                    .find(token)
                    .unwrap()
                    .peer_tid;
                sched
                    .tasks
                    .get_mut(&OWNER)
                    .unwrap()
                    .async_operations
                    .remove(token);
                add_operation(&mut sched, OWNER, peer, next, Phase::Dispatched);
                next += 1;
            } else if chosen.source == NET_RX {
                max_net_gap = max_net_gap.max(returns - last_net);
                last_net = returns;
            } else {
                max_timer_gap = max_timer_gap.max(returns - last_timer);
                last_timer = returns;
            }
        }
        while let Some(chosen) = select(&mut sched, IPC, false, false) {
            assert!(armed.remove(&(chosen.result as usize)).is_some());
        }
        assert!(armed.is_empty(), "every armed terminal was reported");
        assert!(
            max_returns <= 3 * MAX_OPERATIONS,
            "max_returns={max_returns}"
        );
        assert!(
            max_overtakes < MAX_OPERATIONS,
            "max_overtakes={max_overtakes}"
        );
        assert!(max_net_gap <= 3 && max_timer_gap <= 3);
        assert!(max_scanned <= MAX_OPERATIONS);
        std::println!(
            "P03D-A8 returns={returns} ipc_records={ipc_records} max_returns={max_returns} \
             bound={} max_overtakes={max_overtakes} max_net_gap={max_net_gap} \
             max_timer_gap={max_timer_gap} max_scanned={max_scanned}",
            3 * MAX_OPERATIONS
        );
    }

    #[test]
    fn stale_and_late_replies_never_satisfy_another_operation_or_generation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;

        // Expired dispatched work is notified once; the late reply is refused.
        add_operation(&mut sched, OWNER, PROVIDER, 41, Phase::Dispatched);
        expire(&mut sched, u64::MAX);
        assert_eq!(notify(&mut sched), Some(41));
        assert_eq!(
            reply_in_sched(&mut sched, PROVIDER, 41, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(notify(&mut sched), None);
        assert_eq!(
            kind(&sched, 41),
            Some(api::syscall::ipc_status::INDETERMINATE)
        );

        // A replacement provider generation cannot settle or arm the old token.
        add_operation(&mut sched, OWNER, PROVIDER, 42, Phase::Dispatched);
        assert_eq!(
            reply_in_sched(&mut sched, FOREIGN_GENERATION, 42, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(
            reply_in_sched(&mut sched, FOREIGN_CELL, 42, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(notify(&mut sched), None);
        settle(&mut sched, PROVIDER, 42);
        assert_eq!(notify(&mut sched), Some(42));
        assert_eq!(notify(&mut sched), None);

        // A terminal owned by an older incarnation is never reported to a newer one.
        add_operation(&mut sched, OWNER, PROVIDER, 43, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 43);
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation += 1;
        assert!(!has_pending_notification(&sched.tasks[&OWNER]));
        assert_eq!(notify(&mut sched), None);
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation -= 1;

        // Another Cell's waiter never sees this owner's token, even if a record
        // naming it were misplaced into that waiter's own list.
        let misplaced = sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .remove(43)
            .unwrap();
        let foreign = sched.tasks.get_mut(&FOREIGN_CELL).unwrap();
        foreign.async_operations.push(misplaced).unwrap();
        assert!(!has_pending_notification(foreign));
        assert_eq!(take_notification(foreign), None);
    }

    #[test]
    fn two_hart_model_rows_for_the_ipc_source() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        let ready = |sched: &Scheduler| matches!(sched.tasks[&OWNER].state, TaskState::Ready);

        // R1 pre-park publication: the terminal lands before the waiter publishes.
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        add_operation(&mut sched, OWNER, PROVIDER, 51, Phase::Dispatched);
        serve(&mut sched, PROVIDER, OWNER, 51);
        assert_eq!(
            reply_current_in_sched(&mut sched, PROVIDER, OWNER, 0, 0),
            Some(Ok(()))
        );
        let task = sched.tasks.get_mut(&OWNER).unwrap();
        assert_eq!(
            super::super::completion_wait::publish_wait_state_locked(
                task,
                OWNER,
                IPC | TIMER,
                Some(1)
            ),
            super::super::completion_wait::CompletionParkDecision::IpcPending
        );
        assert!(ready(&sched), "the waiter does not park");
        assert_eq!(
            select(&mut sched, IPC | TIMER, false, false)
                .unwrap()
                .result,
            51
        );

        // R2 parked wake, and the unchanged NET_RX-only behaviour.
        add_operation(&mut sched, OWNER, PROVIDER, 52, Phase::Dispatched);
        park(&mut sched, NET_RX);
        settle(&mut sched, PROVIDER, 52);
        assert!(
            !ready(&sched),
            "a NET_RX-only wait is not woken by an IPC terminal"
        );
        assert_eq!(notify(&mut sched), Some(52));
        add_operation(&mut sched, OWNER, PROVIDER, 53, Phase::Dispatched);
        park(&mut sched, IPC | TIMER);
        settle(&mut sched, PROVIDER, 53);
        assert!(
            ready(&sched),
            "the IPC source wakes the parked waiter in the same hold"
        );
        crate::sched::hart_local::ready::remove_from_all(OWNER);
        assert_eq!(select(&mut sched, IPC, false, false).unwrap().result, 53);

        // R3 peer death against a parked waiter.
        add_operation(&mut sched, OWNER, PROVIDER, 54, Phase::Dispatched);
        park(&mut sched, IPC);
        peer_died(&mut sched, PROVIDER, 1_003, 7);
        assert!(ready(&sched));
        crate::sched::hart_local::ready::remove_from_all(OWNER);
        assert_eq!(select(&mut sched, IPC, false, false).unwrap().result, 54);
        assert_eq!(kind(&sched, 54), Some(api::syscall::ipc_status::PEER_GONE));

        // R4 expire versus reply, both orders: one terminal, one notification.
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        add_operation(&mut sched, OWNER, WORKER, 55, Phase::Dispatched);
        settle(&mut sched, WORKER, 55);
        expire(&mut sched, u64::MAX);
        assert_eq!(kind(&sched, 55), Some(api::syscall::ipc_status::REPLY));
        add_operation(&mut sched, OWNER, WORKER, 56, Phase::Dispatched);
        expire(&mut sched, u64::MAX);
        assert_eq!(
            reply_in_sched(&mut sched, WORKER, 56, 0, 0),
            Err(Failure::Invalid)
        );
        assert_eq!(
            kind(&sched, 56),
            Some(api::syscall::ipc_status::INDETERMINATE)
        );
        assert_eq!(notify(&mut sched), Some(55));
        assert_eq!(notify(&mut sched), Some(56));
        assert_eq!(notify(&mut sched), None);

        // R5 take wins over notification.
        add_operation(&mut sched, OWNER, PROVIDER, 57, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 57);
        sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .remove(57);
        assert_eq!(select(&mut sched, IPC, false, false), None);

        // R6 NET_RX and IPC at the same instant: one record now, the other kept.
        add_operation(&mut sched, OWNER, PROVIDER, 58, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 58);
        sched.tasks.get_mut(&OWNER).unwrap().completion_rotor = INITIAL_ROTOR;
        let first = select(&mut sched, ALL_SOURCES, true, false).unwrap();
        assert_eq!((first.source, first.relevel_net_rx), (NET_RX, false));
        assert_eq!(
            select(&mut sched, ALL_SOURCES, false, false)
                .unwrap()
                .result,
            58
        );
        add_operation(&mut sched, OWNER, PROVIDER, 59, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 59);
        sched.tasks.get_mut(&OWNER).unwrap().completion_rotor = 0;
        let ipc_first = select(&mut sched, ALL_SOURCES, true, false).unwrap();
        assert_eq!(ipc_first.result, 59);
        assert!(
            ipc_first.relevel_net_rx,
            "the losing NET_RX observation is re-levelled"
        );

        // R7 deadline and IPC together: TIMER is this call's deadline only.
        add_operation(&mut sched, OWNER, PROVIDER, 60, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 60);
        sched.tasks.get_mut(&OWNER).unwrap().completion_rotor = 0;
        assert_eq!(
            select(&mut sched, IPC | TIMER, false, true).unwrap().result,
            60
        );
        assert_eq!(select(&mut sched, IPC | TIMER, false, false), None);

        // R8 a raw mailbox message interrupts only a mask that names NET_RX and
        // never consumes a pending IPC notification.
        add_operation(&mut sched, OWNER, PROVIDER, 61, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 61);
        park(&mut sched, NET_RX | IPC);
        let cause =
            super::super::wake_after_ipc_publish(sched.tasks.get_mut(&OWNER).unwrap(), false);
        assert!(cause == super::super::IpcWakeCause::NetRxCompletionWait);
        park(&mut sched, IPC);
        let cause =
            super::super::wake_after_ipc_publish(sched.tasks.get_mut(&OWNER).unwrap(), false);
        assert!(cause == super::super::IpcWakeCause::None);
        assert_eq!(notify(&mut sched), Some(61));
        crate::sched::hart_local::ready::remove_from_all(OWNER);
    }

    #[test]
    fn retained_terminals_rewake_ipc_wait_but_not_a_one_shot_waiter() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        for token in 71..79 {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Dispatched);
            settle(&mut sched, PROVIDER, token);
            assert_eq!(notify(&mut sched), Some(token));
        }
        add_operation(&mut sched, OWNER, PROVIDER, 79, Phase::Dispatched);
        // E3: IpcWait's gate is `has_terminal`; the one-shot gate is the
        // pending notification. Same 1000 park attempts, retained results kept.
        let task = &sched.tasks[&OWNER];
        let ipc_wait_spurious = (0..1000)
            .filter(|_| task.async_operations.has_terminal())
            .count();
        let one_shot_spurious = (0..1000).filter(|_| has_pending_notification(task)).count();
        assert_eq!((ipc_wait_spurious, one_shot_spurious), (1000, 0));
        std::println!(
            "P03D-E3 retained=8 ipcwait_spurious={ipc_wait_spurious}/1000 \
             oneshot_spurious={one_shot_spurious}/1000"
        );
    }

    #[test]
    fn wait_path_selection_does_not_allocate() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        for token in 100..100 + MAX_OPERATIONS {
            add_operation(&mut sched, OWNER, PROVIDER, token, Phase::Dispatched);
            settle(&mut sched, PROVIDER, token);
        }
        // A Cell's queue is created once, charged to the kernel; count after it.
        let created = alloc_probe::allocations();
        let queue = super::super::completion::queue_for(&mut sched, OWNER).expect("queue");
        let creation_allocations = alloc_probe::allocations() - created;
        let before = alloc_probe::allocations();
        let mut records = 0;
        {
            let again = super::super::completion::queue_for(&mut sched, OWNER).expect("queue");
            let slot = again.reserve().expect("slot");
            for tid in 0..super::super::completion::MAX_WAITERS {
                again.register_waiter(5_000 + tid);
            }
            for tid in 0..super::super::completion::MAX_WAITERS {
                let _ = again.clear_waiter(5_000 + tid);
            }
            assert!(again.release(slot));
            let task = sched.tasks.get_mut(&OWNER).unwrap();
            let _ = super::super::completion_wait::publish_wait_state_locked(
                task,
                OWNER,
                ALL_SOURCES,
                Some(1),
            );
            while select_locked(task, ALL_SOURCES, true, true).is_some()
                && records < 3 * MAX_OPERATIONS
            {
                records += 1;
                let _ = has_pending_notification(task);
            }
        }
        drop(queue);
        let allocated = alloc_probe::allocations() - before;
        assert_eq!(
            allocated, 0,
            "wait-side selection allocated {allocated} times"
        );
        // Every pre-03d field is a word, `Phase` or `IpcWireMessage`, so the
        // pre-03d size is their sum; the one-shot flag may cost one alignment unit.
        let pre_03d = 9 * core::mem::size_of::<u64>()
            + core::mem::size_of::<Phase>()
            + core::mem::size_of::<ipc_wire::IpcWireMessage>();
        let op_size = core::mem::size_of::<Operation>();
        #[cfg(not(feature = "p01-trace"))]
        assert!(
            op_size <= pre_03d + 8,
            "Operation grew from {pre_03d} to {op_size}"
        );
        std::println!(
            "P03D-M2 records={records} wait_path_allocations={allocated} \
             queue_creation_allocations={creation_allocations} \
             operation_bytes_pre={pre_03d} operation_bytes_now={op_size} task_rotor_bytes={}",
            core::mem::size_of::<u8>()
        );
    }

    #[test]
    fn a_notification_whose_record_was_not_delivered_is_re_armed_once() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = fixture();
        sched.tasks.get_mut(&OWNER).unwrap().state = TaskState::Ready;
        add_operation(&mut sched, OWNER, PROVIDER, 91, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 91);
        assert_eq!(notify(&mut sched), Some(91));
        let owner = sched.tasks.get_mut(&OWNER).unwrap();
        assert!(restore_notification(owner, 91));
        assert!(
            !restore_notification(owner, 91),
            "only a consumed notification re-arms"
        );
        assert_eq!(
            notify(&mut sched),
            Some(91),
            "reported again after the failed copy"
        );
        assert_eq!(notify(&mut sched), None);
        // A taken operation cannot be re-armed.
        sched
            .tasks
            .get_mut(&OWNER)
            .unwrap()
            .async_operations
            .remove(91);
        assert!(!restore_notification(
            sched.tasks.get_mut(&OWNER).unwrap(),
            91
        ));
        // Another incarnation cannot re-arm this owner's token.
        add_operation(&mut sched, OWNER, PROVIDER, 92, Phase::Dispatched);
        settle(&mut sched, PROVIDER, 92);
        assert_eq!(notify(&mut sched), Some(92));
        sched.tasks.get_mut(&OWNER).unwrap().cell_generation += 1;
        assert!(!restore_notification(
            sched.tasks.get_mut(&OWNER).unwrap(),
            92
        ));
    }
}
