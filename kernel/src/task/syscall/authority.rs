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
    sched: &crate::sched::scheduler::Scheduler,
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
    sched: &mut crate::sched::scheduler::Scheduler,
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

/// Handler for `Syscall::CapRevoke` (extracted from the dispatcher).
pub(super) fn cap_revoke(caller_id: usize, target_tid: usize, cap_mask: u32) -> SyscallResult {
    use api::syscall::cap_mask as CM;

    if target_tid == caller_id {
        return Err(SyscallError::InvalidCommand);
    }

    let _serial = RevokeSerial::acquire().ok_or(SyscallError::TryAgain)?;

    // Gates, then the facts the teardown needs. The teardown itself runs
    // with SCHEDULER released: the grant reclaim reaches
    // `live_task_binding` and the notify reaches `ipc_post_nonblock`, and
    // this spinlock is not reentrant. Both passes re-run the gates, so
    // nothing is revoked on authority withdrawn while it ran. The mask is
    // published as pending on the root first, so no task of the Cell —
    // including one spawned during the teardown — can re-claim what is
    // being torn down.
    let (members, pending) = {
        let mut sched_opt = crate::task::SCHEDULER.lock();
        let sched = match sched_opt.as_mut() {
            Some(s) => s,
            None => return Err(SyscallError::InvalidCommand),
        };
        cap_revoke_gate(sched, caller_id, target_tid, cap_mask)?;
        let target = sched
            .tasks
            .get(&target_tid)
            .ok_or(SyscallError::InvalidCommand)?;
        let incarnation = (target.cell_id, target.cell_generation);
        // Rights are per Cell incarnation, so revoking a sibling thread
        // is revoking oneself — refused like `target_tid == caller_id`.
        if sched
            .tasks
            .get(&caller_id)
            .is_some_and(|c| (c.cell_id, c.cell_generation) == incarnation)
        {
            return Err(SyscallError::InvalidCommand);
        }
        let root_tid = target.root_tid;
        // Same incarnation guard as the commit and `PendingRevoke::drop`,
        // so the bits set here are exactly the bits cleared there.
        if let Some(root) = sched.tasks.get_mut(&root_tid) {
            if (root.cell_id, root.cell_generation) == incarnation {
                root.revoke_pending |= cap_mask;
            }
        }
        (
            incarnation_members(sched, target_tid),
            PendingRevoke {
                root_tid,
                incarnation,
                mask: cap_mask,
            },
        )
    };

    // ── Class-2 teardown, BEFORE the record changes ──────────────────
    // Ambient authority — a mapped MMIO window, an already-shared grant —
    // is exercised without a per-use syscall re-check, so clearing the
    // record alone would leave it live. Resources are owned by the tid
    // that claimed them, so every thread of the incarnation is torn down.
    // Fail-closed: a teardown that cannot finish returns with the
    // capability still in the record (the pending mask is withdrawn on
    // return). Members torn down before the failure stay torn down; a
    // retry of the same revoke is idempotent.
    let mmio_revoke = ((cap_mask >> CM::MMIO_SHIFT) & 0xFF) as u8;
    for &tid in members.iter() {
        if mmio_revoke != 0 {
            revoke_mmio_classes(tid, mmio_revoke as crate::resource_registry::MmioClass)?;
            // A revoked device class can be the authority behind a grant
            // the target owns (grants carry no source-cap tag), so its
            // owned grants are reclaimed with it. Frames a live pin still
            // protects are quarantined rather than freed.
            reclaim_owned_grants(tid);
        }

        if cap_mask & CM::PCIE_DRIVER != 0 {
            // The DMA-anywhere surface: the whole domain, its requester
            // entries, and the BAR windows the Cell claimed. Exactly the
            // teardown a dying driver Cell gets.
            if !crate::task::teardown_dma_authority(tid) {
                log::error!(
                    "[kernel] CapRevoke: DMA teardown for task {tid} was not \
                             acknowledged; capability left in the record, retry the revoke"
                );
                return Err(SyscallError::NotSupported);
            }
            revoke_mmio_classes(
                tid,
                crate::resource_registry::DEV_PCIE as crate::resource_registry::MmioClass,
            )?;
            reclaim_owned_grants(tid);
        }

        if cap_mask & CM::PLATFORM != 0 {
            // The Platform Cell's ECAM config-space window.
            revoke_mmio_classes(tid, crate::resource_registry::CLASS_ECAM)?;
        }
    }

    let target_present = commit_cap_revoke(
        caller_id,
        target_tid,
        pending.root_tid,
        pending.incarnation,
        cap_mask,
    )?;
    let root_tid = pending.root_tid;
    drop(pending);

    crate::audit::log_event(
        crate::audit::AuditEvent::CapRevoked,
        &crate::audit::encode_subject(audit_subject(caller_id), target_tid as u32, cap_mask),
    );
    log::info!(
        "[kernel] CapRevoke: task {} revoked mask={:#010x} from task {}",
        caller_id,
        cap_mask,
        target_tid
    );

    // Courtesy notification, last and best-effort: the teardown above is
    // what enforces the revocation, so a target that is gone or whose
    // mailbox is full does not fail the call. The Cell's root (its app
    // loop) is told too, since the revoke applied to the whole Cell.
    if target_present && !send_cap_revoked_event(target_tid, cap_mask) {
        log::warn!("[kernel] CapRevoke: task {target_tid} not notified (exited or mailbox full)");
    }
    if root_tid != target_tid && !send_cap_revoked_event(root_tid, cap_mask) {
        log::warn!(
            "[kernel] CapRevoke: Cell root {root_tid} not notified (exited or mailbox full)"
        );
    }

    Ok(0)
}

/// Handler for `Syscall::RegisterService` (extracted from the dispatcher).
pub(super) fn register_service(caller_id: usize, service_id: u16, tid: usize) -> SyscallResult {
    // `BLOCK_DRIVER` is retired: block routes are per device (416/432),
    // so no caller may publish the singleton service.
    if service_id == api::syscall::service::BLOCK_DRIVER {
        return Err(SyscallError::PermissionDenied);
    }
    // The machine pin endpoint is init's own: only trusted init may
    // register it, and only naming itself (ADR-0029 §3). The VFS admits
    // the live provider to the pin store, so a hijacked registration
    // would hand over pin write access.
    if service_id == api::syscall::service::MACHINE_PIN
        && !(tid == caller_id && caller_is_trusted_init(caller_id))
    {
        return Err(SyscallError::PermissionDenied);
    }
    // Development Silo readiness is self-published only after artifact,
    // VM, entropy, guest READY, and public-key validation. The dedicated
    // non-delegable cap is minted solely for the governed `/bin/silo`
    // root task in test-hooks kernels; HypervisorCap alone is insufficient.
    #[cfg(feature = "test-hooks")]
    if service_id == api::syscall::service::SILO
        && tid == 0
        && caller_has_development_silo_registration(caller_id)
    {
        let Some((cell_id, generation)) = live_cell_identity(caller_id) else {
            return Err(SyscallError::InvalidInput);
        };
        return if crate::cell::service_registry::register(
            service_id, caller_id, cell_id, generation,
        ) {
            Ok(0)
        } else {
            Err(SyscallError::InvalidInput)
        };
    }
    // The state-transfer demo owns one non-production service ID and may
    // self-register (`tid=0`) so QEMU can exercise the real Supervisor path.
    if service_id == api::syscall::service::HOTSWAP_DEMO && tid == 0 && caller_has_spawn(caller_id)
    {
        // The caller is executing, so its identity is present. If it were
        // somehow absent, the entry records 0/0 and the legacy `lookup`
        // path keeps working while `LookupServiceBound` correctly reports
        // that no binding exists for it.
        let (cell_id, generation) = live_cell_identity(caller_id).unwrap_or((0, 0));
        crate::cell::service_registry::register(service_id, caller_id, cell_id, generation);
        return Ok(0);
    }
    // Driver Cell self-registration path: a PCIe GPU driver or the
    // display-only BCM mailbox driver may register itself with tid=0.
    if (caller_has_pcie_driver(caller_id)
        || caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY))
        && tid == 0
        && service_id == api::syscall::service::GPU_DRIVER
    {
        let (cell_id, generation) = live_cell_identity(caller_id).unwrap_or((0, 0));
        // Role and route publish as one transaction against exact-TID
        // teardown. This path has always answered 0 once authorized, so
        // a rejected route only rolls the role back; it adds no refusal.
        crate::dev::driver_cell::publish_role_or_rollback(
            caller_id,
            crate::dev::driver_cell::register_gpu_driver,
            || crate::cell::service_registry::register(service_id, caller_id, cell_id, generation),
            crate::dev::driver_cell::deregister_gpu_driver,
        );
        return Ok(0);
    }
    // Privileged: SpawnCap is necessary but not sufficient. Well-known ids
    // belong to trusted init and SupervisorCap holders; a spawn-only cell
    // (e.g. a USB host driver with a protocol child) may publish only a
    // private id it owns, so it cannot hijack VFS/NET/KMS/SUPERVISOR.
    if !caller_has_spawn(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    let registrar = if caller_is_trusted_init(caller_id) || caller_has_supervisor(caller_id) {
        crate::cell::service_registry::Registrar::Namespace
    } else {
        crate::cell::service_registry::Registrar::Spawner(caller_id)
    };
    // The liveness check and the identity capture share one scheduler lock,
    // so a provider cannot be recorded with a binding that was already stale
    // when it was written.
    let provider = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| {
            scheduler.tasks.get(&tid).and_then(|task| {
                (!matches!(task.state, TaskState::Retiring | TaskState::Terminated))
                    .then_some((task.cell_id.0, task.cell_generation))
            })
        });
    let Some((cell_id, generation)) = provider else {
        return Err(SyscallError::InvalidInput);
    };
    match crate::cell::service_registry::register_as(
        registrar, service_id, tid, cell_id, generation,
    ) {
        Ok(()) => Ok(0),
        Err(crate::cell::service_registry::Refusal::NotOwner) => {
            Err(SyscallError::PermissionDenied)
        }
        Err(crate::cell::service_registry::Refusal::Rejected) => Err(SyscallError::InvalidInput),
    }
}

/// Handler for `Syscall::LookupService` (extracted from the dispatcher).
pub(super) fn lookup_service(service_id: u16) -> SyscallResult {
    // Open to all cells: resolve the live provider tid (0 = none registered),
    // so a client reconnects transparently after the supervisor respawns a
    // service. The dynamic replacement for the boot-order `ServiceLookup` hardcode.
    Ok(crate::cell::service_registry::lookup(service_id).unwrap_or(0))
}

/// Handler for `Syscall::LookupServiceBound` (extracted from the dispatcher).
pub(super) fn lookup_service_bound(
    caller_id: usize,
    service_id: u16,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    // Open to all cells, same allowlist bit as `LookupService`: this answers
    // the same question with the provider's identity instead of a bare tid.
    //
    // The record is written whole or not at all. A short buffer is refused
    // with the same `BufferTooSmall` error the other fixed-record writers
    // use (`QueryDirHandles`, `ResolveCellOwner`) rather than partially
    // filled; an absent, paused or identity-less provider returns the `0`
    // absence sentinel `LookupService` uses — never a binding the kernel
    // cannot stand behind.
    if out_len < api::service_binding::SERVICE_BINDING_LEN {
        return Err(SyscallError::BufferTooSmall);
    }
    let Some((tid, cell_id, generation)) = crate::cell::service_registry::lookup_bound(service_id)
    else {
        return Ok(0);
    };
    let binding = api::service_binding::ServiceBinding::new(tid as u64, cell_id, generation);
    write_user_slice(caller_id, out_ptr, &binding.to_bytes(), MAX_USER_BUF)?;
    Ok(api::service_binding::SERVICE_BINDING_LEN)
}

/// Handler for `Syscall::ServiceLookup` (extracted from the dispatcher).
pub(super) fn service_lookup(caller_id: usize, name_ptr: usize, name_len: usize) -> SyscallResult {
    let name = read_user_string(caller_id, name_ptr, name_len, MAX_LOG_MSG)?;
    let id: usize = match name.as_str() {
        "vfs" => 3,
        "config" => 4,
        "input" => 5,
        "net" => 6,
        "compositor" => 7,
        "shell" => 8,
        _ => return Err(SyscallError::FileNotFound),
    };
    Ok(id)
}

/// Handler for `Syscall::ResolveCellOwner` (extracted from the dispatcher).
pub(super) fn resolve_cell_owner(
    caller_id: usize,
    cell_id: u64,
    generation: u64,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    let len = api::cell_owner::CELL_OWNER_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let owner = {
        let guard = crate::task::SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
        let owner = sched
            .resolve_live_cell_owner(types::CellId(cell_id), generation)
            .ok_or(SyscallError::PermissionDenied)?;
        let vfs_principal = sched.tasks.get(&caller_id).is_some_and(|task| {
            crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize)
                && task.caller.cell_id == cell_id
                && task.caller.cell_generation == generation
        });
        let registered_service =
            crate::cell::service_registry::is_registered_tid(owner.root_tid as usize);
        if !vfs_principal && !registered_service {
            return Err(SyscallError::PermissionDenied);
        }
        owner
    };
    let bytes = owner.to_bytes();
    write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::WatchCellOwner` (extracted from the dispatcher).
pub(super) fn watch_cell_owner(
    caller_id: usize,
    cell_id: u64,
    generation: u64,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    let len = api::cell_owner::CELL_OWNER_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let (owner, token) = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| {
            sched.watch_live_cell_owner(caller_id, types::CellId(cell_id), generation)
        })
        .ok_or(SyscallError::PermissionDenied)?;
    let bytes = owner.to_bytes();
    write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
    usize::try_from(token).map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::ResolveCellOwnerRecord` (extracted from the dispatcher).
pub(super) fn resolve_cell_owner_record(
    caller_id: usize,
    request_ptr: usize,
    request_len: usize,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
    let len = api::cell_owner::CELL_OWNER_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let owner = {
        let guard = crate::task::SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
        let allowed = sched.tasks.get(&caller_id).is_some_and(|task| {
            crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize)
                && task.caller.cell_id == request.cell_id
                && task.caller.cell_generation == request.generation
        });
        if !allowed {
            return Err(SyscallError::PermissionDenied);
        }
        sched
            .resolve_live_cell_owner(types::CellId(request.cell_id), request.generation)
            .ok_or(SyscallError::PermissionDenied)?
    };
    let bytes = owner.to_bytes();
    write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::ResolveCellImage` (extracted from the dispatcher).
pub(super) fn resolve_cell_image(
    caller_id: usize,
    request_ptr: usize,
    request_len: usize,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
    let len = api::cell_image::CELL_IMAGE_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let image = {
        let guard = crate::task::SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
        crate::task::cell_image::resolve_for_caller(
            sched,
            caller_id,
            request,
            crate::fast_ipc::is_registered_vfs_cell,
        )
        .ok_or(SyscallError::PermissionDenied)?
    };
    write_user_slice(caller_id, out_ptr, &image.to_bytes(), MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::WatchCellOwnerRecord` (extracted from the dispatcher).
pub(super) fn watch_cell_owner_record(
    caller_id: usize,
    request_ptr: usize,
    request_len: usize,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
    let len = api::cell_owner::CELL_OWNER_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let (owner, token) = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| {
            sched.watch_live_cell_owner(
                caller_id,
                types::CellId(request.cell_id),
                request.generation,
            )
        })
        .ok_or(SyscallError::PermissionDenied)?;
    let bytes = owner.to_bytes();
    write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
    usize::try_from(token).map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::CancelCellOwnerWatch` (extracted from the dispatcher).
pub(super) fn cancel_cell_owner_watch(caller_id: usize, token: u64) -> SyscallResult {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        sched.cancel_cell_owner_watch(caller_id, token);
    }
    Ok(0)
}

/// Handler for `Syscall::FreezeCell` (extracted from the dispatcher).
pub(super) fn freeze_cell(caller_id: usize, target_tid: usize) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // Refuse to freeze the kernel (tid 0) or the caller itself.
    if target_tid == 0 || target_tid == caller_id {
        return Err(SyscallError::InvalidInput);
    }
    // swap_id u64::MAX = admin freeze (not a hotswap sequence).
    crate::cell::hotswap::freeze_task_with_ceiling(target_tid, u64::MAX)
        .map(|_| 0)
        .map_err(|error| {
            if error == types::ViError::NotFound {
                SyscallError::FileNotFound
            } else {
                SyscallError::PermissionDenied
            }
        })
}

/// Handler for `Syscall::PauseService` (extracted from the dispatcher).
pub(super) fn pause_service(
    caller_id: usize,
    service_id: u16,
    expected_tid: usize,
) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if expected_tid == 0 {
        return Err(SyscallError::InvalidInput);
    }
    if !crate::cell::service_registry::pause(service_id, expected_tid) {
        return Err(SyscallError::TryAgain);
    }
    if !crate::task::inbound_ipc_drained(expected_tid) {
        return Err(SyscallError::TryAgain);
    }
    Ok(0)
}

/// Handler for `Syscall::ResumeCell` (extracted from the dispatcher).
pub(super) fn resume_cell(
    caller_id: usize,
    target_tid: usize,
    source_tid: usize,
    service_id: usize,
    reserved: usize,
) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    if target_tid == 0 || reserved != 0 {
        return Err(SyscallError::InvalidInput);
    }
    if source_tid == 0 {
        if service_id != 0 {
            return Err(SyscallError::InvalidInput);
        }
        crate::cell::hotswap::unfreeze_task(target_tid);
        return Ok(0);
    }
    if source_tid == target_tid || service_id > u16::MAX as usize {
        return Err(SyscallError::InvalidInput);
    }
    crate::cell::hotswap::commit_hotswap_barrier(source_tid, target_tid, service_id as u16)
        .map(|()| 0)
        .map_err(|error| match error {
            ViError::NotFound => SyscallError::FileNotFound,
            ViError::PermissionDenied => SyscallError::PermissionDenied,
            ViError::WouldBlock | ViError::OutOfMemory => SyscallError::TryAgain,
            ViError::InvalidArgument | ViError::InvalidInput => SyscallError::InvalidInput,
            _ => SyscallError::Unknown,
        })
}

/// Handler for `Syscall::KillCell` (extracted from the dispatcher).
pub(super) fn kill_cell(caller_id: usize, target_tid: usize, exit_code: u32) -> SyscallResult {
    if !caller_has_supervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // Never kill kernel (tid 0) or the caller itself.
    if target_tid == 0 || target_tid == caller_id {
        return Err(SyscallError::InvalidInput);
    }
    // Never kill critical cells (init) — would collapse the restart tree.
    // Single lock acquisition to get both is_critical and cell_id.
    let (is_crit, cell_id) = {
        let guard = crate::task::SCHEDULER.lock();
        match guard.as_ref().and_then(|s| s.tasks.get(&target_tid)) {
            None => return Err(SyscallError::FileNotFound),
            Some(t) => (t.is_critical, t.cell_id),
        }
    };
    if is_crit {
        return Err(SyscallError::PermissionDenied);
    }
    crate::cell::hotswap::exit_task_internal(target_tid, cell_id);
    Ok(exit_code as usize)
}
