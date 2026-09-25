//! Futex wait queues — wait-on-address for cell threads (ADR-0018 §2.1).
//!
//! A futex wait parks the calling task until another task wakes the same *key*.
//! The key is `(address-space identity, generation, word address)`, which is what
//! makes the primitive correct in both execution tiers:
//!
//! * Tier 1 cells share one address space, so the identity is the shared root and
//!   the address alone names the word — two cells that can both write a word can
//!   also wake each other on it, which is the standard shared-memory contract.
//! * Tier 2 cells have private page tables, so the same virtual address means
//!   different words in different domains. The identity and generation in the key
//!   keep a peer domain from waking (or being woken by) a wait it does not share.
//!
//! Correctness of the compare-and-park is the whole point of this module:
//!
//! 1. the caller's word is read through the validated, domain-aware copy path —
//!    never by dereferencing a raw user pointer;
//! 2. the *deciding* read happens while `SCHEDULER` is held, and the enqueue
//!    happens under the same lock;
//! 3. a waker updates the word before it calls wake and takes `SCHEDULER` to
//!    select waiters.
//!
//! Together those give the standard result: a wake that raced the park is either
//! observed by the deciding read (the waiter returns `VALUE_MISMATCH` and never
//! sleeps) or arrives after the waiter is enqueued (and wakes it). There is no
//! window in which a woken waiter sleeps forever.
//!
//! Out of scope by design: `FUTEX_REQUEUE`, priority inheritance, and robust-list
//! semantics. A waiter that dies leaves no entry behind — the scheduler sweep and
//! the wake path both drop entries whose task is no longer waiting.

use crate::sync::Spinlock;
use alloc::collections::{BTreeMap, VecDeque};
use types::VAddr;

/// Wait outcome: the task was woken by a `FutexWake`.
pub(crate) const OUTCOME_WOKEN: usize = 0;
/// Wait outcome: the word did not hold the expected value, so the caller never
/// parked (or parked and was woken without the value having changed).
pub(crate) const OUTCOME_VALUE_MISMATCH: usize = 1;
/// Wait outcome: the deadline elapsed before any wake.
pub(crate) const OUTCOME_TIMED_OUT: usize = 2;

/// A wait key. `space` is the address-space identity (0 = the shared SAS root) and
/// `generation` pins the specific domain incarnation, so a recycled domain identity
/// cannot inherit an old domain's waiters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FutexKey {
    pub space: u64,
    pub generation: u64,
    pub addr: VAddr,
}

/// Key for a word in the calling task's address space.
pub(crate) fn key_for(addr: VAddr) -> FutexKey {
    let (space, generation) = super::hart_local::current_domain();
    FutexKey {
        space,
        generation,
        addr,
    }
}

/// Waiter queues, one entry per key. A leaf lock: taken while `SCHEDULER` is held
/// (the same order as the ready queues), never the reverse.
///
/// The queue is bounded by the per-cell thread cap (`MAX_THREADS_PER_CELL`): only a
/// task that is parked can be enqueued, and a cell cannot have more parked tasks
/// than it has threads. It never grows without bound.
static WAITERS: Spinlock<BTreeMap<FutexKey, VecDeque<usize>>> = Spinlock::new(BTreeMap::new());

fn enqueue(key: FutexKey, tid: usize) {
    let mut waiters = WAITERS.lock();
    let queue = waiters.entry(key).or_default();
    if !queue.contains(&tid) {
        queue.push_back(tid);
    }
}

/// Drop one waiter. Called on timeout and on any exit from the wait state.
pub(crate) fn remove_waiter(key: FutexKey, tid: usize) {
    let mut waiters = WAITERS.lock();
    if let Some(queue) = waiters.get_mut(&key) {
        queue.retain(|queued| *queued != tid);
        if queue.is_empty() {
            waiters.remove(&key);
        }
    }
}

/// Take up to `count` waiters off a key's queue (`count == 0` takes every waiter).
fn take_waiters(key: FutexKey, count: usize, out: &mut VecDeque<usize>) {
    let mut waiters = WAITERS.lock();
    if let Some(queue) = waiters.get_mut(&key) {
        while count == 0 || out.len() < count {
            match queue.pop_front() {
                Some(tid) => out.push_back(tid),
                None => break,
            }
        }
        if queue.is_empty() {
            waiters.remove(&key);
        }
    }
}

/// Read the caller's futex word through the validated, domain-aware copy path.
///
/// Allocation-free: a four-byte stack buffer and one copy through the caller's
/// view. Null, kernel, unmapped, and peer-domain addresses fail here and become the
/// ABI's recoverable error — the kernel never dereferences the raw pointer.
pub(crate) fn read_word(
    view: &super::copy_glue::TaskCopyView,
    addr: VAddr,
) -> Result<u32, super::syscall::SyscallError> {
    if addr == 0 || !addr.is_multiple_of(core::mem::align_of::<u32>()) {
        return Err(super::syscall::SyscallError::InvalidInput);
    }
    let mut bytes = [0u8; 4];
    view.read_into(addr, &mut bytes)
        .map_err(|_| super::syscall::SyscallError::InvalidInput)?;
    Ok(u32::from_ne_bytes(bytes))
}

/// Park the caller until woken, the word changes, or the deadline elapses.
///
/// `timeout_ticks == 0` blocks indefinitely (the `WaitEvent` convention). Returns
/// the wait outcome, or `Err` when the caller has no live task record.
pub(crate) fn wait(
    caller_id: usize,
    addr: VAddr,
    expected: u32,
    timeout_ticks: u64,
) -> Result<usize, super::syscall::SyscallError> {
    let key = key_for(addr);

    // The copy view is taken *before* `SCHEDULER`: building it locks the scheduler
    // (it snapshots the caller's task), so taking it under the lock would
    // self-deadlock. The view is a plain descriptor; the copy itself is lock-free
    // and validated per call, so using it under the lock is safe.
    let view = super::syscall::caller_copy_view_for(caller_id)?;

    // First check, outside the lock: the common case is "the value already moved",
    // and it needs no queue traffic at all.
    if read_word(&view, addr)? != expected {
        return Ok(OUTCOME_VALUE_MISMATCH);
    }

    let deadline = if timeout_ticks == 0 {
        None
    } else {
        Some(super::system_ticks() as u64 + timeout_ticks)
    };

    // Decide and park under one lock: the word is re-read here, so a wake that
    // already ran (it must take this same lock to select waiters) is observed
    // instead of being lost.
    let mut scheduler = super::SCHEDULER.lock();
    let sched = scheduler
        .as_mut()
        .ok_or(super::syscall::SyscallError::Unknown)?;
    if read_word(&view, addr)? != expected {
        return Ok(OUTCOME_VALUE_MISMATCH);
    }
    match sched.tasks.get_mut(&caller_id) {
        Some(task) => {
            enqueue(key, caller_id);
            task.state = super::tcb::TaskState::FutexWait { key, deadline };
            // A waker writes the outcome into this slot before pushing the task to
            // a run queue; the handler reads it back after the park.
            task.trap_frame.regs[10] = OUTCOME_WOKEN as _;
        }
        None => return Err(super::syscall::SyscallError::PermissionDenied),
    }
    drop(scheduler);

    super::yield_cpu();

    // Resumed: the waker or the deadline sweep published the outcome.
    let outcome = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|task| task.trap_frame.regs[10])
        .unwrap_or(OUTCOME_WOKEN);
    // The task left the wait state; make sure no stale queue entry survives.
    remove_waiter(key, caller_id);
    Ok(outcome)
}

/// Wake up to `count` waiters on the caller's key. Returns how many were woken.
pub(crate) fn wake(
    caller_id: usize,
    addr: VAddr,
    count: usize,
) -> Result<usize, super::syscall::SyscallError> {
    let key = key_for(addr);
    let mut scheduler = super::SCHEDULER.lock();
    let sched = scheduler
        .as_mut()
        .ok_or(super::syscall::SyscallError::Unknown)?;
    if !sched.tasks.contains_key(&caller_id) {
        return Err(super::syscall::SyscallError::PermissionDenied);
    }

    let mut candidates = VecDeque::new();
    take_waiters(key, count, &mut candidates);

    let mut woken = 0;
    while let Some(tid) = candidates.pop_front() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            // Only a task still parked on this key may be woken: a timeout or an
            // exit that raced this wake already removed it from the wait state.
            if matches!(task.state, super::tcb::TaskState::FutexWait { key: k, .. } if k == key) {
                task.state = super::tcb::TaskState::Ready;
                task.trap_frame.regs[10] = OUTCOME_WOKEN as _;
                sched.push_ready(tid);
                woken += 1;
            }
        }
    }
    Ok(woken)
}

/// Boot-time assertion for the wait key's discriminating power.
///
/// The runtime cross-domain case (two Tier 2 cells waiting on the same virtual
/// address) needs two cooperating cells; this proves the same property at the
/// queue: a waiter enqueued under one address space is invisible to a wake from
/// another, and to a later generation of the same space.
#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    let addr = 0x1000usize;
    let owner = FutexKey {
        space: 1,
        generation: 1,
        addr,
    };
    let peer_space = FutexKey {
        space: 2,
        generation: 1,
        addr,
    };
    let later_generation = FutexKey {
        space: 1,
        generation: 2,
        addr,
    };

    const TID: usize = 7;
    enqueue(owner, TID);
    enqueue(owner, TID + 1);

    let mut out = VecDeque::new();
    take_waiters(peer_space, 8, &mut out);
    let cross_space_invisible = out.is_empty();

    out.clear();
    take_waiters(later_generation, 8, &mut out);
    let cross_generation_invisible = out.is_empty();

    out.clear();
    take_waiters(owner, 0, &mut out);
    let owner_sees_broadcast = out.len() == 2 && out[0] == TID && out[1] == TID + 1;

    // Leave no residue for the rest of the boot.
    out.clear();
    remove_waiter(owner, TID);
    remove_waiter(owner, TID + 1);

    if cross_space_invisible && cross_generation_invisible && owner_sees_broadcast {
        log::info!("S22-RV64-FUTEX-KEY: PASS");
    } else {
        log::error!(
            "S22-RV64-FUTEX-KEY: FAIL space={} generation={} broadcast={}",
            cross_space_invisible,
            cross_generation_invisible,
            owner_sees_broadcast
        );
    }
}

/// Deadline sweep arm: a parked waiter whose deadline elapsed.
///
/// Called from the scheduler's global sweep, which already holds `SCHEDULER`.
pub(crate) fn on_deadline(task: &mut super::tcb::Task, tid: usize, key: FutexKey) {
    remove_waiter(key, tid);
    task.trap_frame.regs[10] = OUTCOME_TIMED_OUT as _;
}

/// Drop a waiter whose task is leaving the wait state for any other reason
/// (exit, forced exit, fault).
pub(crate) fn on_task_leaves_wait(tid: usize) {
    let mut waiters = WAITERS.lock();
    let keys: alloc::vec::Vec<FutexKey> = waiters
        .iter()
        .filter(|(_, queue)| queue.contains(&tid))
        .map(|(key, _)| *key)
        .collect();
    for key in keys {
        if let Some(queue) = waiters.get_mut(&key) {
            queue.retain(|queued| *queued != tid);
            if queue.is_empty() {
                waiters.remove(&key);
            }
        }
    }
}
