// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: authority (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{decode::*, dev::*, fs::*, grant::*, ipc::*, misc::*, spawn::*, vm::*};

/// Advance a row's domain lifecycle to `Revoked` outside every table lock.
///
/// Returns `false` when the receiver or owner invalidation is not acknowledged:
/// the caller must keep the row (so a retry is idempotent) and must not release
/// a single frame.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
pub(super) fn revoke_domain_record(record: &DomainRecord, id: usize, kind: &str) -> bool {
    match record.revoke() {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "[grant] {kind} {id:#x}: domain revoke deferred ({error:?}); frames retained and \
                 record kept Revoking for idempotent retry"
            );
            false
        }
    }
}

#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
pub(super) fn revoke_domain_record(_record: &DomainRecord, _id: usize, _kind: &str) -> bool {
    false
}

/// Returns `true` if the calling task satisfies the given capability check.
///
/// Lock-ordering: acquires SCHEDULER, drops before returning.
/// Gate on the caller's **Cell** rights (`Scheduler::cell_rights`, the root's
/// incarnation record), never on a per-thread copy: a revoke aimed at any
/// thread of the Cell is seen by all of them. A missing task fails closed.
pub(super) fn caller_has_cap<F: Fn(&crate::task::cap::CapSet) -> bool>(
    caller_id: usize,
    check: F,
) -> bool {
    crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.contains_key(&caller_id) && check(&sched.cell_rights(caller_id))
    })
}

/// Read a task's live Cell identity `(cell_id, generation)`.
///
/// Takes the scheduler lock itself, so a caller must not already hold it. The
/// service registry records this identity so `LookupServiceBound` can answer with
/// a binding instead of a bare tid; a task whose identity is missing yields
/// `None` and the caller decides whether that is a refusal or an unbindable route.
pub(super) fn live_cell_identity(tid: usize) -> Option<(u64, u64)> {
    crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&tid))
        .map(|t| (t.cell_id.0, t.cell_generation))
}

pub(super) fn caller_has_network(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.network)
}

#[cfg(feature = "test-hooks")]
pub(super) fn caller_has_development_silo_registration(caller_id: usize) -> bool {
    // A launch-route token on the root task, not a Cell right (not in CapSet).
    crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&caller_id).is_some_and(|task| {
            task.root_tid == caller_id && task.development_silo_registration_cap.is_some()
        })
    })
}

pub(super) fn caller_has_supervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.supervisor)
}

/// ADR-0031 CP1 §6: 418/424 offer a function that is unowned or already the
/// caller's, and skip one a live task owns, so a second instance of one driver
/// reaches the next identical function.
/// H-3: a PCI query (418/424) must fall inside the caller's catalog row,
/// keyed by the caller Cell's measured program name (`catalog::PCI_DRIVERS`).
/// Logs the refused query so a missing row is visible in the boot log.
pub(super) fn caller_pci_query_allowed(caller_id: usize, allowed: impl Fn(&str) -> bool) -> bool {
    let program = caller_launch_state(caller_id)
        .map(|(name, _, _)| name)
        .unwrap_or_default();
    let ok = allowed(&program);
    if !ok {
        log::warn!(
            "[pcie] DENY query from caller={} program={:?}: outside its catalog::PCI_DRIVERS row",
            caller_id,
            program
        );
    }
    ok
}

/// Look up the attested identity of `sender_tid`.
///
/// Returns `None` when the sender is gone (a reply to a dead cell, a queued
/// message whose sender already exited) or is not attributable to a cell. A
/// receiver must read that as "unknown caller" and deny, never as "some caller
/// that owns nothing" — owning nothing still reads unowned state.
///
/// The flag word carries the VFS mutation bit, the four network rights, the Q
/// Config-write right and the two KMS SSH rights (`CALLER_FLAG_KMS_SSH_SIGN`,
/// `CALLER_FLAG_KMS_SSH_ADMIN`, KSSH K3). Every right except the VFS bit is read
/// from the Cell root's `CapSet`, so every thread of a Cell reports the same
/// right; the kernel attests it and the receiving service enforces it.
pub fn attested_identity_of(sender_tid: usize) -> Option<api::caller_identity::CallerIdentity> {
    let sched_guard = crate::task::SCHEDULER.lock();
    let sched = sched_guard.as_ref()?;
    let task = sched.tasks.get(&sender_tid)?;
    let owner = sched.resolve_live_cell_owner(task.cell_id, task.cell_generation)?;
    // Every attested right is a property of the Cell: read from the
    // incarnation record (`Scheduler::cell_rights`), the CapSet spawn bounded by
    // request ∩ ceiling ∩ policy and revokes narrowed. `resolve_live_cell_owner`
    // has already proven the root is live and is this exact Cell generation, so
    // a thread can neither claim more nor less than its Cell holds.
    let root_caps = sched.cell_rights(sender_tid);
    let mut flags = vfs_mutate_caller_flag(task.syscall_allowlist, root_caps.vfs_mutate);
    flags |= root_caps.net_caller_flags();
    // Q (Phase 01 CP1) is a Cell right exactly like the network rights: read
    // from the same root CapSet, so a thread cannot hold more or less of it.
    flags |= root_caps.config_caller_flags();
    // KSSH K3: the KMS SSH sign/admin rights are Cell rights too, read from the
    // same root CapSet for the same reason. Attested, not enforced, here.
    flags |= root_caps.kms_caller_flags();
    (owner.cell_id == task.cell_id.0 && owner.generation == task.cell_generation).then_some(
        api::caller_identity::CallerIdentity {
            flags,
            cell_id: task.cell_id.0,
            generation: task.cell_generation,
            sender_tid: sender_tid as u64,
        },
    )
}

/// Write the caller-identity trailer into the last bytes of a receiver's buffer.
///
/// MUST be called only after the payload has been copied into `buf_ptr`: the
/// trailer's unforgeability rests on the kernel writing it last, so a sender that
/// pads its message across the whole buffer cannot pre-place a forged one.
///
/// A sender the scheduler can no longer attribute leaves the tail zeroed, which
/// parses back as "no identity" and therefore as deny.
pub(super) fn write_caller_identity(
    caller_id: usize,
    buf_ptr: usize,
    buf_len: usize,
    sender_tid: usize,
) {
    let len = api::caller_identity::CALLER_IDENTITY_LEN;
    let Some(offset) = buf_len.checked_sub(len) else {
        return; // buffer cannot hold a trailer; receiver sees no identity → deny
    };
    let dst = buf_ptr.wrapping_add(offset);
    if validate_user_buf(dst, len, MAX_USER_BUF).is_err() {
        return;
    }
    let trailer = attested_identity_of(sender_tid)
        .map(|id| id.to_trailer())
        .unwrap_or([0u8; api::caller_identity::CALLER_IDENTITY_LEN]);
    if let Ok(view) = caller_copy_view(caller_id) {
        view.write_bytes(dst, &trailer).ok();
    } else {
        TaskCopyView::sas().write_bytes(dst, &trailer).ok();
    }
}

/// Drop the exact queued owner-death record `token` after its delivery copy
/// succeeded; a record already gone (or another one now at the head) is left alone.
pub(super) fn remove_owner_death(task: &mut crate::task::Task, token: u64) {
    if let Some(pos) = task
        .pending_owner_deaths
        .iter()
        .position(|(queued, _, _)| *queued == token)
    {
        task.pending_owner_deaths.remove(pos);
    }
}

/// Every task of `tid`'s Cell incarnation (root and threads), in a fixed
/// buffer: no allocation, bounded by the per-Cell thread cap.
pub(super) fn incarnation_members(
    sched: &crate::task::scheduler::Scheduler,
    tid: usize,
) -> incarnation_tids::Tids {
    let mut out = incarnation_tids::Tids::default();
    if let Some(target) = sched.tasks.get(&tid) {
        let incarnation = (target.cell_id, target.cell_generation);
        for (&id, task) in sched.tasks.iter() {
            if (task.cell_id, task.cell_generation) == incarnation {
                out.push(id);
            }
        }
    }
    out
}

pub(super) struct RevokeSerial;

/// The revoke's mask published as pending on the target incarnation's root.
/// Dropping it (every early return, including a failed teardown) withdraws the
/// pending bits again; a committed revoke has already folded them into
/// `Task::rights`, so the clear is then a no-op.
pub(super) struct PendingRevoke {
    pub(super) root_tid: usize,
    pub(super) incarnation: (types::CellId, u64),
    pub(super) mask: u32,
}

/// The audit subject of `tid` (its Cell incarnation). Takes `SCHEDULER`;
/// callers that already hold it use `audit::Subject::of` directly.
pub(super) fn audit_subject(tid: usize) -> crate::audit::Subject {
    crate::task::SCHEDULER.lock().as_ref().map_or(
        crate::audit::Subject {
            tid: tid as u64,
            ..crate::audit::Subject::KERNEL
        },
        |sched| crate::audit::Subject::of(sched, tid),
    )
}

/// Audit a refused syscall against the caller's Cell incarnation.
pub(super) fn audit_syscall_denied(caller_id: usize, detail: u32) {
    crate::audit::log_event(
        crate::audit::AuditEvent::SyscallDenied,
        &crate::audit::encode_subject(audit_subject(caller_id), detail, 0),
    );
}

/// Notify `target_tid` that `mask` was revoked: `[0xAC, 0xF2, mask_le4]`, the
/// byte-1 discriminant `0xF2` reserved in `spec/17` §3.
///
/// Best-effort by design: the teardown already happened, and a Cell that ignores
/// the event still lost the hardware. Returns `false` when the target is gone or
/// its mailbox is full.
pub(crate) fn send_cap_revoked_event(target_tid: usize, mask: u32) -> bool {
    let mut msg = [0u8; 6];
    msg[0] = 0xAC; // App SDK envelope magic
    msg[1] = 0xF2; // CapRevoked
    msg[2..6].copy_from_slice(&mask.to_le_bytes());
    crate::task::ipc_post_nonblock(0, target_tid, &msg).is_ok()
}

/// Annex S2-T/S2-W §3: `GetProcs` and `GetProcs2` additionally need the
/// caller's `observe_tasks` right and `MemInfo` its `observe_memory` right, on
/// top of the allowlist bit (and whatever a permit-all allowlist says). Every
/// other opcode needs none, and neither does the kernel-context sentinel (tid 0),
/// which has no task. A missing or terminal task is denied.
pub(super) fn observe_right_held(sc: ViSyscall, caller_id: usize) -> bool {
    let held: fn(&crate::task::cap::CapSet) -> bool = match sc {
        ViSyscall::GetProcs | ViSyscall::GetProcs2 | ViSyscall::TelemetrySnapshot => {
            |rights| rights.observe_tasks
        }
        ViSyscall::MemInfo => |rights| rights.observe_memory,
        _ => return true,
    };
    if caller_id == 0 {
        return true;
    }
    caller_has_cap(caller_id, held)
}

pub(super) fn read_cell_owner_request(
    caller_id: usize,
    request_ptr: usize,
    request_len: usize,
) -> Result<api::cell_owner::CellOwnerRequest, SyscallError> {
    let len = api::cell_owner::CELL_OWNER_REQUEST_LEN;
    if request_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(request_ptr, len, MAX_USER_BUF)?;
    let mut bytes = [0u8; api::cell_owner::CELL_OWNER_REQUEST_LEN];
    let view = caller_copy_view(caller_id)?;
    view.read_into(request_ptr, &mut bytes)
        .map_err(|_| SyscallError::InvalidInput)?;
    api::cell_owner::CellOwnerRequest::from_bytes(&bytes).ok_or(SyscallError::InvalidInput)
}

/// Withdraw `cap_mask` from the target's Cell incarnation after its Class-2
/// teardown has finished: re-run the gate, then narrow the incarnation record
/// (`root_tid`'s `Task::rights`, captured before the teardown) that every
/// thread of the Cell is gated on. The record is narrowed even if the target
/// thread exited meanwhile; only a retired incarnation (root gone) has nothing
/// left to narrow. Returns whether the target itself is still present. Takes
/// `SCHEDULER` only.
pub(super) fn commit_cap_revoke(
    caller_id: usize,
    target_tid: usize,
    root_tid: usize,
    incarnation: (types::CellId, u64),
    cap_mask: u32,
) -> Result<bool, SyscallError> {
    let mut sched_opt = crate::task::SCHEDULER.lock();
    let sched = match sched_opt.as_mut() {
        Some(s) => s,
        None => return Err(SyscallError::InvalidCommand),
    };
    cap_revoke_gate(sched, caller_id, target_tid, cap_mask)?;
    if let Some(root) = sched.tasks.get_mut(&root_tid) {
        if (root.cell_id, root.cell_generation) == incarnation {
            root.rights = root.rights.without_revoke_mask(cap_mask);
            root.revoke_pending &= !cap_mask;
        }
    }
    Ok(sched.tasks.contains_key(&target_tid))
}

/// Authority gates for `CapRevoke`.
///
/// Runs twice — before the teardown and before the fields are cleared — so a
/// `SpawnCap` withdrawn while the teardown runs cannot authorize the clear.
///
/// Gate 1: the caller must hold `SpawnCap`, the same authority `ForceExit` needs.
/// Gate 1b: refuse bits whose authority is AMBIENT with no teardown path —
///   `HYPERVISOR` (H-extension CSR access is not syscall-gated). MMIO device
///   windows and the privileged path caps are torn down by the revoke itself.
/// Gate 2: a system service cell (block_io/network) is refused — revoking I/O
///   authority from a running VFS/net service mid-flight corrupts driver state;
///   the supervisor freeze/replace flow is the supported path.
pub(super) fn cap_revoke_gate(
    sched: &mut crate::task::scheduler::Scheduler,
    caller_id: usize,
    target_tid: usize,
    cap_mask: u32,
) -> Result<(), SyscallError> {
    use api::syscall::cap_mask as CM;

    if !sched.tasks.contains_key(&caller_id) || !sched.cell_rights(caller_id).spawn {
        return Err(SyscallError::PermissionDenied);
    }

    if cap_mask & CM::HYPERVISOR != 0 {
        crate::audit::log_event(
            crate::audit::AuditEvent::CapRevoked,
            &crate::audit::encode_subject(
                crate::audit::Subject::of(sched, caller_id),
                target_tid as u32,
                cap_mask,
            ),
        );
        log::warn!(
            "[kernel] CapRevoke: refused ambient-authority bits mask={:#010x} from task {} \
             (no teardown for hypervisor authority)",
            cap_mask,
            caller_id
        );
        return Err(SyscallError::NotSupported);
    }

    let target_rights = sched.cell_rights(target_tid);
    let target_is_system = target_rights.block_io || target_rights.network;
    if target_is_system {
        return Err(SyscallError::PermissionDenied);
    }
    Ok(())
}

/// Release task `tid`'s MMIO windows of `classes` and take their user
/// accessibility away.
///
/// Fail-closed: an error means a window the task can still reach, so the caller
/// returns without clearing the capability.
pub(super) fn revoke_mmio_classes(
    tid: usize,
    classes: crate::resource_registry::MmioClass,
) -> Result<(), SyscallError> {
    for (base, len) in crate::resource_registry::revoke_mmio_for(tid, classes) {
        if let Err(e) = crate::memory::paging::revoke_mmio_user(base, len) {
            log::error!(
                "[kernel] CapRevoke: MMIO window {base:#x}+{len:#x} is still user-reachable \
                 ({e:?}); capability left intact"
            );
            return Err(SyscallError::NotSupported);
        }
    }
    Ok(())
}
