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

/// One replacement ceiling per exact frozen task incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CeilingState {
    Available {
        ceiling: crate::task::cap::CapSet,
        generation: u64,
        swap_id: u64,
        freeze_nonce: u64,
    },
    Reserved {
        generation: u64,
        swap_id: u64,
        freeze_nonce: u64,
    },
}

static SWAP_CEILINGS: Spinlock<BTreeMap<usize, CeilingState>> = Spinlock::new(BTreeMap::new());
static NEXT_FREEZE_NONCE: Spinlock<u64> = Spinlock::new(1);

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    FROZEN.force_unlock();
    SWAP_CEILINGS.force_unlock();
    NEXT_FREEZE_NONCE.force_unlock();
}

fn next_freeze_nonce() -> u64 {
    let mut next = NEXT_FREEZE_NONCE.lock();
    let nonce = *next;
    *next = next.checked_add(1).expect("freeze nonce space exhausted");
    nonce
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

/// Exclusive rollback owner for one exact frozen task incarnation.
pub(crate) struct ReplacementReservation {
    source_tid: usize,
    source_generation: u64,
    swap_id: u64,
    freeze_nonce: u64,
    ceiling: Option<crate::task::cap::CapSet>,
}

impl ReplacementReservation {
    pub(crate) fn ceiling(&self) -> crate::task::cap::CapSet {
        self.ceiling.expect("live replacement reservation")
    }

    /// Check the binding while the caller holds `SCHEDULER`.
    pub(crate) fn can_bind(&self, sched: &crate::task::scheduler::Scheduler) -> bool {
        let source_matches = sched.tasks.get(&self.source_tid).is_some_and(|task| {
            task.cell_generation == self.source_generation
                && matches!(task.state, crate::task::tcb::TaskState::Frozen { swap_id } if swap_id == self.swap_id)
        });
        source_matches
            && matches!(
                SWAP_CEILINGS.lock().get(&self.source_tid),
                Some(CeilingState::Reserved {
                    generation,
                    swap_id,
                    freeze_nonce,
                }) if *generation == self.source_generation
                    && *swap_id == self.swap_id
                    && *freeze_nonce == self.freeze_nonce
            )
    }

    /// Consume the ceiling and install the source binding on an unpublished TCB.
    pub(crate) fn commit_into(mut self, task: &mut crate::task::Task) {
        task.hotswap_source_tid = Some(self.source_tid);
        self.ceiling = None;
        let removed = SWAP_CEILINGS.lock().remove(&self.source_tid);
        debug_assert!(matches!(
            removed,
            Some(CeilingState::Reserved {
                generation,
                swap_id,
                freeze_nonce,
            }) if generation == self.source_generation
                && swap_id == self.swap_id
                && freeze_nonce == self.freeze_nonce
        ));
    }

    /// Construct a deliberately unbindable reservation for publication-boundary
    /// tests. It owns no ceiling, so dropping it cannot mutate swap state.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn invalid_for_test() -> Self {
        Self {
            source_tid: usize::MAX,
            source_generation: 0,
            swap_id: 0,
            freeze_nonce: 0,
            ceiling: None,
        }
    }
}

impl Drop for ReplacementReservation {
    fn drop(&mut self) {
        let Some(ceiling) = self.ceiling.take() else {
            return;
        };
        let mut ceilings = SWAP_CEILINGS.lock();
        if matches!(
            ceilings.get(&self.source_tid),
            Some(CeilingState::Reserved {
                generation,
                swap_id,
                freeze_nonce,
            }) if *generation == self.source_generation
                && *swap_id == self.swap_id
                && *freeze_nonce == self.freeze_nonce
        ) {
            ceilings.insert(
                self.source_tid,
                CeilingState::Available {
                    ceiling,
                    generation: self.source_generation,
                    swap_id: self.swap_id,
                    freeze_nonce: self.freeze_nonce,
                },
            );
        }
    }
}

/// Reserve the live frozen task's one-shot replacement ceiling.
pub(crate) fn reserve_frozen_replacement(tid: usize) -> Option<ReplacementReservation> {
    use crate::task::tcb::TaskState;

    let scheduler = crate::task::SCHEDULER.lock();
    let task = scheduler.as_ref()?.tasks.get(&tid)?;
    let TaskState::Frozen { swap_id } = task.state else {
        return None;
    };
    let generation = task.cell_generation;
    let mut ceilings = SWAP_CEILINGS.lock();
    let (ceiling, freeze_nonce) = match ceilings.get(&tid).copied()? {
        CeilingState::Available {
            ceiling,
            generation: available_generation,
            swap_id: available_swap_id,
            freeze_nonce,
        } if available_generation == generation && available_swap_id == swap_id => {
            (ceiling, freeze_nonce)
        }
        CeilingState::Available { .. } | CeilingState::Reserved { .. } => return None,
    };
    ceilings.insert(
        tid,
        CeilingState::Reserved {
            generation,
            swap_id,
            freeze_nonce,
        },
    );
    Some(ReplacementReservation {
        source_tid: tid,
        source_generation: generation,
        swap_id,
        freeze_nonce,
        ceiling: Some(ceiling),
    })
}

pub(crate) fn clear_swap_ceiling(tid: usize) {
    SWAP_CEILINGS.lock().remove(&tid);
}

#[cfg(feature = "test-hooks")]
/// One frozen swap ceiling: `(tid, Some((ceiling, generation, swap_id, freeze_nonce)))`.
#[cfg(feature = "test-hooks")]
pub(crate) type CeilingSnapshotEntry =
    (usize, Option<(crate::task::cap::CapSet, u64, u64, u64)>);

#[cfg(feature = "test-hooks")]
pub(crate) fn replacement_ceiling_snapshot(
) -> alloc::vec::Vec<CeilingSnapshotEntry> {
    SWAP_CEILINGS
        .lock()
        .iter()
        .map(|(tid, state)| {
            let reservation = match state {
                CeilingState::Available {
                    ceiling,
                    generation,
                    swap_id,
                    freeze_nonce,
                } => Some((*ceiling, *generation, *swap_id, *freeze_nonce)),
                CeilingState::Reserved { .. } => None,
            };
            (*tid, reservation)
        })
        .collect()
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
    let generation = task.cell_generation;
    let freeze_nonce = next_freeze_nonce();
    enter_frozen_state(task, swap_id);
    ceilings.insert(
        tid,
        CeilingState::Available {
            ceiling,
            generation,
            swap_id,
            freeze_nonce,
        },
    );
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
            let wire = match &message.wire {
                Some(w) => w.try_clone().ok(),
                None => None,
            };
            PendingMsgData::try_copy(message.payload(), target_cell).map(|data| PendingMsg {
                sender_tid: message.sender_tid,
                data,
                wire,
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
                target.set_received_caller_context(sender_tid, sender_cell_id, sender_generation);
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
/// Uses the same scheduler retirement funnel as every other exit path. The
/// `CellId` is retained only to preserve this internal call shape; it must not
/// authorize Cell-wide cleanup for a worker target.
pub(crate) fn exit_task_internal(tid: usize, _cell_id: CellId) {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        // 0xAAAA_AAAA = hot-swap sentinel (distinguishes from clean exit 0 or watchdog MAX).
        sched.exit_task(tid, 0xAAAA_AAAAusize);
    }

    crate::audit::log_event(
        crate::audit::AuditEvent::CellExit,
        &crate::audit::encode_u32x2(tid as u32, 0xAA00_0000u32), // hot-swap marker
    );
}

#[cfg(test)]
mod tests {
    use super::{CeilingState, ReplacementReservation, SWAP_CEILINGS};
    use crate::task::cap::CapSet;

    #[test]
    fn stale_reservation_cannot_restore_a_new_freeze() {
        const TID: usize = usize::MAX - 1;
        let old = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        let new = CapSet {
            network: true,
            ..CapSet::EMPTY
        };
        SWAP_CEILINGS.lock().insert(
            TID,
            CeilingState::Reserved {
                generation: 3,
                swap_id: 9,
                freeze_nonce: 41,
            },
        );
        let stale = ReplacementReservation {
            source_tid: TID,
            source_generation: 3,
            swap_id: 9,
            freeze_nonce: 41,
            ceiling: Some(old),
        };
        SWAP_CEILINGS.lock().insert(
            TID,
            CeilingState::Available {
                ceiling: new,
                generation: 3,
                swap_id: 9,
                freeze_nonce: 42,
            },
        );

        drop(stale);

        assert!(matches!(
            SWAP_CEILINGS.lock().remove(&TID),
            Some(CeilingState::Available {
                ceiling,
                generation: 3,
                swap_id: 9,
                freeze_nonce: 42,
            }) if ceiling == new
        ));
    }
}
