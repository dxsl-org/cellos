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
    sched: &crate::sched::scheduler::Scheduler,
    sender_tid: usize,
) -> (u64, u64) {
    crate::task::sender_context(sched, sender_tid)
}

/// Handler for `Syscall::IpcSubmit` (extracted from the dispatcher).
pub(super) fn ipc_submit(caller_id: usize, target: usize, ptr: usize, len: usize) -> SyscallResult {
    crate::task::async_ipc::submit(caller_id, target, ptr, len).map_err(ipc_error)
}

/// Handler for `Syscall::IpcTake` (extracted from the dispatcher).
pub(super) fn ipc_take(
    caller_id: usize,
    token: usize,
    ptr: usize,
    len: usize,
    status_ptr: usize,
) -> SyscallResult {
    crate::task::async_ipc::take(caller_id, token, ptr, len, status_ptr)
        .map(usize::from)
        .map_err(ipc_error)
}

/// Handler for `Syscall::IpcWait` (extracted from the dispatcher).
pub(super) fn ipc_wait(caller_id: usize, ticks: u64) -> SyscallResult {
    crate::task::async_ipc::wait(caller_id, ticks)
        .map(usize::from)
        .map_err(ipc_error)
}

/// Handler for `Syscall::IpcCancel` (extracted from the dispatcher).
pub(super) fn ipc_cancel(caller_id: usize, token: usize) -> SyscallResult {
    crate::task::async_ipc::cancel(caller_id, token)
        .map(|()| 0)
        .map_err(ipc_error)
}

/// Handler for `Syscall::IpcCurrent` (extracted from the dispatcher).
pub(super) fn ipc_current(caller_id: usize) -> SyscallResult {
    Ok(crate::task::async_ipc::current(caller_id))
}

/// Handler for `Syscall::IpcReply` (extracted from the dispatcher).
pub(super) fn ipc_reply(caller_id: usize, token: usize, ptr: usize, len: usize) -> SyscallResult {
    // An explicit reply is the same holder-quiescence boundary as Send.
    // A stale/cancelled token may already be gone from the client, but
    // must still release only this exact current VFS request's lease.
    let vfs_release = if token != 0 && crate::task::async_ipc::current(caller_id) == token {
        match current_vfs_grant_lookup(caller_id) {
            VfsGrantLookup::Active(context) => Some(context),
            VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
        }
    } else {
        None
    };
    let out = crate::task::async_ipc::reply(caller_id, token, ptr, len)
        .map(|()| 0)
        .map_err(ipc_error);
    if let Some(context) = vfs_release {
        finish_vfs_send_release(caller_id, context.grant_owner, Some(context));
    }
    out
}

/// Handler for `Syscall::Send` (extracted from the dispatcher).
pub(super) fn send(
    caller_id: usize,
    target: usize,
    msg_ptr: usize,
    msg_len: usize,
) -> SyscallResult {
    crate::audit::log_event(
        crate::audit::AuditEvent::IpcSend,
        &crate::audit::encode_u32x2(caller_id as u32, target as u32),
    );
    let vfs_release = match current_vfs_grant_lookup(caller_id) {
        VfsGrantLookup::Active(context) => Some(context),
        VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
    };
    if let Some(outcome) =
        crate::task::async_ipc::reply_current(caller_id, target, msg_ptr, msg_len)
    {
        let out = outcome.map(|()| 0).map_err(ipc_error);
        finish_vfs_send_release(caller_id, target, vfs_release);
        return out;
    }
    let res = crate::task::ipc_send(caller_id, target, msg_ptr, msg_len);
    let out = match res {
        Ok(0) => Ok(0),
        Ok(1) => {
            crate::task::yield_cpu(); // Blocked
            if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
                Ok(sched
                    .tasks
                    .get(&caller_id)
                    .and_then(|t| t.reply_value)
                    .unwrap_or(0))
            } else {
                Ok(0)
            }
        }
        Err(crate::task::IpcSendError::Backpressure) => Err(SyscallError::TryAgain),
        Err(crate::task::IpcSendError::TargetGone) => Err(SyscallError::InvalidCommand),
        _ => Ok(0),
    };
    finish_vfs_send_release(caller_id, target, vfs_release);
    out
}

/// Handler for `Syscall::TrySend` (extracted from the dispatcher).
pub(super) fn try_send(
    caller_id: usize,
    target: usize,
    msg_ptr: usize,
    msg_len: usize,
) -> SyscallResult {
    // Non-blocking: deliver if target in Recv, else drop. Never blocks.
    // Target 0 is resolved to caller_id for self-wake without an extra tid lookup.
    let target_id = if target == 0 { caller_id } else { target };
    match crate::task::ipc_try_send(caller_id, target_id, msg_ptr, msg_len) {
        Ok(()) => Ok(0), // delivered (or queued for input service / same-cell wake)
        Err(()) => Ok(usize::MAX), // dropped (target not ready or gone)
    }
}

/// Handler for `Syscall::Recv` (extracted from the dispatcher).
pub(super) fn recv(
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
    attest_caller: bool,
) -> SyscallResult {
    // Identity trailer, when requested, is written at each delivery point
    // AFTER the payload copy and AFTER the scheduler lock is dropped —
    // `attested_identity_of` takes that lock itself.
    let attest = |sender_tid: usize| {
        if attest_caller {
            write_caller_identity(caller_id, buf_ptr, buf_len, sender_tid);
        }
    };

    let death_info = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().and_then(|sched| {
            let t = sched.tasks.get_mut(&caller_id)?;
            let owner_death = crate::task::Task::owner_death_matches_receive_mask(mask)
                .then(|| t.pending_owner_deaths.first().copied())
                .flatten()
                .map(|(_, dead_tid, reason)| (true, dead_tid, reason));
            let death = owner_death.or_else(|| {
                t.pending_deaths
                    .first()
                    .copied()
                    .map(|(dead_tid, reason)| (false, dead_tid, reason))
            });
            death.map(|(is_owner, dead_tid, reason)| {
                let caller_view = TaskCopyView::of(t);
                (is_owner, dead_tid, reason, caller_view)
            })
        })
    };

    if let Some((is_owner, dead_tid, reason, caller_view)) = death_info {
        if buf_len >= core::mem::size_of::<u64>() {
            validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
            let reason_bytes = (reason as u64).to_ne_bytes();
            caller_view
                .write_bytes(buf_ptr, &reason_bytes)
                .map_err(|_| SyscallError::InvalidInput)?;
        }
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(t) = sched.tasks.get_mut(&caller_id) {
                if is_owner {
                    if !t.pending_owner_deaths.is_empty() {
                        t.pending_owner_deaths.remove(0);
                    }
                } else if !t.pending_deaths.is_empty() {
                    t.pending_deaths.remove(0);
                }
            }
        }
        attest(dead_tid);
        return Ok(dead_tid);
    }

    // Hot-swap pending-message drain — guaranteed delivery path.
    // Payload snapshot is fallible (never an infallible Vec::from);
    // wire records carry scalar identity and gate the sender wake.
    let pending_msg_info: Result<Option<_>, ()> = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().map_or(Ok(None), |sched| {
            let Some(t) = sched.tasks.get_mut(&caller_id) else {
                return Ok(None);
            };
            let Some(pos) = t
                .pending_msgs
                .iter()
                .position(|m| mask == 0 || m.sender_tid == mask)
            else {
                return Ok(None);
            };
            let msg = &t.pending_msgs.as_slice()[pos];
            let sender_tid = msg.sender_tid;
            let wire_header = msg.wire_header();
            // Allocation failure is a real error, not "no message":
            // encoding it as None would block despite queued data.
            // Stage only what fits the caller's buffer; the record is
            // removed whole, so the rest would be copied for nothing.
            let payload = msg.payload();
            let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
            let caller_view = TaskCopyView::of(t);
            let (sender_cell_id, sender_generation) = match wire_header {
                Some(header) => (header.sender_cell_id, header.sender_generation),
                None => sender_cell_context_in_sched(sched, sender_tid),
            };
            let info = (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            );
            if let Some(header) = wire_header {
                crate::task::async_ipc::dispatch(sched, caller_id, header);
            }
            Ok(Some(info))
        })
    };
    let (pos, sender_tid, sender_cell_id, sender_generation, wire_header, caller_view, data) =
        match pending_msg_info {
            Err(()) => return Err(SyscallError::OutOfMemory),
            Ok(None) => {
                // A receive masked to a peer that is already gone, with no
                // message of its left to deliver, would park forever: the
                // exit-time wake (scheduler `exit_task`) fired before this
                // receive existed. Complete it as that death, reason 0 like
                // NotifyOnExit's already-dead target.
                if mask != 0 {
                    let peer_gone = crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
                        sched.tasks.get(&mask).is_none_or(|peer| {
                            matches!(
                                peer.state,
                                crate::task::tcb::TaskState::Retiring
                                    | crate::task::tcb::TaskState::Terminated
                            )
                        })
                    });
                    if peer_gone {
                        if buf_len >= core::mem::size_of::<u64>() {
                            validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
                            let view = caller_copy_view(caller_id)?;
                            view.write_bytes(buf_ptr, &0u64.to_ne_bytes())
                                .map_err(|_| SyscallError::InvalidInput)?;
                        }
                        attest(mask);
                        return Ok(mask);
                    }
                }
                let res = crate::task::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                match res {
                    Ok(0) => {
                        crate::task::yield_cpu();
                        // Peek the next event without removing it,
                        // copy payload, then commit the exact record.
                        let snap = {
                            let mut guard = crate::task::SCHEDULER.lock();
                            guard
                                .as_mut()
                                .map_or(Ok(ResumeSnapshot::Wake { sender_tid: 0 }), |sched| {
                                    snapshot_resume_dispatched(sched, caller_id, mask)
                                })
                        };
                        let snap = match snap {
                            Err(()) => return Err(SyscallError::OutOfMemory),
                            Ok(s) => s,
                        };
                        // Copy-out before any commit.
                        let sender = match &snap {
                            ResumeSnapshot::Death {
                                sender_tid, reason, ..
                            } => {
                                if buf_len >= core::mem::size_of::<u64>() {
                                    validate_user_buf(
                                        buf_ptr,
                                        core::mem::size_of::<u64>(),
                                        MAX_USER_BUF,
                                    )?;
                                    let reason_bytes = (*reason as u64).to_ne_bytes();
                                    let view = caller_copy_view(caller_id)?;
                                    view.write_bytes(buf_ptr, &reason_bytes)
                                        .map_err(|_| SyscallError::InvalidInput)?;
                                }
                                *sender_tid
                            }
                            ResumeSnapshot::Message {
                                sender_tid,
                                payload,
                                ..
                            } => {
                                let copy_len = core::cmp::min(payload.len(), buf_len);
                                if copy_len > 0 {
                                    validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                                    let view = caller_copy_view(caller_id)?;
                                    view.write_bytes(buf_ptr, &payload[..copy_len])
                                        .map_err(|_| SyscallError::InvalidInput)?;
                                    #[cfg(feature = "bench-counters")]
                                    crate::bench_counters::ipc_bytes(copy_len);
                                }
                                *sender_tid
                            }
                            ResumeSnapshot::Wake { sender_tid } => *sender_tid,
                        };
                        // Copy succeeded: commit the exact record.
                        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                commit_resume(task, &snap);
                                if let ResumeSnapshot::Message {
                                    sender_tid,
                                    sender_cell_id,
                                    sender_generation,
                                    wire_header,
                                    ..
                                } = &snap
                                {
                                    task.set_received_request_context(
                                        *sender_tid,
                                        *sender_cell_id,
                                        *sender_generation,
                                        wire_header.map_or(0, |header| header.async_op),
                                    );
                                }
                            }
                            if let ResumeSnapshot::Message {
                                sender_tid,
                                wire_header: Some(header),
                                ..
                            } = snap
                            {
                                crate::task::wake_sender_token(
                                    sched, sender_tid, caller_id, header,
                                );
                            }
                        }
                        attest(sender);
                        return Ok(sender);
                    }
                    Ok(id) => {
                        attest(id);
                        return Ok(id);
                    }
                    Err(_) => return Err(SyscallError::InvalidCommand),
                }
            }
            Ok(Some(snapshot)) => snapshot,
        };
    let copy_len = core::cmp::min(data.len(), buf_len);
    if copy_len > 0 {
        validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
        caller_view
            .write_bytes(buf_ptr, &data[..copy_len])
            .map_err(|_| SyscallError::InvalidInput)?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::ipc_bytes(copy_len);
    }
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(t) = sched.tasks.get_mut(&caller_id) {
            let _ = t.pending_msgs.remove_delivered(
                pos,
                sender_tid,
                wire_header.map(|header| header.delivery_id),
            );
            t.set_received_request_context(
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header.map_or(0, |header| header.async_op),
            );
        }
        if let Some(header) = wire_header {
            crate::task::wake_sender_token(sched, sender_tid, caller_id, header);
        }
    }
    attest(sender_tid);
    Ok(sender_tid)
}

/// Handler for `Syscall::SendGather` (extracted from the dispatcher).
pub(super) fn send_gather(
    caller_id: usize,
    target: usize,
    iovec_ptr: usize,
    iovec_count: usize,
) -> SyscallResult {
    const MAX_IOVEC: usize = 8;
    const IOVEC_ENTRY: usize = core::mem::size_of::<usize>() * 2;
    if iovec_count == 0 || iovec_count > MAX_IOVEC {
        return Err(SyscallError::InvalidInput);
    }
    let caller_view = caller_copy_view(caller_id)?;
    let mut iovec_entries = alloc::vec::Vec::with_capacity(iovec_count);
    let mut total = 0usize;
    for i in 0..iovec_count {
        let base = iovec_ptr + i * IOVEC_ENTRY;
        let mut entry_bytes = [0u8; IOVEC_ENTRY];
        caller_view
            .read_into(base, &mut entry_bytes)
            .map_err(|_| SyscallError::InvalidInput)?;
        let ptr = usize::from_ne_bytes(
            entry_bytes[..core::mem::size_of::<usize>()]
                .try_into()
                .unwrap(),
        );
        let len = usize::from_ne_bytes(
            entry_bytes[core::mem::size_of::<usize>()..]
                .try_into()
                .unwrap(),
        );
        validate_user_buf(ptr, len, MAX_USER_BUF)?;
        total = total.checked_add(len).ok_or(SyscallError::BufferTooSmall)?;
        if total > MAX_USER_BUF {
            return Err(SyscallError::BufferTooSmall);
        }
        iovec_entries.push((ptr, len));
    }
    let mut gathered = alloc::vec::Vec::new();
    gathered
        .try_reserve_exact(total)
        .map_err(|_| SyscallError::OutOfMemory)?;
    gathered.resize(total, 0);
    let mut pos = 0;
    for (ptr, len) in iovec_entries {
        if len > 0 {
            caller_view
                .read_into(ptr, &mut gathered[pos..pos + len])
                .map_err(|_| SyscallError::InvalidInput)?;
            pos += len;
        }
    }
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::ipc_bytes(total);
    crate::task::ipc_post_nonblock(caller_id, target, &gathered)
        .map_err(|_| SyscallError::TryAgain)?;
    Ok(0)
}

/// Handler for `Syscall::RecvScatter` (extracted from the dispatcher).
pub(super) fn recv_scatter(
    caller_id: usize,
    mask: usize,
    iovec_ptr: usize,
    iovec_count: usize,
) -> SyscallResult {
    const MAX_IOVEC: usize = 8;
    const IOVEC_ENTRY: usize = core::mem::size_of::<usize>() * 2;
    if iovec_count == 0 || iovec_count > MAX_IOVEC {
        return Err(SyscallError::InvalidInput);
    }
    let caller_view = caller_copy_view(caller_id)?;
    let mut iovec_entries = alloc::vec::Vec::with_capacity(iovec_count);
    let mut total = 0usize;
    for i in 0..iovec_count {
        let base = iovec_ptr + i * IOVEC_ENTRY;
        let mut entry_bytes = [0u8; IOVEC_ENTRY];
        caller_view
            .read_into(base, &mut entry_bytes)
            .map_err(|_| SyscallError::InvalidInput)?;
        let ptr = usize::from_ne_bytes(
            entry_bytes[..core::mem::size_of::<usize>()]
                .try_into()
                .unwrap(),
        );
        let len = usize::from_ne_bytes(
            entry_bytes[core::mem::size_of::<usize>()..]
                .try_into()
                .unwrap(),
        );
        validate_user_buf(ptr, len, MAX_USER_BUF)?;
        total = total.checked_add(len).ok_or(SyscallError::BufferTooSmall)?;
        if total > MAX_USER_BUF {
            return Err(SyscallError::BufferTooSmall);
        }
        iovec_entries.push((ptr, len));
    }
    // Multi-range receive transaction: snapshot/stage the message
    // payload in kernel memory, then stage and commit all destination
    // ranges via the copy boundary's atomic scatter write.
    // No destination is touched and no message is consumed if ANY
    // iovec range is invalid, unmapped, or lacks write permissions.
    let drained: Result<Option<_>, ()> = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().map_or(Ok(None), |sched| {
            let Some(t) = sched.tasks.get_mut(&caller_id) else {
                return Ok(None);
            };
            let Some(pos) = t
                .pending_msgs
                .iter()
                .position(|m| mask == 0 || m.sender_tid == mask)
            else {
                return Ok(None);
            };
            let record = &t.pending_msgs.as_slice()[pos];
            let sender_tid = record.sender_tid;
            let wire_header = record.wire_header();
            let payload = record.payload();
            // Stage only what the iovecs can take (see `Recv`).
            let tmp = MsgPayload::from_slice(&payload[..payload.len().min(total)])?;
            if let Some(header) = wire_header {
                crate::task::async_ipc::dispatch(sched, caller_id, header);
            }
            Ok(Some((pos, sender_tid, wire_header, tmp)))
        })
    };
    let (pos, sender_tid, wire_header, tmp) = match drained {
        Err(()) => return Err(SyscallError::OutOfMemory),
        Ok(None) => return Err(SyscallError::TryAgain),
        Ok(Some(snapshot)) => snapshot,
    };
    let copy_total = core::cmp::min(tmp.len(), total);
    caller_view
        .write_scatter(&iovec_entries, &tmp[..copy_total])
        .map_err(|_| SyscallError::InvalidInput)?;
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::ipc_bytes(copy_total);
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(t) = sched.tasks.get_mut(&caller_id) {
            let _ = t.pending_msgs.remove_delivered(
                pos,
                sender_tid,
                wire_header.map(|header| header.delivery_id),
            );
            if let Some(header) = wire_header {
                // Commit caller context together with removal so a
                // reply after scatter carries the delivered sender's
                // identity and request generation.
                t.set_received_request_context(
                    sender_tid,
                    header.sender_cell_id,
                    header.sender_generation,
                    header.async_op,
                );
            }
        }
        if let Some(header) = wire_header {
            crate::task::wake_sender_token(sched, sender_tid, caller_id, header);
        }
    }
    Ok(sender_tid)
}

/// Handler for `Syscall::RecvTimeout` (extracted from the dispatcher).
pub(super) fn recv_timeout(
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
    deadline: u64,
) -> SyscallResult {
    // Queued death records first, exactly as the plain `Recv` arm delivers
    // them: a supervised loop that polls with a deadline (a supervisor with
    // backoff timers) must still see its children exit.
    if let Some(dead_tid) = take_queued_death(caller_id, mask, buf_ptr, buf_len)? {
        return Ok(dead_tid);
    }
    let mut vfs_context_drop = None;
    let pending_msg_info: Result<Option<_>, ()> = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().map_or(Ok(None), |sched| {
            let Some(t) = sched.tasks.get_mut(&caller_id) else {
                return Ok(None);
            };
            vfs_context_drop = t.begin_receive_context(mask);
            let Some(pos) = t
                .pending_msgs
                .iter()
                .position(|m| mask == 0 || m.sender_tid == mask)
            else {
                return Ok(None);
            };
            let record = &t.pending_msgs.as_slice()[pos];
            let sender_tid = record.sender_tid;
            let wire_header = record.wire_header();
            // Fallible snapshot: allocation failure must not be
            // mistaken for "no message" (the receiver would block
            // despite queued data).
            let payload = record.payload();
            // Stage only what fits the caller's buffer (see `Recv`).
            let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
            let caller_view = TaskCopyView::of(t);
            let (sender_cell_id, sender_generation) = match wire_header {
                Some(header) => (header.sender_cell_id, header.sender_generation),
                None => sender_cell_context_in_sched(sched, sender_tid),
            };
            let info = (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            );
            if let Some(header) = wire_header {
                crate::task::async_ipc::dispatch(sched, caller_id, header);
            }
            Ok(Some(info))
        })
    };
    finish_vfs_context_drop(caller_id, vfs_context_drop);

    let (pos, sender_tid, sender_cell_id, sender_generation, wire_header, caller_view, data) =
        match pending_msg_info {
            Err(()) => return Err(SyscallError::OutOfMemory),
            Ok(None) => {
                // Fast path: check for a pending message immediately.
                let res = crate::task::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                match res {
                    Ok(0) => {
                        // Blocked with deadline:None — install the absolute deadline.
                        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                if let crate::task::tcb::TaskState::Recv {
                                    deadline: ref mut d,
                                    ..
                                } = task.state
                                {
                                    *d = Some(deadline);
                                }
                            }
                        }
                        // Yield so the scheduler runs other tasks and can fire the timeout.
                        crate::task::yield_cpu();
                        let snap = {
                            let mut guard = crate::task::SCHEDULER.lock();
                            guard
                                .as_mut()
                                .map_or(Ok(ResumeSnapshot::Wake { sender_tid: 0 }), |sched| {
                                    snapshot_resume_dispatched(sched, caller_id, mask)
                                })
                        };
                        let snap = match snap {
                            Err(()) => return Err(SyscallError::OutOfMemory),
                            Ok(s) => s,
                        };
                        let sender = match &snap {
                            ResumeSnapshot::Death {
                                sender_tid, reason, ..
                            } => {
                                if buf_len >= core::mem::size_of::<u64>() {
                                    validate_user_buf(
                                        buf_ptr,
                                        core::mem::size_of::<u64>(),
                                        MAX_USER_BUF,
                                    )?;
                                    let reason_bytes = (*reason as u64).to_ne_bytes();
                                    let view = caller_copy_view(caller_id)?;
                                    view.write_bytes(buf_ptr, &reason_bytes)
                                        .map_err(|_| SyscallError::InvalidInput)?;
                                }
                                *sender_tid
                            }
                            ResumeSnapshot::Message {
                                sender_tid,
                                payload,
                                ..
                            } => {
                                let copy_len = core::cmp::min(payload.len(), buf_len);
                                if copy_len > 0 {
                                    validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
                                    let view = caller_copy_view(caller_id)?;
                                    view.write_bytes(buf_ptr, &payload[..copy_len])
                                        .map_err(|_| SyscallError::InvalidInput)?;
                                    #[cfg(feature = "bench-counters")]
                                    crate::bench_counters::ipc_bytes(copy_len);
                                }
                                *sender_tid
                            }
                            ResumeSnapshot::Wake { sender_tid } => *sender_tid,
                        };
                        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                commit_resume(task, &snap);
                                if let ResumeSnapshot::Message {
                                    sender_tid,
                                    sender_cell_id,
                                    sender_generation,
                                    wire_header,
                                    ..
                                } = &snap
                                {
                                    task.set_received_request_context(
                                        *sender_tid,
                                        *sender_cell_id,
                                        *sender_generation,
                                        wire_header.map_or(0, |header| header.async_op),
                                    );
                                }
                            }
                            if let ResumeSnapshot::Message {
                                sender_tid,
                                wire_header: Some(header),
                                ..
                            } = snap
                            {
                                crate::task::wake_sender_token(
                                    sched, sender_tid, caller_id, header,
                                );
                            }
                        }
                        return Ok(sender);
                    }
                    Ok(id) => return Ok(id),
                    Err(_) => return Err(SyscallError::InvalidCommand),
                }
            }
            Ok(Some(snapshot)) => snapshot,
        };
    let copy_len = core::cmp::min(data.len(), buf_len);
    if copy_len > 0 {
        validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
        caller_view
            .write_bytes(buf_ptr, &data[..copy_len])
            .map_err(|_| SyscallError::InvalidInput)?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::ipc_bytes(copy_len);
    }
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(t) = sched.tasks.get_mut(&caller_id) {
            let _ = t.pending_msgs.remove_delivered(
                pos,
                sender_tid,
                wire_header.map(|header| header.delivery_id),
            );
            t.set_received_request_context(
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header.map_or(0, |header| header.async_op),
            );
        }
        if let Some(header) = wire_header {
            crate::task::wake_sender_token(sched, sender_tid, caller_id, header);
        }
    }
    Ok(sender_tid)
}

/// Handler for `Syscall::TryRecv` (extracted from the dispatcher).
pub(super) fn try_recv(
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
    attest_caller: bool,
) -> SyscallResult {
    // A queued owner death is delivered first, exactly as `Recv` does
    // for a wildcard receive: reason at the start of the buffer, zero
    // trailer for the dead (unattributable) root, dead root TID returned.
    // `NotifyOnExit` deaths stay out of `TryRecv` on purpose.
    let owner_death = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().and_then(|sched| {
            let t = sched.tasks.get_mut(&caller_id)?;
            let (token, root_tid, reason) = peek_owner_death_for_try_recv(t, mask)?;
            Some((token, root_tid, reason, TaskCopyView::of(t)))
        })
    };
    if let Some((token, root_tid, reason, caller_view)) = owner_death {
        // Validate and copy before removing: a failed copy leaves the
        // event queued, the same two-phase order as `Recv`.
        if buf_len >= core::mem::size_of::<u64>() {
            validate_user_buf(buf_ptr, core::mem::size_of::<u64>(), MAX_USER_BUF)?;
            caller_view
                .write_bytes(buf_ptr, &(reason as u64).to_ne_bytes())
                .map_err(|_| SyscallError::InvalidInput)?;
        }
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(t) = sched.tasks.get_mut(&caller_id) {
                remove_owner_death(t, token);
            }
        }
        if attest_caller {
            write_caller_identity(caller_id, buf_ptr, buf_len, root_tid);
        }
        return Ok(root_tid);
    }
    let mut vfs_context_drop = None;
    let pending_msg_info: Result<Option<_>, ()> = {
        let mut guard = crate::task::SCHEDULER.lock();
        guard.as_mut().map_or(Ok(None), |sched| {
            let Some(t) = sched.tasks.get_mut(&caller_id) else {
                return Ok(None);
            };
            vfs_context_drop = t.begin_receive_context(mask);
            let Some(pos) = t
                .pending_msgs
                .iter()
                .position(|m| mask == 0 || m.sender_tid == mask)
            else {
                return Ok(None);
            };
            let msg = &t.pending_msgs.as_slice()[pos];
            let sender_tid = msg.sender_tid;
            let wire_header = msg.wire_header();
            let delivery_id = wire_header.as_ref().map(|h| h.delivery_id);
            // Extract cell context before dropping t to avoid
            // holding a mutable borrow of sched.tasks while calling
            // sender_cell_context_in_sched (which borrows sched immutably).
            let inline_cell_context = wire_header
                .as_ref()
                .map(|h| (h.sender_cell_id, h.sender_generation));
            let caller_view = TaskCopyView::of(t);
            let payload = msg.payload();
            // Stage only what fits the caller's buffer (see `Recv`).
            let data = MsgPayload::from_slice(&payload[..payload.len().min(buf_len)])?;
            // t is no longer borrowed here.
            let (sender_cell_id, sender_generation) = inline_cell_context
                .unwrap_or_else(|| sender_cell_context_in_sched(sched, sender_tid));
            let info = (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                delivery_id,
                caller_view,
                data,
            );
            if let Some(header) = wire_header {
                crate::task::async_ipc::dispatch(sched, caller_id, header);
            }
            Ok(Some(info))
        })
    };
    finish_vfs_context_drop(caller_id, vfs_context_drop);

    if let Some((
        pos,
        sender_tid,
        sender_cell_id,
        sender_generation,
        wire_header,
        delivery_id,
        caller_view,
        data,
    )) = match pending_msg_info {
        Err(()) => return Err(SyscallError::OutOfMemory),
        Ok(v) => v,
    } {
        let copy_len = core::cmp::min(data.len(), buf_len);
        if copy_len > 0 {
            validate_user_buf(buf_ptr, copy_len, MAX_USER_BUF)?;
            caller_view
                .write_bytes(buf_ptr, &data[..copy_len])
                .map_err(|_| SyscallError::InvalidInput)?;
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::ipc_bytes(copy_len);
        }
        if attest_caller {
            write_caller_identity(caller_id, buf_ptr, buf_len, sender_tid);
        }
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(t) = sched.tasks.get_mut(&caller_id) {
                let _ = t
                    .pending_msgs
                    .remove_delivered(pos, sender_tid, delivery_id);
                t.set_received_request_context(
                    sender_tid,
                    sender_cell_id,
                    sender_generation,
                    wire_header.map_or(0, |header| header.async_op),
                );
            }
            if let Some(header) = wire_header {
                crate::task::wake_sender_token(sched, sender_tid, caller_id, header);
            }
        }
        return Ok(sender_tid);
    }

    // Non-blocking Recv (scan Sending tasks)
    let res = crate::task::ipc_try_recv(caller_id, mask, buf_ptr, buf_len);
    match res {
        Ok(id) => {
            if attest_caller && id > 0 {
                write_caller_identity(caller_id, buf_ptr, buf_len, id);
            }
            Ok(id)
        }
        Err(_) => Err(SyscallError::InvalidCommand),
    }
}

/// Handler for `Syscall::Wait` (extracted from the dispatcher).
pub(super) fn wait(caller_id: usize, pid: usize) -> SyscallResult {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        let already_reaped = if let Some(target) = sched.tasks.get_mut(&pid) {
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                Some(crate::p01_resources::owner(target)),
                crate::memory::heap::p01_owner::TASK_FIELDS,
            );
            target.waiters.push(caller_id);
            false
        } else if let Some(zombie) = sched.zombies.iter_mut().find(|task| task.id == pid) {
            // It exited but a hart may still hold its saved context.
            // The zombie reaper transfers these waiters only after the
            // exact selected/executing quiescence proof.
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                Some(crate::p01_resources::owner(zombie)),
                crate::memory::heap::p01_owner::TASK_FIELDS,
            );
            zombie.waiters.push(caller_id);
            false
        } else {
            true
        };
        if already_reaped {
            return Err(SyscallError::InvalidDriverId);
        }

        if let Some(caller) = sched.tasks.get_mut(&caller_id) {
            caller.state = TaskState::Waiting { target: pid };
        }
    }
    crate::task::yield_cpu();
    // Resume with exit code published by the quiescent zombie reaper.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        return Ok(sched
            .tasks
            .get(&caller_id)
            .and_then(|task| task.reply_value)
            .unwrap_or(0));
    }
    Ok(0)
}

/// Handler for `Syscall::FutexWait` (extracted from the dispatcher).
pub(super) fn futex_wait(
    caller_id: usize,
    addr: usize,
    expected: u32,
    timeout_ticks: u64,
) -> SyscallResult {
    // The wait parks the caller inside `futex::wait`; the outcome
    // (woken / value mismatch / timed out) is the syscall's return value,
    // so it needs no error encoding. An invalid address is a recoverable
    // error, never a kernel fault.
    crate::task::futex::wait(caller_id, addr, expected, timeout_ticks)
}

/// Handler for `Syscall::FutexWake` (extracted from the dispatcher).
pub(super) fn futex_wake(caller_id: usize, addr: usize, count: usize) -> SyscallResult {
    if let Ok(n) = crate::task::futex::wake(caller_id, addr, count) {
        Ok(n)
    } else {
        Err(SyscallError::Unknown) // Should not fail typically
    }
}

/// Handler for `Syscall::Log` (extracted from the dispatcher).
pub(super) fn log(caller_id: usize, msg_ptr: usize, msg_len: usize) -> SyscallResult {
    if let Ok(msg) = read_user_string(caller_id, msg_ptr, msg_len, MAX_LOG_MSG) {
        #[cfg(cellos_p01_trace)]
        {
            let owner = crate::task::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|task| {
                    let owner = crate::p01_measurement::Owner::of(task);
                    // The temporary user-copy buffer belongs to this Log
                    // invocation, including while a stage census reads it.
                    if let Ok(layout) = core::alloc::Layout::array::<u8>(msg.capacity()) {
                        crate::memory::heap::p01_owner::runtime_buffer(
                            msg.as_ptr() as usize,
                            layout,
                            owner,
                            crate::memory::heap::p01_owner::TASK_IO,
                        );
                    } else {
                        crate::p01_measurement::invalidate(crate::p01_measurement::COUNTER);
                    }
                    owner
                });
            if let Some(owner) = owner {
                crate::p01_measurement::stage(owner, &msg);
            }
        }
        crate::task::print_user_log(&msg, caller_id);
    }
    Ok(0)
}

/// Handler for `Syscall::Heartbeat` (extracted from the dispatcher).
pub(super) fn heartbeat(caller_id: usize, interval: usize) -> SyscallResult {
    // Open: a cell asserts its own liveness. Arms a deadline `interval` ticks
    // ahead; `pick_next` terminates the cell as HUNG if it lapses. interval=0
    // disables. Self-targeted only — a cell can only (re)arm its OWN deadline.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(t) = sched.tasks.get_mut(&caller_id) {
            t.heartbeat_deadline = if interval == 0 {
                None
            } else {
                Some(crate::task::system_ticks() as u64 + interval as u64)
            };
        }
    }
    Ok(0)
}

/// Handler for `Syscall::Reply` (extracted from the dispatcher).
pub(super) fn reply(caller_id: usize, result: usize) -> SyscallResult {
    crate::task::ipc_reply(caller_id, result).map_err(|_| SyscallError::InvalidCommand)
}

/// Handler for `Syscall::SetTimer` (extracted from the dispatcher).
pub(super) fn set_timer(deadline: usize) -> SyscallResult {
    // Check if deadline passed
    let now = crate::task::system_ticks();
    let wake_at = now + deadline;

    // Sleep!
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            task.state = TaskState::Sleeping { until: wake_at };
        }
    }
    // Yield CPU safely
    crate::task::yield_cpu();
    Ok(0)
}

/// Handler for `Syscall::GetProcs` (extracted from the dispatcher).
pub(super) fn get_procs(caller_id: usize, buf_ptr: usize, buf_len: usize) -> SyscallResult {
    let bytes_len = buf_len
        .checked_mul(core::mem::size_of::<api::syscall::ProcessInfo>())
        .ok_or(SyscallError::InvalidInput)?;
    validate_user_buf(buf_ptr, bytes_len, MAX_USER_BUF)?;

    let rows = collect_process_rows(buf_len, snapshot_process_info);
    let raw_bytes = unsafe {
        core::slice::from_raw_parts(
            rows.as_ptr() as *const u8,
            rows.len() * core::mem::size_of::<api::syscall::ProcessInfo>(),
        )
    };
    write_user_slice(caller_id, buf_ptr, raw_bytes, MAX_USER_BUF)?;
    Ok(rows.len())
}

/// Handler for `Syscall::GetProcs2` (extracted from the dispatcher).
pub(super) fn get_procs2(caller_id: usize, buf_ptr: usize, buf_len: usize) -> SyscallResult {
    let bytes_len = buf_len
        .checked_mul(core::mem::size_of::<api::syscall::ProcessInfoV2>())
        .ok_or(SyscallError::InvalidInput)?;
    validate_user_buf(buf_ptr, bytes_len, MAX_USER_BUF)?;

    let sample_ticks = crate::task::system_ticks() as u64;
    let rows = collect_process_rows(buf_len, |task| snapshot_process_info_v2(task, sample_ticks));
    let raw_bytes = unsafe {
        core::slice::from_raw_parts(
            rows.as_ptr() as *const u8,
            rows.len() * core::mem::size_of::<api::syscall::ProcessInfoV2>(),
        )
    };
    write_user_slice(caller_id, buf_ptr, raw_bytes, MAX_USER_BUF)?;
    Ok(rows.len())
}

/// Handler for `Syscall::MemInfo` (extracted from the dispatcher).
pub(super) fn mem_info(caller_id: usize, out_ptr: usize, out_len: usize) -> SyscallResult {
    let len = core::mem::size_of::<api::syscall::ViMemInfoV1>();
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
    let info = {
        let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = guard.as_ref().ok_or(SyscallError::OutOfMemory)?;
        api::syscall::ViMemInfoV1 {
            total_frames: allocator.total_frames() as u64,
            used_frames: allocator.used_frames() as u64,
            free_frames: allocator.free_frames() as u64,
            page_size: allocator.page_size() as u64,
        }
    };
    let raw_bytes = unsafe {
        core::slice::from_raw_parts(&info as *const api::syscall::ViMemInfoV1 as *const u8, len)
    };
    write_user_slice(caller_id, out_ptr, raw_bytes, MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::GetTime` (extracted from the dispatcher).
pub(super) fn get_time(op: usize) -> SyscallResult {
    match op {
        // op=0: raw monotonic ticks (arch-specific frequency)
        0 => {
            #[cfg(target_arch = "riscv64")]
            let t = hal::common::timer::read_mtime() as usize;
            #[cfg(target_arch = "aarch64")]
            let t = hal::timer::read_ticks() as usize;
            #[cfg(target_arch = "x86_64")]
            let t = hal::clock::now_ns() as usize;
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            let t = 0usize;
            Ok(t)
        }
        // op=1: milliseconds since boot
        1 => {
            // 10 MHz mtime on QEMU RV64 → 10_000 ticks/ms
            #[cfg(target_arch = "riscv64")]
            let ms = (hal::common::timer::read_mtime() / 10_000) as usize;
            // Dynamic timer frequency on ARM64 (19.2 MHz on RPi3, 62.5 MHz on QEMU virt)
            #[cfg(target_arch = "aarch64")]
            let ms = {
                let freq = hal::timer::read_freq();
                let ticks_per_ms = if freq > 0 { freq / 1000 } else { 62_500 };
                (hal::timer::read_ticks() / ticks_per_ms) as usize
            };
            // The x86 clocksource already returns nanoseconds; ÷ 1_000_000 → ms
            #[cfg(target_arch = "x86_64")]
            let ms = (hal::clock::now_ns() / 1_000_000) as usize;
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            let ms = 0usize;
            Ok(ms)
        }
        // op=4: scheduler ticks (10 ms preemption slices)
        4 => Ok(crate::task::system_ticks()),
        // op=2: nanoseconds since Unix epoch (wall-clock)
        2 => {
            #[cfg(target_arch = "riscv64")]
            let ns = hal::common::rtc::now_epoch_ns() as usize;
            #[cfg(target_arch = "aarch64")]
            let ns = hal::rtc::now_epoch_ns() as usize;
            #[cfg(target_arch = "x86_64")]
            let ns = hal::rtc::now_epoch_ns() as usize;
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            let ns = 0usize;
            Ok(ns)
        }
        // op=3: seconds since Unix epoch (wall-clock)
        3 => {
            #[cfg(target_arch = "riscv64")]
            let s = (hal::common::rtc::now_epoch_ns() / 1_000_000_000) as usize;
            #[cfg(target_arch = "aarch64")]
            let s = (hal::rtc::now_epoch_ns() / 1_000_000_000) as usize;
            #[cfg(target_arch = "x86_64")]
            let s = (hal::rtc::now_epoch_ns() / 1_000_000_000) as usize;
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            let s = 0usize;
            Ok(s)
        }
        // op=5: monotonic tick counter frequency in Hz
        5 => {
            #[cfg(target_arch = "riscv64")]
            let freq = 10_000_000usize;
            #[cfg(target_arch = "aarch64")]
            let freq = hal::timer::read_freq() as usize;
            #[cfg(target_arch = "x86_64")]
            let freq = 1_000_000_000usize; // HPET is in nanoseconds (1 GHz)
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            let freq = 10_000_000usize;
            Ok(freq)
        }
        // Unknown op — return 0 for backward compatibility
        _ => Ok(0),
    }
}

/// Handler for `Syscall::ReadLog` (extracted from the dispatcher).
pub(super) fn read_log(
    caller_id: usize,
    buf_ptr: usize,
    max: usize,
    source: usize,
) -> SyscallResult {
    // Per-call staging cap: the user-log ring holds 8 KiB, so more is
    // zeroed heap that can never be filled.
    let cap = match source {
        0 => 8192,
        1 => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            65536
        }
        _ => return Err(SyscallError::InvalidInput),
    };
    if max == 0 {
        return Ok(0);
    }
    if buf_ptr == 0 {
        return Err(SyscallError::InvalidInput);
    }
    let max = max.min(cap);
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(max)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(max, 0);
    if source == 0 {
        let n = crate::task::read_log_ring(&mut kbuf);
        if n > 0 {
            write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
        }
        return Ok(n);
    }
    // Audit records leave the ring only once delivered: a faulting
    // buffer must not destroy the evidence it was meant to receive.
    let (start, n) = crate::audit::peek(&mut kbuf);
    if n > 0 {
        write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
        crate::audit::consume(start, n);
    }
    Ok(n)
}

/// Handler for `Syscall::WaitForEvent` (extracted from the dispatcher).
pub(super) fn wait_for_event(caller_id: usize, mask: u32, deadline: Option<u64>) -> SyscallResult {
    // Lost-wakeup guard: check pending events BEFORE parking.
    let already = crate::task::waker::consume_pending(mask);
    if already != 0 {
        return Ok(already as usize);
    }
    // Park: set WaitEvent state so the timer sweep can wake this task.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            task.state = crate::task::tcb::TaskState::WaitEvent { mask, deadline };
        }
    }
    // Yield; wake happens in pick_next's global sweep (hart 0).
    crate::task::yield_cpu();
    // After re-schedule: timer sweep wrote the fired mask into trap_frame.regs[10].
    // Return it so ViCell_syscall_dispatch writes the correct value back.
    let fired = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|t| t.trap_frame.regs[10])
        .unwrap_or(0);
    #[cfg(target_arch = "riscv32")]
    let fired = fired as usize;
    Ok(fired)
}

/// Handler for `Syscall::WaitCompletion` (extracted from the dispatcher).
pub(super) fn wait_completion(
    caller_id: usize,
    mask: u32,
    deadline: Option<u64>,
    out_ptr: usize,
) -> SyscallResult {
    crate::task::completion_wait::wait_completion(caller_id, mask, deadline, out_ptr)
}

/// Handler for `Syscall::GetRandom` (extracted from the dispatcher).
pub(super) fn get_random(caller_id: usize, buf_ptr: usize, len: usize) -> SyscallResult {
    if len == 0 {
        return Ok(0);
    }
    // Validate the ABI descriptor before entropy is consumed. `len`
    // remains the caller-declared capacity; only the output span caps
    // at 64 bytes, as required by the frozen syscall contract.
    validate_user_buf(buf_ptr, len, MAX_USER_BUF)?;
    let capped = len.min(64);
    preflight_user_output(caller_id, buf_ptr, capped)?;
    let mut kbuf = [0u8; 64];
    let written = crate::dev::virtio_rng::get_random(&mut kbuf[..capped]);
    let n = if written > 0 {
        written
    } else {
        #[cfg(all(feature = "dev-weak-rng", not(feature = "qemu-dev-https")))]
        {
            static WEAK_RNG_WARNED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !WEAK_RNG_WARNED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                log::warn!(
                    "[kernel] GetRandom: no hardware entropy — serving WEAK xorshift32 \
                             (dev-weak-rng). NOT cryptographically secure; never ship this."
                );
            }
            let seed =
                crate::task::system_ticks() as u32 ^ (caller_id as u32).wrapping_mul(0x9e37_79b9);
            let mut state = if seed == 0 { 1 } else { seed };
            for byte in kbuf[..capped].iter_mut() {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            capped
        }
        // The QEMU HTTPS image must fail closed even when development
        // defaults also enabled dev-weak-rng through feature unification.
        #[cfg(any(not(feature = "dev-weak-rng"), feature = "qemu-dev-https"))]
        0
    };
    if n > 0 {
        write_getrandom_output(caller_id, buf_ptr, &kbuf[..n])?;
    }
    Ok(n)
}
