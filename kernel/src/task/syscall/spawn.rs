// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: spawn (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, grant::*, ipc::*, misc::*, vm::*};

/// Called outside source locks after exact root teardown. Never infer pin/DMA or
/// historical heap ownership from grant-table absence.
#[cfg(cellos_p01_trace)]
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
pub(crate) enum ResumeSnapshot {
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
        wire_header: Option<crate::task::ipc_wire::IpcWireHeader>,
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
pub(crate) fn snapshot_resume(task: &crate::task::Task, mask: usize) -> Result<ResumeSnapshot, ()> {
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
            sender_tid: task.current_caller.unwrap_or(0),
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
        #[cfg(cellos_p01_trace)]
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
        sender_tid: task.current_caller.unwrap_or(0),
    })
}

pub(super) fn snapshot_process_info(task: &crate::task::tcb::Task) -> api::syscall::ProcessInfo {
    api::syscall::ProcessInfo {
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
    #[cfg(cellos_p01_trace)] trace: Option<crate::p01_measurement::Attempt>,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (incarnation, caller_authority, caller_root) = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| {
            let task = sched.tasks.get(&caller_id)?;
            #[cfg(cellos_p01_trace)]
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
pub(crate) fn launch_identity_in(
    sched: &crate::task::scheduler::Scheduler,
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
        api::TaskPriority::Normal as u8,
        None,
        names_child(task.root_tid, route),
    ))
}

/// Bind the dispatch boundary while the receive snapshot owns the scheduler
/// lock, before an independent caller can cancel and remove the queued record.
pub(super) fn snapshot_resume_dispatched(
    sched: &mut crate::task::scheduler::Scheduler,
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

pub(super) fn snapshot_process_info_v2(
    task: &crate::task::tcb::Task,
    sample_ticks: u64,
) -> api::syscall::ProcessInfoV2 {
    api::syscall::ProcessInfoV2 {
        id: task.id as u64,
        state: encode_task_state(&task.state),
        reserved0: 0,
        name: encode_task_name(&task.name),
        sample_ticks,
        cpu_run_ticks: task.cpu_run_ticks,
        heap_bytes: task_heap_bytes(task),
        owned_bytes: task_owned_bytes(task),
    }
}
