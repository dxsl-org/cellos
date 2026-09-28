//! Capability witness for domain-backed zero-copy grants (phase 03).
//!
//! The phase-01 containment gate refused every grant entry point that named a
//! private-root task. Phase 03 replaces that blanket refusal with a capability
//! check: exactly one shape is allowed — a live task whose address space is a
//! still-`Live` private root, on the one architecture whose lifecycle is
//! implemented — and every other shape keeps the byte-for-byte sentinel.
//!
//! This fixture drives the *production* `handle_syscall` path with real
//! `TaskAddressSpace::Domain` tasks, so both sides of the check are asserted
//! where the ABI sees them, not inside a private helper:
//!
//!   * a live private root allocates and registers its backing, and that backing
//!     is **not** USER-accessible in the SAS/global root (the phase-01 leak);
//!   * an unsupported rights request on a domain pair is refused;
//!   * a SAS owner still cannot publish a private-root receiver, and a private
//!     root still cannot resolve an SAS-owned record;
//!   * a retired private root is not a capability: it gets the alloc-safe `0` and
//!     the `usize::MAX` slice sentinel;
//!   * Tier-1 SAS→SAS grants keep their exact behaviour;
//!   * a revoked domain record refuses a new receiver slice, a new owner slice and
//!     a new share whether its revoke completed or deferred, and its receiver
//!     mapping is gone either way;
//!   * the denies themselves consume and leak no frame.
//!
//! Emits one marker per property, plus a single terminal:
//!   `S22-RV64-GRANT-GATE: PASS` — every property held.

use super::syscall::{handle_syscall, Syscall, SyscallError};
use super::tcb::TaskAddressSpace;
use super::thread_cap_selftest::{insert, mk_task, remove};
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::paging::Flags;
use alloc::sync::Arc;
use types::GrantPerm;

// Synthetic tids above the range the boot sequence assigns, removed before
// return. Cells stay clear of the ids the sibling boot self-tests use.
const DOMAIN_TID: usize = 9501;
const DOMAIN_RECEIVER_TID: usize = 9504;
const RETIRED_TID: usize = 9505;
const SAS_OWNER_TID: usize = 9502;
const SAS_GRANTEE_TID: usize = 9503;
const DOMAIN_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 14) as u64;
const DOMAIN_RECEIVER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 17) as u64;
const RETIRED_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 18) as u64;
const SAS_OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 15) as u64;
const SAS_GRANTEE_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 16) as u64;

const PAGE_SIZE: usize = 4096;
const GRANT_SIZE: usize = PAGE_SIZE;
/// Private-root virtual addresses for the fixture's own pages. Kept out of the
/// SAS identity window so nothing else in the boot can alias them.
const DOMAIN_VA: usize = 0x0400_0000;
const DOMAIN_RECEIVER_VA: usize = 0x0800_0000;
const RETIRED_VA: usize = 0x0c00_0000;

/// Frames currently available to the allocator.
fn free_frames() -> usize {
    crate::memory::frame::FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|allocator| allocator.free_frames())
        .unwrap_or(0)
}

/// A live private root for one fixture task. The page is never faulted in by
/// hardware; it exists so the task is a real domain identity, not a task with a
/// hand-set flag.
fn build_domain_space(va: usize) -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    let bits = Flags::READ | Flags::WRITE;
    builder
        .map_user_page(va, MappingKind::Private, Flags::from_bits(bits))
        .ok()?;
    builder.build().ok()
}

/// Properties the gate must hold, in the order they are reported.
struct GateProbe {
    alloc: bool,
    register: bool,
    unsupported_rights: bool,
    share: bool,
    slice: bool,
    retired: bool,
    sas: bool,
    frames: bool,
    /// The post-revoke refusal invariant, and whether the revoke deferred.
    retire_refusal: bool,
    retire_deferred: bool,
}

impl GateProbe {
    fn all(&self) -> bool {
        self.alloc
            && self.register
            && self.unsupported_rights
            && self.share
            && self.slice
            && self.retired
            && self.sas
            && self.frames
            && self.retire_refusal
    }
}

fn grant_alloc(tid: usize) -> Option<usize> {
    match handle_syscall(tid, Syscall::GrantAlloc { size: GRANT_SIZE }) {
        Ok(base) if base != 0 => Some(base),
        _ => None,
    }
}

fn grant_register(tid: usize) -> Option<usize> {
    match handle_syscall(tid, Syscall::GrantRegister { size: GRANT_SIZE }) {
        Ok(base) if base != 0 => Some(base),
        _ => None,
    }
}

fn grant_share(owner: usize, grant_id: usize, target: usize, perm: GrantPerm) -> bool {
    handle_syscall(
        owner,
        Syscall::GrantShare {
            grant_id,
            target_cell: target,
            perm: perm as usize,
        },
    )
    .is_ok()
}

fn grant_slice(tid: usize, grant_id: usize) -> Result<usize, SyscallError> {
    handle_syscall(tid, Syscall::GrantSlice { grant_id, size_out_ptr: 0 })
}

/// Does `space` hold a `Grant` mapping at `va` whose rights are exactly `bits`?
fn ledger_has(space: &Arc<AddressSpace>, va: usize, bits: usize) -> bool {
    space.ledger().into_iter().any(|entry| {
        entry.virtual_address == va && entry.kind == MappingKind::Grant && entry.flags.bits() & bits == bits
    })
}

/// Revoke `grant_id` and assert the post-revoke refusal invariant.
///
/// `GrantFree` may complete or defer: an unacknowledged remote invalidation — the
/// fail-closed outcome when a peer hart stops acknowledging mid-boot — leaves the
/// record `Revoking` with its frames retained. `RetireProbe` covers both, because
/// *both* outcomes must refuse a new receiver slice, a new owner slice and a new
/// share of the record, and must have removed the receiver mapping.
///
/// Positive control: property 3 published a live `ReadOnly` mapping of this same
/// grant into `receiver_space` and resolved it, so every refusal below is a state
/// change rather than the absence of one. A revoke that silently did nothing
/// leaves the record `Live` and the re-share succeeds — the probe is then false.
struct RetireProbe {
    invariant: bool,
    /// The revoke could not be acknowledged and was left for the retry sweep.
    deferred: bool,
}

fn retire_probe(grant_id: usize, receiver_space: &Arc<AddressSpace>) -> RetireProbe {
    let deferred = handle_syscall(DOMAIN_TID, Syscall::GrantFree { grant_id }).is_err();
    let receiver_slice_refused =
        matches!(grant_slice(DOMAIN_RECEIVER_TID, grant_id), Ok(usize::MAX));
    let owner_slice_refused = matches!(grant_slice(DOMAIN_TID, grant_id), Ok(usize::MAX));
    let share_refused = !grant_share(
        DOMAIN_TID,
        grant_id,
        DOMAIN_RECEIVER_TID,
        GrantPerm::ReadWrite,
    );
    let receiver_mapping_gone = receiver_space
        .ledger()
        .into_iter()
        .all(|entry| entry.virtual_address != grant_id);
    RetireProbe {
        invariant: receiver_slice_refused
            && owner_slice_refused
            && share_refused
            && receiver_mapping_gone,
        deferred,
    }
}

/// Drive every grant entry point and return what the ABI observed.
///
/// The probe cleans up after itself in both worlds: a supported private root's
/// alloc and register are freed here, and a refused one published nothing, so a
/// red run does not leak frames into the following fixtures.
fn probe(
    domain_space: &Arc<AddressSpace>,
    receiver_space: &Arc<AddressSpace>,
    retired_space: &Arc<AddressSpace>,
) -> GateProbe {
    let baseline = free_frames();

    // 1. A live private root owns its backing: the alloc succeeds, the owner's
    //    own root gains an RW+NX grant page, and the SAS/global root never gains
    //    USER access to it (the phase-01 information leak).
    let domain_grant = grant_alloc(DOMAIN_TID);
    let alloc = domain_grant.is_some_and(|base| {
        let (present, user) = crate::memory::paging::mapping_state(base);
        present
            && !user
            && ledger_has(
                domain_space,
                base,
                Flags::READ | Flags::WRITE,
            )
            && !ledger_has(domain_space, base, Flags::EXECUTE)
    });

    // 2. The persistent registered buffer follows the same rule.
    let domain_reg = grant_register(DOMAIN_TID);
    let register = domain_reg.is_some_and(|base| {
        let (present, user) = crate::memory::paging::mapping_state(base);
        present && !user && ledger_has(domain_space, base, Flags::READ | Flags::WRITE)
    });

    // 3. A live domain pair shares ReadOnly accurately (R+NX in the receiver
    //    root), while genuine write-only — which has no ordinary-page
    //    representation — is refused rather than silently widened to RW.
    let unsupported_rights = domain_grant.is_some_and(|grant_id| {
        let ro_shared = grant_share(
            DOMAIN_TID,
            grant_id,
            DOMAIN_RECEIVER_TID,
            GrantPerm::ReadOnly,
        );
        let ro_sliced =
            matches!(grant_slice(DOMAIN_RECEIVER_TID, grant_id), Ok(base) if base == grant_id);
        let ro_accurate = ledger_has(receiver_space, grant_id, Flags::READ)
            && !ledger_has(receiver_space, grant_id, Flags::WRITE)
            && !ledger_has(receiver_space, grant_id, Flags::EXECUTE);
        let wo_refused = !grant_share(
            DOMAIN_TID,
            grant_id,
            DOMAIN_RECEIVER_TID,
            GrantPerm::WriteOnly,
        );
        ro_shared && ro_sliced && ro_accurate && wo_refused
    });

    // 4. Neither direction of a mixed pair may be published: a SAS owner cannot
    //    name a private-root receiver, and a private-root owner cannot name a SAS
    //    receiver. (An SAS→SAS share to an arbitrary tid stays the legacy path's
    //    business; resolution happens at slice time there.)
    let sas_grant = grant_alloc(SAS_OWNER_TID);
    let share = sas_grant.is_some_and(|grant_id| {
        !grant_share(SAS_OWNER_TID, grant_id, DOMAIN_TID, GrantPerm::ReadWrite)
    }) && domain_grant.is_some_and(|grant_id| {
        !grant_share(DOMAIN_TID, grant_id, SAS_GRANTEE_TID, GrantPerm::ReadOnly)
    });

    // 5. …and without that share the private root must not be able to resolve a
    //    raw mapping for it, at any rights.
    let slice = sas_grant.is_some_and(|grant_id| {
        matches!(grant_slice(DOMAIN_TID, grant_id), Ok(usize::MAX))
    });

    // 6. A retired private root is not a capability: the alloc-safe sentinel for
    //    allocation and the slice sentinel for any id.
    retired_space.retire();
    let retired = grant_alloc(RETIRED_TID).is_none()
        && matches!(grant_slice(RETIRED_TID, 0), Ok(usize::MAX));

    // 7. Tier-1 regression: a SAS→SAS grant still shares, slices to the
    //    identity-mapped base and frees (the copied-IPC/VFS path).
    let sas = match grant_alloc(SAS_OWNER_TID) {
        Some(grant_id) => {
            let shared = grant_share(SAS_OWNER_TID, grant_id, SAS_GRANTEE_TID, GrantPerm::ReadOnly);
            let sliced = matches!(grant_slice(SAS_GRANTEE_TID, grant_id), Ok(base) if base == grant_id);
            let freed = handle_syscall(SAS_OWNER_TID, Syscall::GrantFree { grant_id }).is_ok();
            shared && sliced && freed
        }
        None => false,
    };

    // Teardown: release whatever the probe published, then prove the denies
    // themselves did not consume or leak a frame. The domain owner's own
    // `GrantFree` is also the post-revoke refusal invariant (see `retire_probe`),
    // so it is attempted exactly once here.
    if let Some(grant_id) = sas_grant {
        let _ = handle_syscall(SAS_OWNER_TID, Syscall::GrantFree { grant_id });
    }
    let retire = match domain_grant {
        Some(grant_id) => retire_probe(grant_id, receiver_space),
        None => RetireProbe {
            invariant: false,
            deferred: false,
        },
    };
    let reg_deferred = match domain_reg {
        Some(reg_id) => handle_syscall(DOMAIN_TID, Syscall::GrantUnregister { reg_id }).is_err(),
        None => false,
    };
    // A revoke that could not be acknowledged retains its frames *by design*: an
    // unacknowledged remote invalidation must never put a page back in the free
    // pool. The leak check is therefore taken against the two rows the probe
    // itself may still hold, before the retirement sweep retries them — a flat
    // `== baseline` would call the deferred path a leak.
    let pages = GRANT_SIZE.div_ceil(PAGE_SIZE);
    let retained_by_design =
        pages * (usize::from(retire.deferred) + usize::from(reg_deferred));
    let before_sweep = free_frames();
    let accounted = before_sweep + retained_by_design == baseline;
    super::syscall::reclaim_owned_grants(DOMAIN_TID);
    let after_sweep = free_frames();
    // The sweep may complete a deferred revoke (returning frames) but can never
    // retain more than the attempt already did, nor push the pool above baseline.
    let frames = accounted && after_sweep >= before_sweep && after_sweep <= baseline;

    GateProbe {
        alloc,
        register,
        unsupported_rights,
        share,
        slice,
        retired,
        sas,
        frames,
        retire_refusal: retire.invariant,
        retire_deferred: retire.deferred,
    }
}

fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}

/// Run the capability self-test and leave the scheduler and grant tables as
/// they were found.
pub(crate) fn run_primary() {
    let (Some(domain_space), Some(receiver_space), Some(retired_space)) = (
        build_domain_space(DOMAIN_VA),
        build_domain_space(DOMAIN_RECEIVER_VA),
        build_domain_space(RETIRED_VA),
    ) else {
        log::error!("S22-RV64-GRANT-GATE: FAIL fixture-setup");
        return;
    };

    let mut domain_task = mk_task(DOMAIN_TID, DOMAIN_CELL);
    domain_task.address_space = TaskAddressSpace::Domain(Arc::clone(&domain_space));
    insert(domain_task);
    let mut receiver_task = mk_task(DOMAIN_RECEIVER_TID, DOMAIN_RECEIVER_CELL);
    receiver_task.address_space = TaskAddressSpace::Domain(Arc::clone(&receiver_space));
    insert(receiver_task);
    let mut retired_task = mk_task(RETIRED_TID, RETIRED_CELL);
    retired_task.address_space = TaskAddressSpace::Domain(Arc::clone(&retired_space));
    insert(retired_task);
    insert(mk_task(SAS_OWNER_TID, SAS_OWNER_CELL));
    insert(mk_task(SAS_GRANTEE_TID, SAS_GRANTEE_CELL));

    let probe = probe(&domain_space, &receiver_space, &retired_space);

    remove(DOMAIN_TID);
    remove(DOMAIN_RECEIVER_TID);
    remove(RETIRED_TID);
    remove(SAS_OWNER_TID);
    remove(SAS_GRANTEE_TID);

    report("S22-RV64-GRANT-GATE-ALLOC", probe.alloc);
    report("S22-RV64-GRANT-GATE-REGISTER", probe.register);
    report("S22-RV64-GRANT-GATE-WO", probe.unsupported_rights);
    report("S22-RV64-GRANT-GATE-SHARE", probe.share);
    report("S22-RV64-GRANT-GATE-SLICE", probe.slice);
    report("S22-RV64-GRANT-GATE-RETIRED", probe.retired);
    report("S22-RV64-GRANT-GATE-SAS", probe.sas);
    report("S22-RV64-GRANT-GATE-RETIRE-REFUSAL", probe.retire_refusal);
    report("S22-RV64-GRANT-GATE-FRAMES", probe.frames);
    // Which outcome the revoke took is reported rather than asserted: the
    // invariant above must hold for both, and a two-hart boot whose peer stops
    // acknowledging legitimately takes the deferred one.
    log::info!(
        "S22-RV64-GRANT-GATE-RETIRE-OUTCOME: {}",
        if probe.retire_deferred { "DEFERRED" } else { "COMPLETED" }
    );

    if probe.all() {
        log::info!("S22-RV64-GRANT-GATE: PASS");
    } else {
        log::error!("S22-RV64-GRANT-GATE: FAIL");
    }
}
