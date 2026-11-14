// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: misc (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, grant::*, ipc::*, spawn::*, vm::*};

/// The live private root of `tid`, if it is a domain right now.
#[cfg(all(feature = "native-domains", cellos_wide))]
pub(super) fn live_domain_space(tid: usize) -> Option<DomainRoot> {
    task_domain_space(tid).filter(|space| space.is_live())
}

#[cfg(not(all(feature = "native-domains", cellos_wide)))]
pub(super) fn live_domain_space(_tid: usize) -> Option<DomainRoot> {
    None
}

/// Clone the lifecycle record a row carries, if any.
///
/// The lifecycle exists on one target tuple only; on every other target the
/// record type has no inhabitants, so this is the sole producer of `None`.
#[cfg(cellos_domain_grants)]
pub(super) fn clone_domain_record(domain: &Option<DomainRecord>) -> Option<DomainRecord> {
    domain.clone()
}

#[cfg(not(cellos_domain_grants))]
pub(super) fn clone_domain_record(_domain: &Option<DomainRecord>) -> Option<DomainRecord> {
    None
}

/// Does a row's `domain` slot carry exactly `record` (the same allocation)?
#[cfg(cellos_domain_grants)]
pub(super) fn carries_record(domain: &Option<DomainRecord>, record: &DomainRecord) -> bool {
    domain
        .as_ref()
        .is_some_and(|carried| Arc::ptr_eq(carried, record))
}

#[cfg(not(cellos_domain_grants))]
pub(super) fn carries_record(_domain: &Option<DomainRecord>, _record: &DomainRecord) -> bool {
    false
}

/// Remove the row at `key` only while `carries` holds for it; `true` when this
/// call removed it.
///
/// A domain record's revoke is idempotent, so every revoker that reaches it —
/// `GrantFree`, `GrantUnregister`, the owner's exit, the deferred sweep on any
/// hart, `CapRevoke`'s owned-grant reclaim — can see it succeed. The frames
/// belong to the one revoker whose removal took the row: a caller that gets
/// `false` must not release them, because another revoker already did (and the
/// key may by now name a new grant over reused frames, which this leaves alone).
pub(super) fn remove_row_if<G>(
    map: Option<&mut BTreeMap<usize, G>>,
    key: usize,
    carries: impl Fn(&G) -> bool,
) -> bool {
    let Some(map) = map else {
        return false;
    };
    if !map.get(&key).is_some_and(carries) {
        return false;
    }
    map.remove(&key).is_some()
}

/// Complete a revoke that succeeded: take `key`'s row out of `table` only while
/// it still carries the revoked record ([`remove_row_if`]), release the table
/// lock, and only then — and only when this call took the row — run `release`,
/// which returns the frames. `true` when this call took the row.
///
/// The one shape of the single-free rule, shared by every revoker (`GrantFree`,
/// `GrantUnregister`, the owner's exit, the deferred sweep): a revoker that lost
/// the race, or whose key now names a new grant over reused frames, releases
/// nothing and leaves that row alone. `release` runs with no table lock held, so
/// it may take `FRAME_ALLOCATOR` and `KERNEL_ROOT`.
pub(super) fn release_if_row_taken<G>(
    table: &Spinlock<Option<BTreeMap<usize, G>>>,
    key: usize,
    carries: impl Fn(&G) -> bool,
    release: impl FnOnce(),
) -> bool {
    let taken = {
        let mut rows = table.lock();
        remove_row_if(rows.as_mut(), key, carries)
    };
    if taken {
        release();
    }
    taken
}

/// Build the lifecycle record for a domain owner that has just published its
/// own mapping of `backing`; the record owns the run from here on. Absent —
/// `None` — wherever the lifecycle does not exist.
#[cfg(cellos_domain_grants)]
pub(super) fn new_domain_record(
    space: &DomainRoot,
    backing: crate::memory::frame_run::FrameRun,
    size: usize,
) -> Option<DomainRecord> {
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    Some(Arc::new(crate::task::domain_grant::DomainGrant::new(
        space, backing, size,
    )))
}

/// No domain owner gets a backing run where the lifecycle does not exist, so
/// this never receives one; if it did, the frames are kept (leaked, counted),
/// never freed.
#[cfg(not(cellos_domain_grants))]
pub(super) fn new_domain_record(
    _space: &DomainRoot,
    backing: crate::memory::frame_run::FrameRun,
    _size: usize,
) -> Option<DomainRecord> {
    crate::memory::frame_run::PENDING.leak(backing, "no domain lifecycle on this target");
    None
}

/// Drop the receiver half of a record left unshared by receiver death.
#[cfg(cellos_domain_grants)]
pub(super) fn drain_domain_receiver(record: &DomainRecord) {
    // Test-hooks CONTROL (`GuardedCheck::DeathDrainsReceiver`): the
    // receiver-death fixture must catch a death that skips this drain.
    #[cfg(feature = "test-hooks")]
    if crate::task::fixture_controls::disabled(
        crate::task::fixture_controls::GuardedCheck::DeathDrainsReceiver,
    ) {
        return;
    }
    if record.drain_receiver().is_err() {
        log::warn!(
            "[grant] receiver exit: domain receiver PTE invalidation unacknowledged; record keeps \
             the receiver mapping until the next share or teardown retries"
        );
    }
}

#[cfg(not(cellos_domain_grants))]
pub(super) fn drain_domain_receiver(_record: &DomainRecord) {}

/// Withhold `run` instead of letting the reaper release it, while an in-flight
/// operation still holds part of its range.
///
/// Pin lookup and the quarantine transfer are one registry transaction. This
/// is essential for VFS leases: if the holder completes the exact lease before
/// the transaction, the reaper must free these frames rather than install an
/// orphaned release key; if the transfer wins, that exact release owns it.
///
/// Withheld frames stay mapped (the holder may still use them) as a
/// `Quarantined(Pinned)` run in the frame-run quarantine until the key's event,
/// with the kernel alias their owner gave the run.
///
/// Returns the run when nothing protects it: the caller releases it. A hold
/// that could not be recorded (no heap) returns `None` too: leaking the frames
/// is the only safe answer left, and the leak is counted in [`leaked_pages`].
pub(super) fn withhold_if_pinned(
    run: crate::memory::frame_run::FrameRun,
) -> Option<crate::memory::frame_run::FrameRun> {
    let (base, pages) = (run.base(), run.pages());
    match crate::memory::pin::withhold_pinned_run(run) {
        crate::memory::pin::FrameTransfer::Free(run) => Some(run),
        crate::memory::pin::FrameTransfer::Withheld => None,
        crate::memory::pin::FrameTransfer::Full => {
            LEAKED_PAGES.fetch_add(pages, core::sync::atomic::Ordering::Relaxed);
            log::error!(
                "[grant] withheld run not recorded (no heap): leaking {pages} page(s) at {base:#x} protected by a live pin"
            );
            None
        }
    }
}

/// Syscalls whose errors use the typed sentinels of [`encode_ipc_result`]
/// (`usize::MAX - 1..=4`) instead of the single generic error.
pub(super) fn has_typed_errors(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::IpcSubmit { .. }
            | Syscall::IpcTake { .. }
            | Syscall::IpcWait { .. }
            | Syscall::IpcCancel { .. }
            | Syscall::IpcCurrent
            | Syscall::IpcReply { .. }
            | Syscall::QueryRetirement { .. }
            | Syscall::LookupBlockRoute { .. }
            | Syscall::QueryMachineFacts { .. }
            | Syscall::RegisterBlockDriver { .. }
            | Syscall::ProbeVirtioMmio { .. }
    )
}

/// Tighter cap for `Syscall::Log` since the kernel holds locks while printing.
pub(super) const MAX_LOG_MSG: usize = 4096;

/// Return the live Cell-generation binding for a task.
///
/// Callers holding a grant table preserve the documented
/// `*_GRANT_TABLE → SCHEDULER` order while taking this snapshot.
pub(super) fn live_task_binding(task_id: usize) -> Option<(CellId, u64)> {
    let scheduler = crate::task::SCHEDULER.lock();
    scheduler
        .as_ref()?
        .tasks
        .get(&task_id)
        .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
        .map(|task| (task.cell_id, task.cell_generation))
}

#[cfg(all(feature = "native-domains", cellos_wide))]
pub(super) fn record_end_containing(base: usize, size: usize, ptr: usize) -> Option<usize> {
    let end = base.checked_add(size)?;
    (base <= ptr && ptr < end).then_some(end)
}

/// Preflight an exact caller-owned output span without moving user bytes.
///
/// SAS ownership comes only from caller stack/root-segment/grant records.
/// Callers that consume or otherwise act on external state must repeat the
/// ownership check under their operation-specific final-commit lock set.
pub(super) fn preflight_user_output(
    caller_id: usize,
    ptr: usize,
    len: usize,
) -> Result<(), SyscallError> {
    let view = caller_copy_view(caller_id)?;
    #[cfg(all(feature = "native-domains", cellos_wide))]
    if view.is_sas() {
        // Lease order: PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER. This
        // extends the established grant-table → scheduler order used by
        // GrantSlice; no removal path may unmap or reuse a contributing record
        // while this final-authorization lock set is held.
        let page_grants = grant_table_lock().lock();
        let registered_grants = reg_grant_table_lock().lock();
        let scheduler = crate::task::SCHEDULER.lock();
        if !scheduler.as_ref().is_some_and(|sched| {
            sas_caller_owned_span(
                sched,
                page_grants.as_ref(),
                registered_grants.as_ref(),
                caller_id,
                ptr,
                len,
            )
        }) {
            return Err(SyscallError::InvalidInput);
        }
    }
    view.validate_writable(ptr, len)
        .map_err(|_| SyscallError::InvalidInput)
}

/// Arm the final-write/unregister serialization fixture for one caller.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_arm_getrandom_revoke_race(caller_id: usize) {
    use core::sync::atomic::Ordering;

    GETRANDOM_RACE_ENTERED.store(false, Ordering::Release);
    GETRANDOM_RACE_PROBED_BUSY.store(false, Ordering::Release);
    GETRANDOM_RACE_PROBE_TIMED_OUT.store(false, Ordering::Release);
    GETRANDOM_RACE_DONE.store(false, Ordering::Release);
    GETRANDOM_RACE_NO_EARLY_DONE.store(false, Ordering::Release);
    GETRANDOM_RACE_ARMED_CALLER.store(caller_id, Ordering::Release);
}

/// Report whether GetRandom's final write has acquired its ownership locks.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_getrandom_revoke_race_entered() -> bool {
    GETRANDOM_RACE_ENTERED.load(core::sync::atomic::Ordering::Acquire)
}

/// Probe the registered-grant lock used by the production unregister helper.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_probe_getrandom_revoke_lock() -> bool {
    use core::sync::atomic::Ordering;

    let busy = reg_grant_table_lock().try_lock().is_none();
    GETRANDOM_RACE_PROBED_BUSY.store(busy, Ordering::Release);
    busy
}

/// Publish that the unregister worker has completed its teardown attempt.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_finish_getrandom_revoke_race() {
    GETRANDOM_RACE_DONE.store(true, core::sync::atomic::Ordering::Release);
}

/// Return the final-write race observations in publication order.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_getrandom_revoke_race_result() -> (bool, bool, bool, bool) {
    use core::sync::atomic::Ordering;

    (
        GETRANDOM_RACE_PROBED_BUSY.load(Ordering::Acquire),
        GETRANDOM_RACE_NO_EARLY_DONE.load(Ordering::Acquire),
        GETRANDOM_RACE_DONE.load(Ordering::Acquire),
        GETRANDOM_RACE_PROBE_TIMED_OUT.load(Ordering::Acquire),
    )
}

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(super) fn pause_getrandom_final_write_for_revoke_race(caller_id: usize) -> bool {
    use core::sync::atomic::Ordering;

    if GETRANDOM_RACE_ARMED_CALLER.swap(0, Ordering::AcqRel) != caller_id {
        return false;
    }
    GETRANDOM_RACE_ENTERED.store(true, Ordering::Release);
    let mut observed_probe = false;
    for _ in 0..100_000_000usize {
        if GETRANDOM_RACE_PROBED_BUSY.load(Ordering::Acquire) {
            observed_probe = true;
            break;
        }
        core::hint::spin_loop();
    }
    GETRANDOM_RACE_PROBE_TIMED_OUT.store(!observed_probe, Ordering::Release);
    true
}

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(super) fn verify_getrandom_revoke_race_done_state(was_armed: bool) {
    use core::sync::atomic::Ordering;

    if !was_armed {
        return;
    }
    GETRANDOM_RACE_NO_EARLY_DONE.store(
        !GETRANDOM_RACE_DONE.load(Ordering::Acquire),
        Ordering::Release,
    );
}

pub(super) fn write_getrandom_output(
    caller_id: usize,
    ptr: usize,
    bytes: &[u8],
) -> Result<(), SyscallError> {
    let view = caller_copy_view(caller_id)?;
    #[cfg(all(feature = "native-domains", cellos_wide))]
    if view.is_sas() {
        // This scoped, no-allocation lease covers every record contributing to
        // a contiguous span. Removal waits for its member lock before it can
        // mark retiring/revoked, unmap, or return backing to the allocator.
        let page_grants = grant_table_lock().lock();
        let registered_grants = reg_grant_table_lock().lock();
        let scheduler = crate::task::SCHEDULER.lock();
        let Some(sched) = scheduler.as_ref() else {
            return Err(SyscallError::InvalidInput);
        };
        if !sas_caller_owned_span(
            sched,
            page_grants.as_ref(),
            registered_grants.as_ref(),
            caller_id,
            ptr,
            bytes.len(),
        ) {
            return Err(SyscallError::InvalidInput);
        }
        let lease = sched
            .tasks
            .get(&caller_id)
            .map(TaskCopyView::of)
            .ok_or(SyscallError::InvalidInput)?;
        #[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
        let was_armed = pause_getrandom_final_write_for_revoke_race(caller_id);
        let write_result = lease
            .write_bytes(ptr, bytes)
            .map_err(|_| SyscallError::InvalidInput);
        #[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
        verify_getrandom_revoke_race_done_state(was_armed);
        return write_result;
    }
    view.write_bytes(ptr, bytes)
        .map_err(|_| SyscallError::InvalidInput)
}

/// Receive-side staging copy of a queued payload: inline up to
/// [`MSG_PAYLOAD_INLINE`] bytes, so small receives never touch the heap.
#[derive(Clone, Debug)]
pub(in crate::task) enum MsgPayload {
    Inline {
        len: u8,
        bytes: [u8; MSG_PAYLOAD_INLINE],
    },
    Heap(alloc::vec::Vec<u8>),
}

/// Largest payload [`MsgPayload`] stores without allocating.
pub(super) const MSG_PAYLOAD_INLINE: usize = 32;

/// Commit the exact event that was peeked by `snapshot_resume`.
/// Must be called under the scheduler lock with the same `task`.
pub(in crate::task) fn commit_resume(task: &mut crate::task::Task, snap: &ResumeSnapshot) {
    match snap {
        ResumeSnapshot::Death {
            sender_tid,
            is_owner_death: true,
            ..
        } => {
            // Remove the first owner-death with matching root_tid.
            if let Some(pos) = task
                .pending_owner_deaths
                .iter()
                .position(|(_, tid, _)| tid == sender_tid)
            {
                task.pending_owner_deaths.remove(pos);
            }
        }
        ResumeSnapshot::Death {
            is_owner_death: false,
            ..
        } => {
            task.pending_exit_reason.take();
        }
        ResumeSnapshot::Message {
            delivery_id: Some(did),
            ..
        } => {
            if let Some(pos) = task
                .pending_msgs
                .iter()
                .position(|m| m.wire_header().is_some_and(|h| h.delivery_id == *did))
            {
                task.pending_msgs.remove(pos);
            }
        }
        ResumeSnapshot::Message {
            sender_tid,
            delivery_id: None,
            ..
        } => {
            if let Some(pos) = task
                .pending_msgs
                .iter()
                .position(|m| m.sender_tid == *sender_tid)
            {
                task.pending_msgs.remove(pos);
            }
        }
        ResumeSnapshot::Wake { .. } => {}
    }
}

/// Legacy owned-removal delivery enum; used by selftests that consume the
/// event and immediately inspect it. Production paths use `snapshot_resume` +
/// `commit_resume` instead. Do not add new callers.
#[allow(dead_code)]
pub(in crate::task) enum ResumeDelivery {
    Death { sender_tid: usize, reason: usize },
    Message(crate::task::tcb::PendingMsg),
    Wake,
}

#[cfg(not(cellos_posture = "release"))]
/// Consume the next matching delivery event and return it. Selftest-only;
/// production callers use `snapshot_resume` + `commit_resume`.
pub(in crate::task) fn take_resume_delivery(
    task: &mut crate::task::Task,
    mask: usize,
) -> ResumeDelivery {
    // Snapshot without allocation — Death and Wake carry no payload.
    // For Message we need to remove the record, so we take it directly.
    if crate::task::Task::owner_death_matches_receive_mask(mask) {
        if let Some((_, root_tid, reason)) = task.pending_owner_deaths.first().copied() {
            task.pending_owner_deaths.remove(0);
            return ResumeDelivery::Death {
                sender_tid: root_tid,
                reason,
            };
        }
    }
    if let Some(reason) = task.pending_exit_reason.take() {
        return ResumeDelivery::Death {
            sender_tid: task.caller.current.unwrap_or(0),
            reason,
        };
    }
    if let Some(index) = task
        .pending_msgs
        .iter()
        .position(|m| mask == 0 || m.sender_tid == mask)
    {
        return ResumeDelivery::Message(task.pending_msgs.remove(index));
    }
    ResumeDelivery::Wake
}

pub(in crate::task) fn encode_task_state(state: &TaskState) -> u32 {
    match state {
        TaskState::Ready => 0,
        TaskState::Running => 1,
        TaskState::Terminated | TaskState::Retiring => 3,
        _ => 2,
    }
}

pub(super) fn encode_task_name(name: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = name.as_bytes();
    let len = core::cmp::min(bytes.len(), out.len());
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

pub(super) fn task_heap_bytes(task: &crate::task::tcb::Task) -> u64 {
    crate::memory::cell_quota::in_use(task.cell_id) as u64
}

pub(super) fn task_owned_bytes(task: &crate::task::tcb::Task) -> u64 {
    let user_stack_bytes = task
        .user_stack
        .as_ref()
        .map(|stack| stack.usable_bytes() as u64)
        .unwrap_or(0);
    let segment_bytes = task
        .segment_mem
        .as_ref()
        .map(|segments| segments.allocated_bytes() as u64)
        .unwrap_or(0);
    task_heap_bytes(task)
        .saturating_add(user_stack_bytes)
        .saturating_add(segment_bytes)
}

pub(super) fn collect_process_rows<T>(
    row_capacity: usize,
    mut build: impl FnMut(&crate::task::tcb::Task) -> T,
) -> Vec<T> {
    let mut rows = Vec::new();
    if row_capacity == 0 {
        return rows;
    }
    if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        let cap = core::cmp::min(row_capacity, sched.tasks.len());
        rows.reserve(cap);
        for task in sched.tasks.values().take(cap) {
            rows.push(build(task));
        }
    }
    rows
}

#[cfg(feature = "bench-counters")]
/// Handler for `Syscall::BenchCounters` (extracted from the dispatcher).
pub(super) fn bench_counters(caller_id: usize, out_ptr: usize, out_len: usize) -> SyscallResult {
    // Gated like MemInfo at dispatch (bit 56 + observe_memory).
    let len = crate::bench_counters::RECORD_BYTES;
    if out_len < len {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
    let record = crate::bench_counters::snapshot_bytes();
    write_user_slice(caller_id, out_ptr, &record, MAX_USER_BUF)?;
    Ok(len)
}
