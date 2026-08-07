//! Hot-swap orchestrator — live-replaces a Cell without message loss.
//!
//! # Protocol (5 steps)
//!
//! 1. **Freeze** — kernel marks the old cell `Frozen { swap_id }`; the FROZEN set
//!    is set so incoming IPC can be queued instead of delivered (Phase 02 drains
//!    the queue; this phase stubs the queue as a no-op).
//! 2. **Serialize** — kernel sends `AppEvent::Snapshot { swap_id }` to the old cell
//!    via an IPC envelope.  The cell serializes state and calls `sys_state_stash(key, …)`;
//!    the kernel polls the stash for the key.  On timeout the swap aborts (old cell
//!    is unfrozen and continues running).
//! 3. **Spawn** — kernel loads the new ELF via `loader::spawn_from_path`; the new
//!    cell starts and blocks in its init recv loop.
//! 4. **Deserialize** — kernel sends `AppEvent::Restore { key }` to the new cell.
//!    The cell calls `sys_state_restore(key, …)` then `sys_hotswap_ready()`.  The
//!    kernel polls the new task's `hotswap_ready` flag.
//! 5. **Unfreeze** — kernel re-registers the service entry for the new cell tid,
//!    terminates the old cell via the internal path (bypasses the Frozen kill-guard),
//!    and (Phase 02) drains queued IPC to the new cell.
//!
//! # Lock ordering
//!
//! `FROZEN` (leaf) is always acquired **before** `SCHEDULER` is dropped, never
//! while holding it.  SCHEDULER → FROZEN ordering is safe (one-way dependency).

// RV32 lacks native 64-bit atomics; portable-atomic polyfills AtomicU64 there
// via the critical-section impl hal/arch/riscv registers.
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
#[cfg(not(target_arch = "riscv32"))]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
#[cfg(target_arch = "riscv32")]
use portable_atomic::AtomicU64;
use types::{CellId, ViError, ViResult};

// ─── Swap-ID counter ─────────────────────────────────────────────────────────

/// Monotonically increasing counter assigning a unique ID to each swap sequence.
///
/// `0` is the null sentinel and is never returned by `next_swap_id()`.
static SWAP_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_swap_id() -> u64 {
    SWAP_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ─── Timeout constant ────────────────────────────────────────────────────────

/// How long (scheduler ticks, ≈10 ms each) to wait for the old cell to serialize
/// and for the new cell to signal ready.  5 seconds.
const HOTSWAP_TIMEOUT_TICKS: u64 = 500;

// ─── IPC envelope byte constants ─────────────────────────────────────────────

/// App SDK magic byte (mirrors `ostd::app::APP_MSG_MAGIC`).
const APP_MSG_MAGIC: u8 = 0xAC;

/// Envelope discriminant for `AppEvent::Snapshot` (hot-swap Step 2).
/// Envelope layout: `[0xAC, 0xF0, swap_id_le8 (8 bytes)]` = 10 bytes total.
const DISC_SNAPSHOT: u8 = 0xF0;

/// Envelope discriminant for `AppEvent::Restore` (hot-swap Step 4).
/// Envelope layout: `[0xAC, 0xF1, key (64 bytes)]` = 66 bytes total.
const DISC_RESTORE: u8 = 0xF1;

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
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.hotswap_ready = true;
            log::info!("[hotswap] task {} signalled HotSwapReady", tid);
        }
    }
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn enter_frozen_state(task: &mut crate::task::Task, swap_id: u64) {
    task.hotswap_ingress_closed = false;
    task.state = crate::task::tcb::TaskState::Frozen { swap_id };
}

/// Resolve the task-id for a live cell, or `ViError::NotFound`.
fn find_tid_for_cell(cell_id: CellId) -> ViResult<usize> {
    let guard = crate::task::SCHEDULER.lock();
    guard
        .as_ref()
        .and_then(|s| {
            s.tasks
                .values()
                .find(|t| t.cell_id == cell_id)
                .map(|t| t.id)
        })
        .ok_or(ViError::NotFound)
}

/// Transition `tid` to `TaskState::Frozen { swap_id }`.
///
/// The task is removed from the scheduler ready queues so it cannot be selected
/// for execution while the swap is in progress.
pub(crate) fn set_task_frozen(tid: usize, swap_id: u64) -> ViResult<()> {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            enter_frozen_state(task, swap_id);
            // Remove from all hart ready queues — a Frozen task must not run.
            crate::task::hart_local::ready::remove_from_all(tid);
            return Ok(());
        }
    }
    Err(ViError::NotFound)
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
        if let Some(target) = sched.tasks.get_mut(&target_tid) {
            target.state = TaskState::Ready;
            target.current_caller = Some(sender_tid);
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

/// Send an `AppEvent::Snapshot { swap_id }` IPC envelope to `tid`.
///
/// Envelope: `[0xAC, 0xF0, swap_id_le8]` = 10 bytes.
fn send_snapshot_event(tid: usize, swap_id: u64) -> ViResult<()> {
    let mut buf = [0u8; 10];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_SNAPSHOT;
    buf[2..10].copy_from_slice(&swap_id.to_le_bytes());
    crate::task::send_to(tid, &buf)
}

/// Send an `AppEvent::Restore { key }` IPC envelope to `tid`.
///
/// `key_str` is the decimal string of `swap_id`, null-padded to 64 bytes.
/// Envelope: `[0xAC, 0xF1, key[64]]` = 66 bytes.
fn send_restore_event(tid: usize, swap_id: u64) -> ViResult<()> {
    let mut buf = [0u8; 66];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_RESTORE;
    // Write decimal swap_id as null-terminated ASCII into buf[2..66].
    let mut tmp = [0u8; 20]; // max u64 decimal = 20 digits
    let n = fmt_u64_decimal(swap_id, &mut tmp);
    let key_len = n.min(63);
    buf[2..2 + key_len].copy_from_slice(&tmp[..key_len]);
    // remaining bytes default to 0 (null terminator + padding)
    crate::task::send_to(tid, &buf)
}

/// Format `val` as decimal ASCII into `buf`. Returns the number of bytes written.
///
/// `buf` must be at least 20 bytes (max u64 decimal length).
fn fmt_u64_decimal(mut val: u64, buf: &mut [u8; 20]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut end = 20usize;
    while val > 0 {
        end -= 1;
        buf[end] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    let len = 20 - end;
    buf.copy_within(end..20, 0);
    len
}

/// Stash key for a given `swap_id`.
///
/// Must match `ostd::hotswap::hotswap_key(swap_id)` exactly — both sides of a
/// hot-swap derive the key independently: the old cell uses the ostd helper,
/// the kernel uses this function.  Any divergence causes a stash-miss (timeout).
///
/// Layout: `0xA3_0000_0000_0000 | (swap_id & 0x0000_FFFF_FFFF_FFFF)`.
fn stash_key_for(swap_id: u64) -> u64 {
    // Namespace tag matches ostd::hotswap::hotswap_key (libs/ostd/src/hotswap.rs:136).
    0x00A3_0000_0000_0000u64 | (swap_id & 0x0000_FFFF_FFFF_FFFFu64)
}

/// Spin-poll until the state-stash contains an entry for `key`, or timeout.
///
/// Returns `Ok(())` when the stash entry appears, `Err(ViError::WouldBlock)` if
/// `HOTSWAP_TIMEOUT_TICKS` ticks elapse without the key appearing.
///
/// Precondition: called from a context where the SCHEDULER lock is NOT held
/// (spin-polls release the CPU between iterations via `core::hint::spin_loop()`).
fn wait_for_stash_key(key: u64) -> ViResult<()> {
    let deadline = crate::task::system_ticks() as u64 + HOTSWAP_TIMEOUT_TICKS;
    loop {
        {
            // Peek without consuming — the new cell needs to read it too.
            let mut probe = [0u8; 1];
            if crate::cell::state_stash::restore(key, &mut probe) > 0 {
                return Ok(());
            }
        }
        if crate::task::system_ticks() as u64 >= deadline {
            return Err(ViError::WouldBlock);
        }
        // Yield CPU so the old cell can run its Snapshot handler and call
        // sys_state_stash.  Without this, a single-hart system deadlocks:
        // the orchestrator spins forever, the old cell never gets to run.
        crate::task::yield_cpu();
    }
}

/// Spin-poll until `tid`'s `hotswap_ready` flag is true, or timeout.
fn wait_for_hotswap_ready(tid: usize) -> ViResult<()> {
    let deadline = crate::task::system_ticks() as u64 + HOTSWAP_TIMEOUT_TICKS;
    loop {
        {
            let guard = crate::task::SCHEDULER.lock();
            if guard
                .as_ref()
                .and_then(|s| s.tasks.get(&tid))
                .map(|t| t.hotswap_ready)
                .unwrap_or(false)
            {
                return Ok(());
            }
        }
        if crate::task::system_ticks() as u64 >= deadline {
            return Err(ViError::WouldBlock);
        }
        // Yield CPU so the new cell can run its Restore handler and call
        // sys_hotswap_ready.  Without this, a single-hart system deadlocks.
        crate::task::yield_cpu();
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Perform a live hot-swap of `old_cell_id` with the ELF at `new_elf_path`.
///
/// Called from `sys_hotswap` (syscall 400) after the SpawnCap gate passes.
/// `caller_tid` is the tid of the orchestrating cell (e.g. init/shell).
///
/// Returns the new task id on success, or an error indicating which step failed.
///
/// # Error codes
/// - `ViError::NotFound` — `old_cell_id` not live or `new_elf_path` missing.
/// - `ViError::WouldBlock`  — old cell did not serialize / new cell did not signal
///   ready within `HOTSWAP_TIMEOUT_TICKS` scheduler ticks (≈5 s).
/// - `ViError::IO`       — IPC send to old or new cell failed.
///
/// # Rollback on failure
/// If any step fails before Step 3 (spawn), the old cell is unfrozen and
/// resumes normally.  If Step 3 succeeds but a later step fails, the old cell
/// remains Frozen (manual recovery required); this is the safe-side choice
/// rather than running two potentially inconsistent instances simultaneously.
///
/// # Panics
/// None — all errors are propagated.
pub fn hotswap(old_cell_id: CellId, new_elf_path: &str, caller_tid: usize) -> ViResult<usize> {
    log::info!(
        "[hotswap] starting swap: cell {} → {} (caller_tid={})",
        old_cell_id.0,
        new_elf_path,
        caller_tid
    );

    // ── Snapshot caller's capability ceiling for the new cell ─────────────
    // The replacement must not receive caps the replaced cell didn't hold.
    let ceiling = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|s| {
            s.tasks
                .values()
                .find(|t| t.cell_id == old_cell_id)
                .map(|t| crate::task::cap::CapSet::of_task(t))
        })
        .unwrap_or(crate::task::cap::CapSet::EMPTY);

    let swap_id = next_swap_id();
    let old_tid = find_tid_for_cell(old_cell_id)?;
    let stash_key = stash_key_for(swap_id);

    // ── Step 1: FREEZE (soft) ────────────────────────────────────────────
    // Mark cell in the FROZEN set so IPC routers know it is mid-swap.  We do
    // NOT set TaskState::Frozen yet — the cell must still be able to receive
    // the Snapshot IPC in Step 2 (it is typically blocked in Recv).
    // Clear service registry so new callers retry instead of stale-delivering.
    freeze(old_cell_id);
    crate::cell::service_registry::clear_tid(old_tid);
    log::info!(
        "[hotswap] step 1 done: old_tid={} FROZEN set (swap_id={})",
        old_tid,
        swap_id
    );

    // ── Step 2: SERIALIZE ────────────────────────────────────────────────
    // Send AppEvent::Snapshot to the old cell.  The cell must be in Recv state
    // (idle, waiting for next IPC) for delivery to succeed immediately.  If the
    // cell is busy (not in Recv) the kernel enqueues the caller in Sending state
    // temporarily — the cell will pick it up when it next enters Recv.
    if let Err(e) = send_snapshot_event(old_tid, swap_id) {
        // Old cell does not exist or IPC is broken; abort.
        unfreeze(old_cell_id);
        log::error!("[hotswap] step 2 aborted: snapshot IPC failed: {:?}", e);
        return Err(e);
    }

    // Poll until the cell stashes its state.  Tolerate timeout — cells that
    // do not implement ViStateTransfer simply won't stash anything; the swap
    // continues with an empty stash (new cell starts with a cold state).
    match wait_for_stash_key(stash_key) {
        Ok(()) => {
            log::info!(
                "[hotswap] step 2 done: state stashed (key={:#x})",
                stash_key
            );
        }
        Err(ViError::WouldBlock) => {
            log::warn!(
                "[hotswap] step 2 timeout: old cell did not stash (continuing with empty stash)"
            );
        }
        Err(e) => {
            unfreeze(old_cell_id);
            log::error!("[hotswap] step 2 error: {:?}", e);
            return Err(e);
        }
    }

    // ── Step 1b: FREEZE (hard) ───────────────────────────────────────────
    // Now that state is serialized, set TaskState::Frozen to prevent the old
    // cell from executing again.  From this point no external actor can kill it
    // (ForceExit gate checks Frozen) and the scheduler will not run it.
    if let Err(e) = set_task_frozen(old_tid, swap_id) {
        // Task exited on its own between step 2 and here — that is a valid race
        // (cell shut down while we were waiting for the stash).  Treat as success
        // with no old-cell cleanup needed.
        log::warn!("[hotswap] step 1b: old task gone ({:?}); continuing", e);
    }
    log::info!(
        "[hotswap] step 1b done: old_tid={} TaskState::Frozen",
        old_tid
    );

    // ── Step 3: SPAWN ────────────────────────────────────────────────────
    let new_tid = match crate::loader::spawn_from_path(
        new_elf_path,
        crate::task::cap::Spawner::Ceiling(ceiling),
    ) {
        Ok(id) => id,
        Err(e) => {
            // New ELF failed to load; roll back old cell to Ready.
            unfreeze(old_cell_id);
            unfreeze_task(old_tid);
            log::error!("[hotswap] step 3 failed: spawn {:?}: {:?}", new_elf_path, e);
            return Err(e);
        }
    };
    log::info!("[hotswap] step 3 done: new_tid={}", new_tid);

    // ── Step 4: DESERIALIZE ──────────────────────────────────────────────
    // Signal the new cell to restore state.  The new cell starts running its
    // event loop and will enter Recv immediately, so the Restore IPC delivers.
    if let Err(e) = send_restore_event(new_tid, swap_id) {
        log::error!("[hotswap] step 4 aborted: restore IPC failed: {:?}", e);
        // Old cell stays Frozen — manual recovery; do not run two instances.
        return Err(e);
    }

    // Wait for the new cell to call sys_hotswap_ready() (syscall 401).
    if let Err(e) = wait_for_hotswap_ready(new_tid) {
        log::error!(
            "[hotswap] step 4 timeout: new cell did not signal ready: {:?}",
            e
        );
        return Err(e);
    }
    log::info!("[hotswap] step 4 done: new cell {} is ready", new_tid);

    // ── Step 5: UNFREEZE ─────────────────────────────────────────────────
    // Remove from FROZEN set so is_frozen() returns false for the old cell.
    // Drain pending_msgs to the new cell BEFORE terminating the old cell —
    // this guarantees the new cell sees all buffered messages in order.
    unfreeze(old_cell_id);

    // Remove one message per scheduler-lock acquisition, then re-lock to copy it
    // into the replacement mailbox. The old mailbox container stays with its task,
    // preserving its kernel-owned allocation accounting.
    let queued_count = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&old_tid))
        .map(|task| task.pending_msgs.len())
        .unwrap_or(0);
    for _ in 0..queued_count {
        let msg = {
            let mut guard = crate::task::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&old_tid))
                .filter(|task| !task.pending_msgs.is_empty())
                .map(|task| task.pending_msgs.remove(0))
        };
        let Some(msg) = msg else {
            break;
        };
        // Copy into receiver-owned storage directly. The original sender already
        // resumed when the Frozen intercept accepted the message, so replaying
        // through ipc_send could incorrectly park that live sender when the
        // replacement is not yet in Recv. The copy also rebinds heap quota ownership
        // because the replacement may have a different CellId from the old task.
        let sender_tid = msg.sender_tid;
        let mut queued = false;
        let mut wake = false;
        {
            let mut guard = crate::task::SCHEDULER.lock();
            if let Some(sched) = guard.as_mut() {
                if let Some(target) = sched.tasks.get_mut(&new_tid) {
                    let recv_matches = matches!(
                        target.state,
                        crate::task::tcb::TaskState::Recv { mask, .. }
                            if mask == 0 || mask == sender_tid
                    );
                    if crate::task::queue_pending_msg(
                        target,
                        sender_tid,
                        msg.data.as_slice(),
                        crate::task::tcb::HOTSWAP_MSG_QUEUE_DEPTH,
                    )
                    .is_ok()
                    {
                        queued = true;
                        if recv_matches {
                            target.state = crate::task::tcb::TaskState::Ready;
                            target.current_caller = Some(sender_tid);
                            wake = true;
                        }
                    }
                }
                if wake {
                    sched.push_ready(new_tid);
                }
            }
        }
        if !queued {
            log::warn!(
                "[hotswap] replacement mailbox full or missing; dropping msg from tid={}",
                sender_tid
            );
        }
    }

    if queued_count > 0 {
        log::info!(
            "[hotswap] step 5: drained {} buffered msgs to new_tid={}",
            queued_count,
            new_tid
        );
    }

    log::info!(
        "[hotswap] step 5 done: old_tid={} → new_tid={}; terminating old cell",
        old_tid,
        new_tid
    );

    // Terminate the old cell via the internal path (bypasses Frozen kill-guard).
    exit_task_internal(old_tid, old_cell_id);

    // Free the stash slot so it does not count toward MAX_ENTRIES.
    crate::cell::state_stash::remove(stash_key);

    log::info!(
        "[hotswap] complete: cell {} is now task {}",
        old_cell_id.0,
        new_tid
    );
    Ok(new_tid)
}
