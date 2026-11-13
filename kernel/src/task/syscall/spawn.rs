// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: spawn (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, grant::*, ipc::*, misc::*, vm::*};

/// Called outside source locks after exact root teardown. Never infer pin/DMA or
/// historical heap ownership from grant-table absence.
#[cfg(feature = "p01-trace")]
pub(crate) fn p01_retirement_check(owner: crate::p01_measurement::Owner) {
    let page_clear =
        {
            let table = grant_table_lock().lock();
            table.as_ref().map_or(Some(true), |table| {
                if table.len() > 128 {
                    return None;
                }
                Some(!table.values().any(|g| {
                    g.owner_cell.0 == owner.cell && g.owner_generation == owner.generation
                }))
            })
        };
    let registered_clear =
        {
            let table = reg_grant_table_lock().lock();
            table.as_ref().map_or(Some(true), |table| {
                if table.len() > 128 {
                    return None;
                }
                Some(!table.values().any(|g| {
                    g.owner_cell.0 == owner.cell && g.owner_generation == owner.generation
                }))
            })
        };
    if page_clear == Some(true) && registered_clear == Some(true) {
        crate::p01_resources::checked(owner, 3);
    } else {
        crate::p01_resources::unknown(owner, 3);
    }
}

/// The caller's incarnation for the state stash; a task without a live record
/// owns nothing and is refused.
pub(super) fn stash_incarnation(
    tid: usize,
) -> Result<crate::cell::state_stash::Incarnation, SyscallError> {
    live_cell_identity(tid)
        .map(|(cell, generation)| crate::cell::state_stash::Incarnation { cell, generation })
        .ok_or(SyscallError::PermissionDenied)
}

pub(super) fn caller_has_spawn(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.spawn)
}

/// Record `tid` as trusted init. Only the first publication counts.
pub(crate) fn publish_trusted_init(tid: usize) {
    use core::sync::atomic::Ordering;
    let _first_wins =
        TRUSTED_INIT_TID.compare_exchange(0, tid, Ordering::AcqRel, Ordering::Acquire);
}

pub(super) fn caller_is_trusted_init(caller_id: usize) -> bool {
    caller_id != 0 && TRUSTED_INIT_TID.load(core::sync::atomic::Ordering::Acquire) == caller_id
}

/// True when `tid` is the published trusted init root. The init-death policy
/// (`cell::init_death`) keys `exit_task` on it; tid zero is never init.
pub(crate) fn is_trusted_init(tid: usize) -> bool {
    caller_is_trusted_init(tid)
}

pub(super) const SPAWN_ARGV_KEY: u64 = 0x0061_7267_7600_0000;

pub(super) fn consumes_staged_spawn_argv(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::SpawnFromPath { .. }
            | Syscall::SpawnFromElf { .. }
            | Syscall::SpawnPinned { .. }
            | Syscall::SpawnFromMem { .. }
    )
}

/// Clears the caller's own *staged* command line after every external-launch
/// attempt, including preflight and allowlist failures before
/// `governed_spawn_request` can consume it.
///
/// It clears only what this task staged for this attempt. A command line the
/// loader handed to a child lives on that child's task record, so no other
/// task's failed attempt can reach it.
pub(super) struct StagedSpawnArgvCleanup {
    pub(super) caller_id: usize,
}

/// Whether the child may be named by its signed image (`Task::launch_named`):
/// the kernel resolved the path (Path, Pinned), or trusted init supplied the
/// bytes it read from its own `/bin` (init starts drivers from VFS grants).
/// Any other caller's ELF/Mem bytes carry an advisory path only.
pub(super) fn names_child(
    caller_root: usize,
    route: crate::loader::launch_profile::LaunchRoute,
) -> bool {
    use crate::loader::launch_profile::LaunchRoute;
    matches!(route, LaunchRoute::Path | LaunchRoute::Pinned) || caller_is_trusted_init(caller_root)
}

/// The caller's launch identity, spawn and supervisor flags.
///
/// The identity is **never the task name** (the basename of a spawner-chosen
/// path, architecture review C-1). It is `"init"` for any task of the Cell
/// whose root the kernel published as trusted init, otherwise the program name
/// from the verified `__ViCell_image` section of the caller's Cell root
/// (`Task::image`) when the kernel itself resolved that root's install path
/// (`Task::launch_named`), and empty otherwise — unsigned or unnamed images,
/// caller-supplied ELF/Mem bytes, sandbox paths and aliases match no launch row. Threads share their Cell root's identity.
pub(super) fn caller_launch_state(caller_id: usize) -> Option<(String, bool, bool)> {
    crate::task::SCHEDULER.lock().as_ref().and_then(|sched| {
        let identity = launch_identity_in(sched, caller_id)?;
        let rights = sched.cell_rights(caller_id);
        Some((identity, rights.spawn, rights.supervisor))
    })
}

/// Non-owning peek at the next delivery event. Payload bytes are copied into
/// kernel memory; the original record is NOT removed. Call `commit_resume`
/// after a successful copy-out to remove exactly the same event.
pub(in crate::task) enum ResumeSnapshot {
    /// A peer died or an exit reason was synthesised. `is_owner_death` flags
    /// which record to commit: owner-deaths queue vs. `pending_exit_reason`.
    Death {
        sender_tid: usize,
        reason: usize,
        is_owner_death: bool,
    },
    /// A queued message. Payload already staged; commit key is `delivery_id`
    /// (wire) or `sender_tid` (inline) — whichever is `Some`/applicable.
    Message {
        sender_tid: usize,
        wire_header: Option<crate::ipc::ipc_wire::IpcWireHeader>,
        delivery_id: Option<u64>,
        sender_cell_id: u64,
        sender_generation: u64,
        payload: MsgPayload,
    },
    /// Woken with no pending message (timeout, manual wake, or 0-return).
    Wake { sender_tid: usize },
}

/// Peek at the next delivery event matching `mask` without removing anything.
/// Returns `Err(())` on allocation failure (OOM staging payload bytes).
pub(in crate::task) fn snapshot_resume(
    task: &crate::task::Task,
    mask: usize,
) -> Result<ResumeSnapshot, ()> {
    // Owner-death events take priority (same order as take_resume_delivery).
    if crate::task::Task::owner_death_matches_receive_mask(mask) {
        if let Some((_, root_tid, reason)) = task.pending_owner_deaths.first().copied() {
            return Ok(ResumeSnapshot::Death {
                sender_tid: root_tid,
                reason,
                is_owner_death: true,
            });
        }
    }
    if let Some(reason) = task.pending_exit_reason {
        return Ok(ResumeSnapshot::Death {
            sender_tid: task.caller.current.unwrap_or(0),
            reason,
            is_owner_death: false,
        });
    }
    if let Some(index) = task
        .pending_msgs
        .iter()
        .position(|m| mask == 0 || m.sender_tid == mask)
    {
        let msg = &task.pending_msgs.as_slice()[index];
        let wire_header = msg.wire_header();
        let delivery_id = wire_header.as_ref().map(|h| h.delivery_id);
        let (sender_cell_id, sender_generation) = match &wire_header {
            Some(h) => (h.sender_cell_id, h.sender_generation),
            None => (0, 0),
        };
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            Some(crate::p01_resources::owner(task)),
            crate::memory::heap::p01_owner::RECEIVE_SCRATCH,
        );
        let payload = MsgPayload::from_slice(msg.payload())?;
        return Ok(ResumeSnapshot::Message {
            sender_tid: msg.sender_tid,
            wire_header,
            delivery_id,
            sender_cell_id,
            sender_generation,
            payload,
        });
    }
    Ok(ResumeSnapshot::Wake {
        sender_tid: task.caller.current.unwrap_or(0),
    })
}

pub(super) fn snapshot_process_info(task: &crate::task::tcb::Task) -> abi::syscall::ProcessInfo {
    abi::syscall::ProcessInfo {
        id: task.id,
        state: encode_task_state(&task.state) as usize,
        name: encode_task_name(&task.name),
    }
}

/// Build the guard only for a spawn syscall.
///
/// `consumes_staged_spawn_argv(&syscall).then_some(StagedSpawnArgvCleanup { .. })`
/// looks equivalent but is not: `then_some` takes its argument **by value**, so the
/// guard was constructed for *every* syscall and dropped again immediately when the
/// predicate was false — its `Drop` then discarded the caller's staged command
/// line. Staging an argv and then issuing any other syscall before the spawn (the
/// shell's VFS read is exactly that) silently lost the command line; the cell then
/// started with no arguments at all (`httpd 9091 /tmp/resp.txt` listened on :8080).
pub(super) fn staged_spawn_argv_cleanup(
    syscall: &Syscall,
    caller_id: usize,
) -> Option<StagedSpawnArgvCleanup> {
    if consumes_staged_spawn_argv(syscall) {
        Some(StagedSpawnArgvCleanup { caller_id })
    } else {
        None
    }
}

pub(super) fn governed_spawn_request(
    caller_id: usize,
    route: crate::loader::launch_profile::LaunchRoute,
    child_ceiling: crate::task::cap::CapSet,
    priority: u8,
    #[cfg(feature = "p01-trace")] trace: Option<crate::p01_measurement::Attempt>,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (incarnation, caller_authority, caller_root) = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| {
            let task = sched.tasks.get(&caller_id)?;
            #[cfg(feature = "p01-trace")]
            if trace.is_some_and(|a| a.parent != crate::p01_measurement::Owner::of(task)) {
                crate::p01_measurement::invalidate(crate::p01_measurement::IDENTITY);
            }
            Some((
                (task.cell_id.0, task.cell_generation),
                sched.cell_rights(caller_id),
                task.root_tid,
            ))
        })
        .ok_or(SyscallError::PermissionDenied)?;
    let argv = crate::cell::state_stash::take_spawn_argv(caller_id);
    Ok(crate::loader::SpawnRequest::governed_caller(
        caller_id,
        incarnation,
        caller_authority,
        child_ceiling,
        priority,
        argv,
        names_child(caller_root, route),
    ))
}

pub(super) fn governed_replacement_request(
    caller_id: usize,
    route_ceiling: crate::task::cap::CapSet,
    replacement: crate::cell::hotswap::ReplacementReservation,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (incarnation, caller_authority) = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| {
            let task = sched.tasks.get(&caller_id)?;
            Some((
                (task.cell_id.0, task.cell_generation),
                sched.cell_rights(caller_id),
            ))
        })
        .ok_or(SyscallError::PermissionDenied)?;
    let argv = crate::cell::state_stash::take_spawn_argv(caller_id);
    Ok(crate::loader::SpawnRequest::governed_replacement(
        caller_id,
        incarnation,
        caller_authority,
        route_ceiling,
        replacement,
        argv,
    ))
}

/// The launch identity of `caller_id` (see [`caller_launch_state`]) read under
/// a scheduler the caller already holds. `None` when the task is gone.
pub(in crate::task) fn launch_identity_in(
    sched: &crate::sched::scheduler::Scheduler,
    caller_id: usize,
) -> Option<String> {
    let init_tid = TRUSTED_INIT_TID.load(core::sync::atomic::Ordering::Acquire);
    let task = sched.tasks.get(&caller_id)?;
    Some(if init_tid != 0 && task.root_tid == init_tid {
        String::from("init")
    } else {
        sched
            .tasks
            .get(&task.root_tid)
            .filter(|root| root.launch_named)
            .and_then(|root| root.image())
            .filter(|image| image.is_signed())
            .and_then(|image| core::str::from_utf8(image.name()).ok().map(String::from))
            .unwrap_or_default()
    })
}

pub(super) fn authorize_launch_edge(
    caller_id: usize,
    route: crate::loader::launch_profile::LaunchRoute,
    target: &str,
) -> Result<crate::loader::launch_profile::LaunchProfile, SyscallError> {
    let (caller_name, has_spawn, has_supervisor) =
        caller_launch_state(caller_id).unwrap_or((String::from("<unknown>"), false, false));
    let caller = crate::loader::launch_profile::CallerLaunchState {
        name: &caller_name,
        has_spawn,
        has_supervisor,
    };
    let profile = crate::loader::launch_profile::authorize(caller, route, target).ok_or_else(|| {
        log::warn!(
            "[loader] DENY launch edge: caller={} name={} route={:?} target={} spawn_cap={} supervisor_cap={}",
            caller_id,
            caller.name,
            route,
            target,
            caller.has_spawn,
            caller.has_supervisor
        );
        SyscallError::PermissionDenied
    })?;
    if profile.requires_lifecycle_authority && !caller.has_spawn {
        log::warn!(
            "[loader] DENY launch edge: caller={} name={} route={:?} target={} lacks lifecycle authority",
            caller_id,
            caller.name,
            route,
            target
        );
        return Err(SyscallError::PermissionDenied);
    }
    Ok(profile)
}

pub(crate) fn session_launch_request(
    caller: usize,
    path: &str,
    route: crate::loader::launch_profile::LaunchRoute,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let profile = authorize_launch_edge(caller, route, path)?;
    let guard = crate::task::SCHEDULER.lock();
    let task = guard
        .as_ref()
        .and_then(|s| s.tasks.get(&caller))
        .ok_or(SyscallError::PermissionDenied)?;
    // The CellSessionV1 transport itself carries no allowlist bit, so the
    // launch it performs must hold the same bit as the equivalent syscall:
    // SpawnFromPath for the path route, SpawnFromElf for caller-owned bytes.
    let syscall = if matches!(route, crate::loader::launch_profile::LaunchRoute::Elf) {
        ViSyscall::SpawnFromElf
    } else {
        ViSyscall::SpawnFromPath
    };
    if let Some(bit) = syscall.allowlist_bit() {
        if (task.syscall_allowlist >> bit) & 1 == 0 {
            log::warn!(
                "[kernel] session launch denied for tid {}: {:?} bit {} not allowed",
                caller,
                syscall,
                bit
            );
            return Err(SyscallError::PermissionDenied);
        }
    }
    let ceiling = guard
        .as_ref()
        .map_or(crate::task::cap::CapSet::EMPTY, |s| s.cell_rights(caller));
    Ok(crate::loader::SpawnRequest::governed_caller(
        caller,
        (task.cell_id.0, task.cell_generation),
        ceiling,
        profile.child_ceiling,
        abi::TaskPriority::Normal as u8,
        None,
        names_child(task.root_tid, route),
    ))
}

/// Bind the dispatch boundary while the receive snapshot owns the scheduler
/// lock, before an independent caller can cancel and remove the queued record.
pub(super) fn snapshot_resume_dispatched(
    sched: &mut crate::sched::scheduler::Scheduler,
    caller_id: usize,
    mask: usize,
) -> Result<ResumeSnapshot, ()> {
    let snap = match sched.tasks.get(&caller_id) {
        Some(task) => snapshot_resume(task, mask)?,
        None => ResumeSnapshot::Wake { sender_tid: 0 },
    };
    if let ResumeSnapshot::Message {
        wire_header: Some(header),
        ..
    } = &snap
    {
        crate::task::async_ipc::dispatch(sched, caller_id, *header);
    }
    Ok(snap)
}

/// Take a blocked receive's resume snapshot, or park the receiver again when it
/// woke with nothing to take.
///
/// Messages reach a receiver only through its mailbox, and deaths only through
/// the death fields, so a `Wake` snapshot means the receiver was made runnable
/// for a message that is gone: a submitted request that was cancelled before the
/// receiver ran, or a post that `mask` does not admit. Completing the receive
/// would hand userspace a sender tid over its previous buffer contents. Instead,
/// under the same lock that found the mailbox empty, the receiver parks again
/// with the same mask, buffer and deadline, so the next matching publish wakes
/// it as usual.
///
/// A receive whose `deadline` has passed reports the timeout as sender 0.
///
/// Returns `Ok(None)` when the receiver parked again: the caller yields and
/// retries. `Err(())` is the snapshot's allocation failure.
pub(in crate::task) fn resume_or_repark(
    sched: &mut crate::sched::scheduler::Scheduler,
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
    deadline: Option<u64>,
) -> Result<Option<ResumeSnapshot>, ()> {
    let snap = snapshot_resume_dispatched(sched, caller_id, mask)?;
    if !matches!(snap, ResumeSnapshot::Wake { .. }) {
        return Ok(Some(snap));
    }
    if deadline.is_some_and(|deadline| crate::task::system_ticks() as u64 >= deadline) {
        return Ok(Some(ResumeSnapshot::Wake { sender_tid: 0 }));
    }
    let Some(task) = sched.tasks.get_mut(&caller_id) else {
        return Ok(Some(snap));
    };
    crate::task::arm_ipc_block_handoff(caller_id);
    task.state = crate::task::tcb::TaskState::Blocked {
        wait: crate::task::tcb::WaitKind::Recv {
            mask,
            buf_ptr,
            buf_len,
        },
        deadline,
    };
    Ok(None)
}

pub(super) fn snapshot_process_info_v2(
    task: &crate::task::tcb::Task,
    sample_ticks: u64,
) -> abi::syscall::ProcessInfoV2 {
    abi::syscall::ProcessInfoV2 {
        id: task.id as u64,
        state: encode_task_state(&task.state),
        reserved0: 0,
        name: encode_task_name(&task.name),
        sample_ticks,
        cpu_run_ticks: task.sched_stats.cpu_run_ticks,
        heap_bytes: task_heap_bytes(task),
        owned_bytes: task_owned_bytes(task),
    }
}

/// Handler for `Syscall::CellSessionV1` (extracted from the dispatcher).
pub(super) fn cell_session_v1(
    caller_id: usize,
    request_ptr: usize,
    request_len: usize,
    result_ptr: usize,
    result_capacity: usize,
) -> SyscallResult {
    const MAX: usize = abi::cell_session::MAX_REQUEST_LEN;
    if !(abi::cell_session::HEADER_LEN..=MAX).contains(&request_len)
        || result_capacity < abi::cell_session::HEADER_LEN
    {
        return Err(SyscallError::InvalidInput);
    }
    let request_end = request_ptr
        .checked_add(request_len)
        .ok_or(SyscallError::InvalidInput)?;
    let result_end = result_ptr
        .checked_add(result_capacity)
        .ok_or(SyscallError::InvalidInput)?;
    if request_ptr < result_end && result_ptr < request_end {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(request_ptr, request_len, MAX)?;
    validate_user_buf(result_ptr, result_capacity, MAX_USER_BUF)?;
    preflight_user_output(caller_id, result_ptr, result_capacity)?;
    let view = caller_copy_view(caller_id)?;
    let mut request = [0u8; MAX];
    let _reply_pin = SessionOutputPin::reserve(caller_id, result_ptr, result_capacity, &view)?;
    view.read_into(request_ptr, &mut request[..request_len])
        .map_err(|_| SyscallError::InvalidInput)?;
    let header = abi::cell_session::RequestHeaderV1::decode(&request[..request_len]);
    let mut result = [0u8; 1536];
    if let Ok(header) = header {
        let required = crate::ipc::cell_session::required_result(
            header.selector,
            &request[abi::cell_session::HEADER_LEN..request_len],
        );
        if result_capacity < required {
            abi::cell_session::ResultHeaderV1 {
                selector: header.selector,
                status: abi::cell_session::Status::BufferTooSmall,
                total_len: required as u32,
                request_id: header.request_id,
            }
            .encode(&mut result)
            .map_err(|_| SyscallError::InvalidInput)?;
            view.write_bytes(result_ptr, &result[..abi::cell_session::HEADER_LEN])
                .map_err(|_| SyscallError::InvalidInput)?;
            return Ok(0);
        }
    }
    let len = crate::ipc::cell_session::dispatch(caller_id, &request[..request_len], &mut result);
    view.write_bytes(result_ptr, &result[..len])
        .map_err(|_| SyscallError::InvalidInput)?;
    // Only a reply that reached the caller may be acknowledged.
    crate::ipc::cell_session::delivered(caller_id, &request[..request_len]);
    Ok(0)
}

/// Handler for `Syscall::Spawn` (extracted from the dispatcher).
pub(super) fn spawn(caller_id: usize, entry: usize, arg: usize) -> SyscallResult {
    // Build and fully inherit the child under one scheduler lock before
    // publication. A runnable thread must never observe Task defaults
    // (SAS, permissive allowlist, or zero PKU state) between construction
    // and its creator's authority transfer.
    match crate::task::spawn_with_arg(caller_id, "thread", entry, arg) {
        Ok(tid) => Ok(tid),
        Err(ViError::OutOfMemory) => Err(SyscallError::TryAgain),
        Err(_) => Err(SyscallError::Unknown),
    }
}

/// Handler for `Syscall::SetTlsBase` (extracted from the dispatcher).
pub(super) fn set_tls_base(caller_id: usize, base: usize) -> SyscallResult {
    // Self-only by construction: the ABI has no target task parameter, so
    // this can only ever change the caller's own thread pointer, and the
    // base carries no authority — the kernel never dereferences it. A bad
    // value is contained by the cell's own tier (LBI on Tier 1, an MMU
    // fault on Tier 2).
    //
    // The previous value is returned so a cell can read its base back
    // without touching the register, which the kernel owns (a direct user
    // write is overwritten on the next resume).
    if let Some(task) = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
    {
        let previous = task.tls_base;
        crate::task::tls::set_base(task, base);
        Ok(previous)
    } else {
        Err(SyscallError::PermissionDenied)
    }
}

/// Handler for `Syscall::Exit` (extracted from the dispatcher).
pub(super) fn exit(caller_id: usize, code: usize) -> SyscallResult {
    // TID zero is the explicit kernel-context sentinel. It has no Cell
    // generation to retire; a kernel scheduling point remains explicit
    // rather than fabricating a user-exit record.
    if caller_id == 0 {
        crate::task::yield_cpu();
        return Ok(0);
    }

    #[cfg(feature = "bench-counters")]
    crate::bench_counters::note_exit(caller_id);

    // Do not inspect the task table, log, audit, or call
    // `Scheduler::exit_task` while the exiting Cell is still charged.
    // A root exit can retire every member and grow scheduler vectors;
    // the fixed record is therefore the entire victim-attributed path.
    let exit = crate::sched::hart_local::DeferredExit {
        tid: caller_id,
        cell_id: crate::sched::hart_local::current_cell_id(),
        generation: crate::sched::hart_local::current_cell_generation(),
        code,
    };
    crate::sched::hart_local::defer_exit(exit);
    crate::sched::hart_local::set_current_cell_id(0);

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::task::retirement_selftest::observe_exit_deferred_record_commit(exit);

    // `yield_cpu` consumes the fixed record under normal scheduler
    // locking and routes roots through quiescent retirement. A worker
    // still reaches the task-local branch of `exit_task`.
    crate::task::yield_cpu();
    Ok(0)
}

/// Handler for `Syscall::ForceExit` (extracted from the dispatcher).
pub(super) fn force_exit(caller_id: usize, tid: usize) -> SyscallResult {
    // Self-kill rejected before touching the scheduler (cheap early check).
    if tid == caller_id {
        return Err(SyscallError::InvalidCommand);
    }

    // Single SCHEDULER lock: SpawnCap gate + all cleanup in one scope.
    // Two separate acquisitions would create a TOCTOU window where the target
    // self-exits between them, causing a spurious InvalidCommand return.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        // Gate 1: only SpawnCap holders (init/shell) may force-terminate tasks.
        let has_spawn = sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
        if !has_spawn {
            return Err(SyscallError::PermissionDenied);
        }

        // Gate 2: protect system service cells (VFS=block_io_cap, net=network_cap).
        // Killing them mid-I/O leaves driver state inconsistent; use hot-swap instead.
        let target_rights = sched.cell_rights(tid);
        let target_is_system = target_rights.block_io || target_rights.network;
        if target_is_system {
            return Err(SyscallError::PermissionDenied);
        }

        // Gate 3: protect Frozen cells — they are mid-swap and cannot be killed
        // by external actors; only the supervisor-owned cutover path may terminate
        // them via the internal exit_task path after a successful swap.
        let target_is_frozen = sched
            .tasks
            .get(&tid)
            .map(|t| matches!(t.state, TaskState::Frozen { .. }))
            .unwrap_or(false);
        if target_is_frozen {
            return Err(SyscallError::PermissionDenied);
        }

        let task = match sched.tasks.get_mut(&tid) {
            Some(t) => t,
            // Target self-exited between the lock boundary — already dead; mission done.
            None => return Ok(0),
        };
        task.exit_code = Some(usize::MAX); // sentinel: force-killed

        // exit_task: zombie move + ready-queue purge + stuck-sender unblock,
        // and wakes sys_wait(tid) waiters with the force-kill sentinel.
        sched.exit_task(tid, usize::MAX);
    } else {
        return Err(SyscallError::InvalidCommand);
    }

    crate::audit::log_event(
        crate::audit::AuditEvent::CellExit,
        &crate::audit::encode_u32x2(tid as u32, 0xFFFF_FFFFu32), // force-kill marker
    );

    log::info!(
        "[kernel] ForceExit: task {} killed by task {}",
        tid,
        caller_id
    );

    Ok(0) // non-blocking — caller keeps running; do NOT yield_cpu
}

/// Handler for `Syscall::NotifyOnExit` (extracted from the dispatcher).
pub(super) fn notify_on_exit(caller_id: usize, watched: usize) -> SyscallResult {
    // Privileged: only SpawnCap holders (supervisors like init) may watch
    // arbitrary tasks — same authority gate as ForceExit. The watcher's
    // next Recv returns `watched` when it dies (see exit_task delivery).
    //
    // Race: the watched task may have already exited before this call.
    // Subscribe while SCHEDULER still proves the target is live; exit_task
    // takes DEATH_SUBSCRIBERS under the same outer lock, so publication and
    // delivery are atomic with respect to task death.
    {
        let mut sched_opt = crate::task::SCHEDULER.lock();
        let sched = match sched_opt.as_mut() {
            Some(s) => s,
            None => return Ok(0),
        };
        let has_spawn = sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
        if !has_spawn {
            return Err(SyscallError::PermissionDenied);
        }
        if sched
            .tasks
            .get(&watched)
            .is_some_and(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
        {
            crate::sched::scheduler::subscribe_death(watched, caller_id);
        } else {
            // Task already dead — queue synthetic death so watcher never stalls.
            if let Some(wt) = sched.tasks.get_mut(&caller_id) {
                crate::sched::scheduler::queue_pending_death(wt, watched, 0);
            }
        }
    }
    Ok(0)
}

/// Handler for `Syscall::QueryRetirement` (extracted from the dispatcher).
pub(super) fn query_retirement(
    caller_id: usize,
    root_tid: usize,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    // Admitted past the allowlist; the verdict table is the gate: a
    // death subscriber of that root at its retirement, or a supervisor.
    if out_len != abi::retirement::RETIREMENT_RECORD_LEN {
        return Err(SyscallError::InvalidInput);
    }
    let supervisor = caller_has_supervisor(caller_id);
    let record = crate::cell::retirement_verdict::query(caller_id, supervisor, root_tid).map_err(
        |error| match error {
            crate::cell::retirement_verdict::QueryError::NotFound => SyscallError::FileNotFound,
            crate::cell::retirement_verdict::QueryError::Denied => SyscallError::PermissionDenied,
        },
    )?;
    write_user_slice(
        caller_id,
        out_ptr,
        &record.encode(),
        abi::retirement::RETIREMENT_RECORD_LEN,
    )?;
    Ok(abi::retirement::RETIREMENT_RECORD_LEN)
}

/// Handler for `Syscall::Yield` (extracted from the dispatcher).
pub(super) fn yield_now() -> SyscallResult {
    crate::task::yield_cpu();
    Ok(0)
}

/// Handler for `Syscall::SpawnFromPath` (extracted from the dispatcher).
pub(super) fn spawn_from_path(
    caller_id: usize,
    path_ptr: usize,
    path_len: usize,
    #[cfg(feature = "p01-trace")] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    let path_str = read_user_string(
        caller_id,
        path_ptr,
        path_len,
        crate::loader::disk_layout::MAX_CELL_PATH,
    )?;
    #[cfg(feature = "p01-trace")]
    crate::p01_measurement::classify(trace, &path_str);
    if !path_str.starts_with('/') {
        return Err(SyscallError::InvalidInput);
    }
    let profile = authorize_launch_edge(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Path,
        &path_str,
    )?;
    let request = match governed_spawn_request(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Path,
        profile.child_ceiling,
        abi::TaskPriority::Normal as u8,
        #[cfg(feature = "p01-trace")]
        *trace,
    ) {
        Ok(request) => request,
        Err(error) => {
            crate::task::dir_inherit::clear_staged(caller_id);
            return Err(error);
        }
    };
    #[cfg(feature = "p01-trace")]
    let request = {
        let mut request = request;
        request.trace = *trace;
        request
    };
    let spawned = crate::loader::spawn_from_path(&path_str, request);
    #[cfg(feature = "p01-trace")]
    if let Err(error) = spawned {
        crate::p01_measurement::source_error(*trace, error);
    }
    // A successful spawn consumed any staged directory-handle set inside
    // task creation. Clearing unconditionally covers the failure paths:
    // a set left staged by a spawn that never produced a child would be
    // handed to whichever child this caller created next.
    crate::task::dir_inherit::clear_staged(caller_id);
    let task_id = spawned.map_err(|e| {
        log::warn!(
            "[loader] SpawnFromPath refused: caller={} path={} error={:?}",
            caller_id,
            path_str,
            e
        );
        match e {
            types::ViError::NotFound => SyscallError::FileNotFound,
            types::ViError::OutOfMemory => {
                log::warn!(
                    "[loader] spawn OOM: op=SpawnFromPath caller={} path={}",
                    caller_id,
                    path_str
                );
                SyscallError::OutOfMemory
            }
            _ => SyscallError::InvalidInput,
        }
    })?;
    Ok(task_id)
}

/// Handler for `Syscall::SpawnSetDirs` (extracted from the dispatcher).
pub(super) fn spawn_set_dirs(caller_id: usize, carrier_ptr: usize) -> SyscallResult {
    if !caller_has_spawn(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if carrier_ptr == 0 {
        crate::task::dir_inherit::clear_staged(caller_id);
        return Ok(0);
    }
    let carrier_size = core::mem::size_of::<abi::dir_handles::ViSpawnDirHandles>();
    let bytes = read_user_slice(caller_id, carrier_ptr, carrier_size, MAX_USER_BUF)?;
    let carrier = unsafe {
        core::ptr::read_unaligned(bytes.as_ptr() as *const abi::dir_handles::ViSpawnDirHandles)
    };
    let set = abi::dir_handles::DirHandleSet::from_carrier(&carrier).map_err(|e| {
        log::warn!(
            "[dirs] tid {} named an invalid directory handle set: {:?}",
            caller_id,
            e
        );
        SyscallError::InvalidInput
    })?;
    crate::task::dir_inherit::stage(caller_id, set);
    Ok(0)
}

/// Handler for `Syscall::SpawnFromElf` (extracted from the dispatcher).
pub(super) fn spawn_from_elf(
    caller_id: usize,
    grant_id: usize,
    len: usize,
    path_ptr: usize,
    path_len: usize,
    #[cfg(feature = "p01-trace")] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    let elf = snapshot_grant_elf(caller_id, grant_id, len)?;
    let path_str = read_user_string(
        caller_id,
        path_ptr,
        path_len,
        crate::loader::disk_layout::MAX_CELL_PATH,
    )?;
    #[cfg(feature = "p01-trace")]
    crate::p01_measurement::classify(trace, &path_str);
    if !path_str.starts_with('/') {
        return Err(SyscallError::InvalidInput);
    }
    let profile = authorize_launch_edge(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Elf,
        &path_str,
    )?;
    log::info!(
        "[loader] SpawnFromElf: {} ({} bytes from grant)",
        path_str,
        len
    );
    let request = match governed_spawn_request(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Elf,
        profile.child_ceiling,
        abi::TaskPriority::Normal as u8,
        #[cfg(feature = "p01-trace")]
        *trace,
    ) {
        Ok(request) => request,
        Err(error) => {
            crate::task::dir_inherit::clear_staged(caller_id);
            return Err(error);
        }
    };
    #[cfg(feature = "p01-trace")]
    let request = {
        let mut request = request;
        request.trace = *trace;
        request
    };
    let spawned = crate::loader::spawn_gated(elf.bytes(), &path_str, request);
    #[cfg(feature = "p01-trace")]
    if let Err(error) = spawned {
        crate::p01_measurement::source_error(*trace, error);
    }
    crate::task::dir_inherit::clear_staged(caller_id);
    let task_id = spawned.map_err(|e| match e {
        types::ViError::NotFound => SyscallError::FileNotFound,
        types::ViError::OutOfMemory => {
            log::warn!(
                "[loader] spawn OOM: op=SpawnFromElf caller={} path={} elf_len={}",
                caller_id,
                path_str,
                len
            );
            SyscallError::OutOfMemory
        }
        types::ViError::PermissionDenied => SyscallError::PermissionDenied,
        _ => SyscallError::InvalidInput,
    })?;
    Ok(task_id)
}

/// Handler for `Syscall::SpawnPinned` (extracted from the dispatcher).
pub(super) fn spawn_pinned(
    caller_id: usize,
    path_ptr: usize,
    path_len: usize,
    priority: u8,
    core_id: usize,
    #[cfg(feature = "p01-trace")] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    if core_id != 0 {
        return Err(SyscallError::NotSupported);
    }
    let path_str = read_user_string(
        caller_id,
        path_ptr,
        path_len,
        crate::loader::disk_layout::MAX_CELL_PATH,
    )?;
    #[cfg(feature = "p01-trace")]
    crate::p01_measurement::classify(trace, &path_str);
    if !path_str.starts_with('/') {
        return Err(SyscallError::InvalidInput);
    }
    let profile = authorize_launch_edge(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Pinned,
        &path_str,
    )?;
    let request = match governed_spawn_request(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Pinned,
        profile.child_ceiling,
        priority,
        #[cfg(feature = "p01-trace")]
        *trace,
    ) {
        Ok(request) => request,
        Err(error) => {
            crate::task::dir_inherit::clear_staged(caller_id);
            return Err(error);
        }
    };
    #[cfg(feature = "p01-trace")]
    let request = {
        let mut request = request;
        request.trace = *trace;
        request
    };
    let spawned = crate::loader::spawn_from_path(&path_str, request);
    #[cfg(feature = "p01-trace")]
    if let Err(error) = spawned {
        crate::p01_measurement::source_error(*trace, error);
    }
    crate::task::dir_inherit::clear_staged(caller_id);
    let task_id = spawned.map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => {
                    let (quota_nulls, heap_nulls) = crate::memory::heap::oom_sources();
                    let (used, free) = crate::memory::heap::usage();
                    let charged = crate::memory::cell_quota::in_use(types::CellId(
                        crate::sched::scheduler::current_cell_id() as u64,
                    ));
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnPinned caller={} path={} heap_used={} heap_free={} caller_charged={} task_size={} null_from_quota={} null_from_heap={}",
                        caller_id,
                        path_str,
                        used,
                        free,
                        charged,
                        core::mem::size_of::<crate::task::tcb::Task>(),
                        quota_nulls,
                        heap_nulls
                    );
                    #[cfg(feature = "cell-scale-experiment")]
                    log::warn!(
                        "[heap] net per size class (bound, allocs, net_bytes): {:?}",
                        crate::memory::heap::size_hist::snapshot()
                    );
                    // Sizes of the per-spawn structures are logged by the alloc
                    // error handler when a layout cannot be satisfied at all.
                    SyscallError::OutOfMemory
                }
                _ => SyscallError::InvalidInput,
            })?;
    // Retained-cost readout: successive spawns show what one parked cell
    // costs the kernel heap (the number the D5 gate needs).
    let (used, free) = crate::memory::heap::usage();
    log::info!(
        "[heap] used={} KiB free={} KiB after SpawnPinned path={}",
        used / 1024,
        free / 1024,
        path_str
    );
    Ok(task_id)
}

/// Handler for `Syscall::SpawnFromMem` (extracted from the dispatcher).
pub(super) fn spawn_from_mem(
    caller_id: usize,
    args_ptr: usize,
    #[cfg(feature = "p01-trace")] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    if args_ptr == 0 {
        return Err(SyscallError::InvalidInput);
    }
    let view = caller_copy_view(caller_id)?;
    let mut args_bytes = [0u8; core::mem::size_of::<ViSpawnArgs>()];
    view.read_into(args_ptr, &mut args_bytes)
        .map_err(|_| SyscallError::InvalidInput)?;
    let args = unsafe { core::ptr::read_unaligned(args_bytes.as_ptr() as *const ViSpawnArgs) };
    validate_user_buf(args.buffer_addr, args.buffer_size, MAX_USER_BUF)?;
    validate_user_buf(args.name_ptr, args.name_len, MAX_LOG_MSG)?;
    let elf_bytes = view
        .read_bytes(args.buffer_addr, args.buffer_size)
        .map_err(|_| SyscallError::InvalidInput)?;
    let name_bytes = view
        .read_bytes(args.name_ptr, args.name_len)
        .map_err(|_| SyscallError::InvalidInput)?;
    let name = core::str::from_utf8(&name_bytes).unwrap_or("unknown");
    let profile = authorize_launch_edge(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Mem,
        name,
    )?;
    let request = match governed_spawn_request(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Mem,
        profile.child_ceiling,
        abi::TaskPriority::Normal as u8,
        #[cfg(feature = "p01-trace")]
        *trace,
    ) {
        Ok(request) => request,
        Err(error) => {
            crate::task::dir_inherit::clear_staged(caller_id);
            return Err(error);
        }
    };
    let spawned = crate::loader::mem_spawn_gate::spawn_from_mem_gated(&elf_bytes, name, request);
    crate::task::dir_inherit::clear_staged(caller_id);
    spawned.map_err(|e| match e {
        types::ViError::PermissionDenied => SyscallError::PermissionDenied,
        types::ViError::OutOfMemory => {
            log::warn!(
                "[loader] spawn OOM: op=SpawnFromMem caller={} name={} elf_len={}",
                caller_id,
                name,
                elf_bytes.len()
            );
            SyscallError::OutOfMemory
        }
        _ => SyscallError::InvalidInput,
    })
}

/// Handler for `Syscall::TelemetrySnapshot` (extracted from the dispatcher).
pub(super) fn telemetry_snapshot(
    caller_id: usize,
    out_ptr: usize,
    out_len: usize,
    flags: usize,
    reserved: usize,
) -> SyscallResult {
    use abi::telemetry::request;
    // The dispatch gate already required `observe_tasks` (X). Arguments
    // are refused before anything is sampled.
    if !crate::task::telemetry::request_is_valid(out_ptr, out_len, flags, reserved) {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(out_ptr, out_len, MAX_USER_BUF)?;
    let memory_allowed = flags & request::SYSTEM_MEMORY != 0
        && caller_has_cap(caller_id, |rights| rights.observe_memory);
    let epoch = crate::task::telemetry::provider_epoch();
    // Identity, membership and CPU in one SCHEDULER hold. The transient
    // rows are allocated there, charged to the caller like GetProcs2.
    let capture = {
        let mut guard = crate::task::SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
        crate::task::telemetry::capture(
            sched,
            epoch,
            crate::task::telemetry::monotonic_ns(),
            out_len,
            flags & request::TASKS != 0,
        )
        .map_err(|_| SyscallError::OutOfMemory)?
    };
    // Frames are read under FRAME_ALLOCATOR only after SCHEDULER is released.
    let memory = if flags & request::SYSTEM_MEMORY == 0 {
        crate::task::telemetry::SystemMemory::NotRequested
    } else if !memory_allowed {
        crate::task::telemetry::SystemMemory::Denied
    } else {
        let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        match guard.as_ref() {
            Some(allocator) => {
                crate::task::telemetry::SystemMemory::Present(abi::syscall::ViMemInfoV1 {
                    total_frames: allocator.total_frames() as u64,
                    used_frames: allocator.used_frames() as u64,
                    free_frames: allocator.free_frames() as u64,
                    page_size: allocator.page_size() as u64,
                })
            }
            None => crate::task::telemetry::SystemMemory::Unavailable,
        }
    };
    let bytes = capture.finish(memory, crate::task::telemetry::monotonic_ns());
    write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
    Ok(bytes.len())
}

/// Handler for `Syscall::StateStash` (extracted from the dispatcher).
pub(super) fn state_stash(
    caller_id: usize,
    key: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    let raw_key = key as u64;
    let is_shell =
        caller_launch_state(caller_id).is_some_and(|(name, _, _)| name.as_str() == "cellbox");
    if is_shell
        && (raw_key != SPAWN_ARGV_KEY || buf_len > crate::cell::state_stash::MAX_SPAWN_ARGV_LEN)
    {
        return Err(SyscallError::PermissionDenied);
    }
    if raw_key == SPAWN_ARGV_KEY {
        let bytes = read_user_slice(
            caller_id,
            buf_ptr,
            buf_len,
            crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
        )?;
        return Ok(crate::cell::state_stash::stage_spawn_argv(
            caller_id, &bytes,
        ));
    }
    let owner = stash_incarnation(caller_id)?;
    let bytes = read_user_slice(
        caller_id,
        buf_ptr,
        buf_len,
        crate::cell::state_stash::MAX_STASH_LEN,
    )?;
    let stored = crate::cell::state_stash::stash(owner, raw_key, &bytes);
    #[cfg(feature = "bench-counters")]
    if stored > 0 {
        crate::bench_counters::milestone("state-stashed", caller_id);
    }
    Ok(stored)
}

/// Handler for `Syscall::StateRestore` (extracted from the dispatcher).
pub(super) fn state_restore(
    caller_id: usize,
    key: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    if key as u64 == SPAWN_ARGV_KEY {
        validate_user_buf(
            buf_ptr,
            buf_len,
            crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
        )?;
        let Some(argv) = crate::cell::state_stash::take_inherited_argv(caller_id) else {
            return Ok(0);
        };
        let n = argv.len().min(buf_len);
        if n > 0 {
            write_user_slice(
                caller_id,
                buf_ptr,
                &argv[..n],
                crate::cell::state_stash::MAX_SPAWN_ARGV_LEN,
            )?;
        }
        return Ok(n);
    }
    let reader = stash_incarnation(caller_id)?;
    // Empty buffer: length probe only (owner, destination or supervisor).
    if buf_len == 0 {
        let supervisor = caller_has_supervisor(caller_id);
        return Ok(crate::cell::state_stash::status(
            reader, supervisor, key as u64,
        ));
    }
    validate_user_buf(buf_ptr, buf_len, crate::cell::state_stash::MAX_STASH_LEN)?;
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(buf_len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(buf_len, 0);
    let n = crate::cell::state_stash::restore(reader, key as u64, &mut kbuf);
    if n > 0 {
        write_user_slice(
            caller_id,
            buf_ptr,
            &kbuf[..n],
            crate::cell::state_stash::MAX_STASH_LEN,
        )?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::milestone("state-restored", caller_id);
    }
    Ok(n)
}

/// Handler for `Syscall::StateStashClear` (extracted from the dispatcher).
pub(super) fn state_stash_clear(caller_id: usize, key: usize) -> SyscallResult {
    if key as u64 == SPAWN_ARGV_KEY {
        crate::cell::state_stash::discard_spawn_argv(caller_id);
    } else {
        let caller = stash_incarnation(caller_id)?;
        let supervisor = caller_has_supervisor(caller_id);
        if !crate::cell::state_stash::clear(caller, supervisor, key as u64) {
            return Err(SyscallError::PermissionDenied);
        }
    }
    Ok(0)
}

/// Handler for `Syscall::QueryHotswapReady` (extracted from the dispatcher).
pub(super) fn query_hotswap_ready(caller_id: usize, target_tid: usize) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    let ready = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|s| s.tasks.get(&target_tid).map(|t| t.hotswap.ready));
    match ready {
        Some(true) => Ok(1),
        Some(false) => Ok(0),
        None => Err(SyscallError::FileNotFound), // tid does not exist
    }
}

/// Handler for `Syscall::SpawnReplacement` (extracted from the dispatcher).
pub(super) fn spawn_replacement(
    caller_id: usize,
    old_tid: usize,
    path_ptr: usize,
    path_len: usize,
) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if old_tid == 0 || path_len == 0 || path_len > crate::loader::disk_layout::MAX_CELL_PATH {
        return Err(SyscallError::InvalidInput);
    }
    // The machine-pin provider is never swapped (`swap_refused`).
    if crate::cell::service_registry::is_swap_refused_tid(old_tid) {
        return Err(SyscallError::PermissionDenied);
    }
    let path_str = read_user_string(
        caller_id,
        path_ptr,
        path_len,
        crate::loader::disk_layout::MAX_CELL_PATH,
    )?;
    if !path_str.starts_with('/') {
        return Err(SyscallError::InvalidInput);
    }
    let profile = authorize_launch_edge(
        caller_id,
        crate::loader::launch_profile::LaunchRoute::Path,
        &path_str,
    )?;
    let replacement = crate::cell::hotswap::reserve_frozen_replacement(old_tid)
        .ok_or(SyscallError::PermissionDenied)?;
    let old_incarnation = stash_incarnation(old_tid)?;
    let request = match governed_replacement_request(caller_id, profile.child_ceiling, replacement)
    {
        Ok(request) => request,
        Err(error) => {
            crate::task::dir_inherit::clear_staged(caller_id);
            return Err(error);
        }
    };
    let spawned = crate::loader::spawn_from_path(&path_str, request);
    // Mirror the regular spawn contract: a successful replacement spawn
    // consumes any staged directory set, and a failed one must clear it
    // so it cannot attach to an unrelated later child.
    crate::task::dir_inherit::clear_staged(caller_id);
    let task_id = spawned.map_err(|e| match e {
        types::ViError::NotFound => SyscallError::FileNotFound,
        types::ViError::OutOfMemory => {
            log::warn!(
                "[loader] spawn OOM: op=SpawnReplacement caller={} old_tid={} path={}",
                caller_id,
                old_tid,
                path_str
            );
            SyscallError::OutOfMemory
        }
        types::ViError::PermissionDenied => SyscallError::PermissionDenied,
        _ => SyscallError::InvalidInput,
    })?;
    // Hand the frozen incarnation's stash entries to exactly this
    // replacement: the old writer is fenced, only the new one may read.
    if let Ok(new_incarnation) = stash_incarnation(task_id) {
        crate::cell::state_stash::seal_for_replacement(old_incarnation, new_incarnation);
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::milestone("replacement-sealed", task_id);
    }
    Ok(task_id)
}

/// Handler for `Syscall::HotSwapReady` (extracted from the dispatcher).
pub(super) fn hot_swap_ready(caller_id: usize) -> SyscallResult {
    // The new cell signals that it has finished deserializing state.
    // Set the per-task flag; the supervisor cutover path polls this.
    // No SpawnCap required — only the new cell itself calls this
    // after its restore flow finishes.
    crate::cell::hotswap::set_task_hotswap_ready(caller_id);
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::milestone("hotswap-ready", caller_id);
    Ok(0)
}

/// Handler for `Syscall::Snapshot` (extracted from the dispatcher).
pub(super) fn snapshot(caller_id: usize) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        log::warn!("[snapshot] denied: caller {caller_id} has no SupervisorCap");
        return Err(SyscallError::PermissionDenied);
    }
    // Cells must be quiesced before calling this (all at yield points).
    // For MVP: the shell is the only active task while the snapshot runs.
    match crate::snapshot::serialize_snapshot() {
        Ok(frame_count) => Ok(frame_count as usize),
        Err(reason) => {
            log::warn!("[snapshot] unavailable: {reason}");
            Err(SyscallError::Unknown)
        }
    }
}
