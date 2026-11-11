// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: ipc (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, grant::*, misc::*, spawn::*, vm::*};

pub(super) fn ipc_error(error: crate::task::async_ipc::Failure) -> SyscallError {
    match error {
        crate::task::async_ipc::Failure::Busy => SyscallError::TryAgain,
        crate::task::async_ipc::Failure::PeerGone => SyscallError::FileNotFound,
        crate::task::async_ipc::Failure::Invalid => SyscallError::InvalidInput,
        crate::task::async_ipc::Failure::TooSmall => SyscallError::BufferTooSmall,
    }
}

pub(super) fn encode_ipc_result(result: SyscallResult) -> usize {
    match result {
        Ok(value) => value,
        Err(SyscallError::TryAgain) => usize::MAX - 1,
        Err(SyscallError::FileNotFound) => usize::MAX - 2,
        Err(SyscallError::InvalidInput | SyscallError::InvalidCommand) => usize::MAX - 3,
        Err(SyscallError::BufferTooSmall) => usize::MAX - 4,
        Err(_) => usize::MAX,
    }
}

/// Does this task hold the pipe endpoint? Takes `SCHEDULER`, so it must not be
/// called while that lock is held.
pub(super) fn caller_owns_pipe(caller_id: usize, handle: crate::task::pipe::PipeHandle) -> bool {
    crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .is_some_and(|task| task.pipe_handles.contains(&handle))
}

/// The oldest queued owner death a `TryRecv` with `mask` may take: only the
/// wildcard receive (`Task::owner_death_matches_receive_mask`), FIFO, and
/// `(token, dead_root_tid, exit_reason)` as queued. Nothing is removed here and
/// `pending_deaths` (NotifyOnExit) is never consulted: delivering those would
/// change every existing `TryRecv(0)` caller.
pub(super) fn peek_owner_death_for_try_recv(
    task: &crate::task::Task,
    mask: usize,
) -> Option<(u64, usize, usize)> {
    crate::task::Task::owner_death_matches_receive_mask(mask)
        .then(|| task.pending_owner_deaths.first().copied())
        .flatten()
}

/// Consume-and-deliver a queued death record for a receive that is **not** the
/// plain blocking `Recv`.
///
/// `Recv` checks `pending_deaths` before ordinary messages and *mask-agnostically*
/// (`Syscall::Recv`, first delivery branch). `RecvTimeout` used to skip that queue
/// entirely: `snapshot_resume` only knew owner-deaths, `pending_exit_reason`, and
/// `pending_msgs`, so a death queued while the watcher was running was never
/// delivered to a watcher that polls with a deadline. That breaks the
/// `NotifyOnExit` contract (Spec 12 §4.3) for exactly the loop shape a supervisor
/// uses — a mailbox with a deadline for its backoff timers — and the loss is
/// silent: the supervisor just never restarts the child.
///
/// Keep the order and the mask rule in step with the plain `Recv` arm.
pub(super) fn take_queued_death(
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> Result<Option<usize>, SyscallError> {
    let pending = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().and_then(|sched| {
            let t = sched.tasks.get_mut(&caller_id)?;
            let owner_death = crate::task::Task::owner_death_matches_receive_mask(mask)
                .then(|| t.pending_owner_deaths.first().copied())
                .flatten()
                .map(|(_, dead_tid, reason)| (true, dead_tid, reason));
            owner_death.or_else(|| {
                t.pending_deaths
                    .first()
                    .copied()
                    .map(|(dead_tid, reason)| (false, dead_tid, reason))
            })
        })
    };

    let Some((is_owner_death, dead_tid, reason)) = pending else {
        return Ok(None);
    };

    if buf_len >= core::mem::size_of::<u64>() {
        validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
        let view = caller_copy_view(caller_id)?;
        view.write_bytes(buf_ptr, &(reason as u64).to_ne_bytes())
            .map_err(|_| SyscallError::InvalidInput)?;
    }

    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(t) = sched.tasks.get_mut(&caller_id) {
            if is_owner_death {
                if !t.pending_owner_deaths.is_empty() {
                    t.pending_owner_deaths.remove(0);
                }
            } else if !t.pending_deaths.is_empty() {
                t.pending_deaths.remove(0);
            }
        }
    }

    Ok(Some(dead_tid))
}

pub(super) fn sender_cell_context_in_sched(
    sched: &crate::task::scheduler::Scheduler,
    sender_tid: usize,
) -> (u64, u64) {
    crate::task::sender_context(sched, sender_tid)
}
