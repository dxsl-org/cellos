//! Hot-swap support primitives shared by the supervisor-owned replacement path.
//!
//! The legacy syscall-400 whole-sequence orchestrator is retired. This module now
//! keeps only the still-live freeze state, replacement binding, mailbox cutover,
//! and `HotSwapReady` bookkeeping used by opcodes 401 and 413-422.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use types::{CellId, ViError, ViResult};

// ─── Freeze registry ─────────────────────────────────────────────────────────

/// Global freeze set — cell ids whose incoming IPC should be queued rather than
/// delivered.  Phase 02 will use this to buffer then flush messages to the new
/// cell.  In Phase 01 the queuing is a no-op stub: existing callers simply fail
/// to send if the cell is frozen (same as before).
///
/// Lock ordering: FROZEN (leaf) — never acquired while SCHEDULER is held.
static FROZEN: Spinlock<alloc::collections::BTreeSet<u64>> =
    Spinlock::new(alloc::collections::BTreeSet::new());

/// One live replacement ceiling per frozen task id.
///
/// The record is written only after the supervisor captures the task's current
/// CapSet and is cleared on every resume/kill terminal path. Missing records
/// fail closed so replacement spawn never falls back to ambient caller caps.
static SWAP_CEILINGS: Spinlock<BTreeMap<usize, crate::task::cap::CapSet>> =
    Spinlock::new(BTreeMap::new());

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    FROZEN.force_unlock();
    SWAP_CEILINGS.force_unlock();
}

/// Mark `cell_id` as frozen.  Subsequent `sys_send` calls to this cell will
/// queue the message in the task's pending queue instead of delivering it.
pub fn freeze(cell_id: CellId) {
    FROZEN.lock().insert(cell_id.0);
    log::info!("[hotswap] froze cell {}", cell_id.0);
}

/// Return true if `cell_id` is currently frozen.
pub fn is_frozen(cell_id: CellId) -> bool {
    FROZEN.lock().contains(&cell_id.0)
}

/// Remove `cell_id` from the freeze set and resume normal message delivery.
pub fn unfreeze(cell_id: CellId) {
    FROZEN.lock().remove(&cell_id.0);
    log::info!("[hotswap] unfroze cell {}", cell_id.0);
}

/// Consume the live frozen task's ceiling for one replacement spawn attempt.
///
/// A freeze record is a one-shot authority token. Consuming it prevents a
/// compromised supervisor from cloning multiple privileged replacements from
/// one frozen task. A failed spawn must be rolled back with resume/refreeze.
///
/// Lock order is `SCHEDULER -> SWAP_CEILINGS`, shared by freeze, resume, exit,
/// and consume. Holding both locks makes the live-Frozen check and token removal
/// one atomic authority decision.
pub(crate) fn take_frozen_replacement_ceiling(tid: usize) -> Option<crate::task::cap::CapSet> {
    use crate::task::tcb::TaskState;

    let scheduler = crate::task::SCHEDULER.lock();
    let task = scheduler.as_ref()?.tasks.get(&tid)?;
    if !matches!(task.state, TaskState::Frozen { .. }) {
        return None;
    }
    SWAP_CEILINGS.lock().remove(&tid)
}

pub(crate) fn clear_swap_ceiling(tid: usize) {
    SWAP_CEILINGS.lock().remove(&tid);
}

/// Bind a freshly spawned replacement to the frozen task whose one-shot
/// capability ceiling authorized its creation.
pub(crate) fn bind_replacement(source_tid: usize, target_tid: usize) -> bool {
    let mut scheduler = crate::task::SCHEDULER.lock();
    let Some(sched) = scheduler.as_mut() else {
        return false;
    };
    let source_is_frozen = sched
        .tasks
        .get(&source_tid)
        .is_some_and(|task| matches!(task.state, crate::task::tcb::TaskState::Frozen { .. }));
    if !source_is_frozen {
        return false;
    }
    let Some(target) = sched.tasks.get_mut(&target_tid) else {
        return false;
    };
    if target.hotswap_source_tid.is_some() {
        return false;
    }
    target.hotswap_source_tid = Some(source_tid);
    true
}

// ─── HotSwapReady flag ───────────────────────────────────────────────────────

/// Called from the `HotSwapReady` syscall handler (syscall 401) to record that
/// the new cell has finished deserializing state.
///
/// Sets `Task::hotswap_ready = true` for `tid` under the SCHEDULER lock.
pub fn set_task_hotswap_ready(tid: usize) {
    let mut scheduler = crate::task::SCHEDULER.lock();
    let Some(sched) = scheduler.as_mut() else {
        return;
    };
    let Some(task) = sched.tasks.get_mut(&tid) else {
        return;
    };
    task.hotswap_ready = true;
    log::info!("[hotswap] task {} signalled HotSwapReady", tid);
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn enter_frozen_state(task: &mut crate::task::Task, swap_id: u64) {
    task.hotswap_ingress_closed = false;
    task.state = crate::task::tcb::TaskState::Frozen { swap_id };
}

/// Freeze one supervisor-managed task and snapshot its capability ceiling at
/// the same scheduler transition point.
pub(crate) fn freeze_task_with_ceiling(tid: usize, swap_id: u64) -> ViResult<()> {
    use crate::task::tcb::TaskState;

    // Lock order contract: SCHEDULER -> SWAP_CEILINGS. The task transition and
    // ceiling publication are atomic to ResumeCell, KillCell, and replacement
    // spawn, so no runnable task can retain a usable freeze record.
    let mut scheduler = crate::task::SCHEDULER.lock();
    let mut ceilings = SWAP_CEILINGS.lock();
    if ceilings.contains_key(&tid) {
        return Err(ViError::AlreadyExists);
    }
    let sched = scheduler.as_mut().ok_or(ViError::NotFound)?;
    let task = sched.tasks.get_mut(&tid).ok_or(ViError::NotFound)?;
    if task.is_critical || matches!(task.state, TaskState::Frozen { .. }) {
        return Err(ViError::PermissionDenied);
    }
    let ceiling = crate::task::cap::CapSet::of_task(task);
    enter_frozen_state(task, swap_id);
    ceilings.insert(tid, ceiling);
    crate::task::hart_local::ready::remove_from_all(tid);
    Ok(())
}

/// Roll back a Frozen task to `TaskState::Ready` and re-queue it.
///
/// Called on swap abort so the old cell resumes from where it left off.
pub(crate) fn unfreeze_task(tid: usize) {
    use crate::task::tcb::TaskState;
    let mut scheduler = crate::task::SCHEDULER.lock();
    // Keep the SCHEDULER -> SWAP_CEILINGS order used by every record lifecycle.
    clear_swap_ceiling(tid);
    if let Some(sched) = scheduler.as_mut() {
        for task in sched.tasks.values_mut() {
            if task.hotswap_source_tid == Some(tid) {
                task.hotswap_source_tid = None;
            }
        }
        if let Some(task) = sched.tasks.get_mut(&tid) {
            if matches!(task.state, TaskState::Frozen { .. }) {
                task.hotswap_ingress_closed = false;
                task.state = TaskState::Ready;
                sched.push_ready(tid);
            }
        }
    }
}

/// Atomically move a frozen provider's accepted IPC to its ready replacement.
///
/// The caller must hold `SupervisorCap`. This helper owns the cutover lock
/// order: `SCHEDULER -> service registry`. A failed copy or registry compare
/// removes every target append and leaves the source mailbox open and intact.
pub(crate) fn commit_hotswap_barrier(
    source_tid: usize,
    target_tid: usize,
    service_id: u16,
) -> ViResult<()> {
    use crate::task::pending_mailbox::{PendingMsg, PendingMsgData};
    use crate::task::tcb::{TaskState, HOTSWAP_MSG_QUEUE_DEPTH};

    let mut scheduler = crate::task::SCHEDULER.lock();
    let sched = scheduler.as_mut().ok_or(ViError::NotFound)?;
    let (source_len, target_start, target_cell, wake_sender) = {
        let source = sched.tasks.get(&source_tid).ok_or(ViError::NotFound)?;
        if !matches!(source.state, TaskState::Frozen { .. }) || source.hotswap_ingress_closed {
            return Err(ViError::PermissionDenied);
        }
        let target = sched.tasks.get(&target_tid).ok_or(ViError::NotFound)?;
        if matches!(
            target.state,
            TaskState::Frozen { .. } | TaskState::Terminated
        ) || !target.hotswap_ready
            || target.hotswap_source_tid != Some(source_tid)
        {
            return Err(ViError::PermissionDenied);
        }
        if target.pending_msgs.len() + source.pending_msgs.len() > HOTSWAP_MSG_QUEUE_DEPTH {
            return Err(ViError::WouldBlock);
        }
        let wake_sender = match target.state {
            TaskState::Recv { mask, .. } => source
                .pending_msgs
                .iter()
                .find(|msg| mask == 0 || mask == msg.sender_tid)
                .map(|msg| msg.sender_tid),
            _ => None,
        };
        (
            source.pending_msgs.len(),
            target.pending_msgs.len(),
            target.cell_id.0 as usize,
            wake_sender,
        )
    };
    if !crate::cell::service_registry::paused_matches(service_id, source_tid) {
        return Err(ViError::WouldBlock);
    }

    for index in 0..source_len {
        let copied = {
            let source = sched.tasks.get(&source_tid).ok_or(ViError::NotFound)?;
            let message = &source.pending_msgs.as_slice()[index];
            PendingMsgData::try_copy(message.data.as_slice(), target_cell).map(|data| PendingMsg {
                sender_tid: message.sender_tid,
                data,
                enqueued_tick: message.enqueued_tick,
            })
        };
        let pushed = copied.and_then(|message| {
            sched
                .tasks
                .get_mut(&target_tid)
                .ok_or(())?
                .pending_msgs
                .try_push(message)
        });
        if pushed.is_err() {
            rollback_mailbox_appends(sched, target_tid, target_start);
            return Err(ViError::WouldBlock);
        }
    }

    let mut source_mailbox = {
        let source = sched.tasks.get_mut(&source_tid).ok_or(ViError::NotFound)?;
        source.hotswap_ingress_closed = true;
        core::mem::take(&mut source.pending_msgs)
    };

    if !crate::cell::service_registry::commit_paused(service_id, source_tid, target_tid) {
        if let Some(source) = sched.tasks.get_mut(&source_tid) {
            source.pending_msgs = core::mem::take(&mut source_mailbox);
            source.hotswap_ingress_closed = false;
        }
        rollback_mailbox_appends(sched, target_tid, target_start);
        return Err(ViError::WouldBlock);
    }

    if let Some(target) = sched.tasks.get_mut(&target_tid) {
        target.hotswap_source_tid = None;
    }
    if let Some(sender_tid) = wake_sender {
        let sender_context = sched
            .tasks
            .get(&sender_tid)
            .map(|sender| (sender_tid, sender.cell_id.0, sender.cell_generation));
        if let Some(target) = sched.tasks.get_mut(&target_tid) {
            target.state = TaskState::Ready;
            if let Some((sender_tid, sender_cell_id, sender_generation)) = sender_context {
                target.set_current_caller_context(sender_tid, sender_cell_id, sender_generation);
            } else {
                target.clear_current_caller_context();
            }
        }
        sched.push_ready(target_tid);
    }
    Ok(())
}

fn rollback_mailbox_appends(
    sched: &mut crate::task::scheduler::Scheduler,
    target_tid: usize,
    target_start: usize,
) {
    if let Some(target) = sched.tasks.get_mut(&target_tid) {
        while target.pending_msgs.len() > target_start {
            let last = target.pending_msgs.len() - 1;
            drop(target.pending_msgs.remove(last));
        }
    }
}

/// Terminate `tid` via the internal path, bypassing the Frozen kill-guard.
///
/// Used at the end of a successful swap to terminate the old cell.  The old cell
/// is Frozen at this point; the regular `ForceExit` syscall would reject the
/// request with `PermissionDenied`.
///
/// Mirrors the cleanup sequence from the `ForceExit` handler — must remain in sync.
pub(crate) fn exit_task_internal(tid: usize, cell_id: CellId) {
    // Resource cleanup (same order as ForceExit handler).
    crate::cell::cap_registry::CAP_TABLE
        .lock()
        .revoke_all_for(cell_id);
    crate::memory::cell_quota::deregister(cell_id);
    crate::task::drivers::driver_cell::deregister_block_driver(tid);
    crate::task::drivers::driver_cell::deregister_nic_driver(tid);
    crate::resource_registry::release_for(cell_id);
    crate::resource_registry::release_bdfs_for(tid);
    // Keyed by task id, as at every other call site: this is also the point that
    // releases quarantined frames, so a cell id here would look up nothing and
    // leak them on every swap. The two agree for a loader-spawned cell, which is
    // what let the mismatch go unnoticed.
    crate::task::drivers::iommu::cleanup_cell(tid as u64);
    // The completed IOTLB teardown is the acknowledgement for ordinary DMA pins.
    // Drop them before grant reap so those frames are freed instead of entering
    // an orphaned quarantine. VFS request leases remain exact-release scoped.
    crate::task::syscall::release_acked_frames(tid);

    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        // 0xAAAA_AAAA = hot-swap sentinel (distinguishes from clean exit 0 or watchdog MAX).
        sched.exit_task(tid, 0xAAAA_AAAAusize);
    }

    // Grant pages are freed outside SCHEDULER lock (lock-order safety).
    // SAFETY: reap_grants_for_task is pub(crate); hotswap.rs is in the same crate.
    crate::task::syscall::reap_grants_for_task(tid);

    crate::audit::log_event(
        crate::audit::AuditEvent::CellExit,
        &crate::audit::encode_u32x2(tid as u32, 0xAA00_0000u32), // hot-swap marker
    );
}
