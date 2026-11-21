// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: grant (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, ipc::*, misc::*, spawn::*, vm::*};

pub(super) const MAX_IN_FLIGHT_DMA_PUBLICATIONS: usize = 64;

pub(crate) struct DmaPublicationGuard {
    pub(super) slot: usize,
    pub(super) tid: usize,
    /// The DMA quota this publication took; `None` for a retirement barrier.
    pub(super) quota: Option<crate::memory::cell_quota::Reservation>,
}

pub(crate) fn dma_publication_in_flight(tid: usize) -> bool {
    DMA_PUBLICATIONS.lock().contains(&tid)
}

#[cfg(test)]
pub(crate) fn test_reserve_dma_publication(tid: usize) -> Option<DmaPublicationGuard> {
    reserve_dma_publication(tid, None)
}

/// The region an `ShmAlloc` handle names: owned by the incarnation that
/// allocated it, never by a task.
fn shm_owner(owner: crate::cell::state_stash::Incarnation) -> RegionOwner {
    RegionOwner::incarnation(owner.cell, owner.generation)
}

#[cfg_attr(not(test), allow(dead_code))]
/// Whether `handle` is an outstanding `ShmAlloc` frame owned by exactly `owner`
/// (same Cell and generation).
pub(super) fn shm_owned_by(handle: usize, owner: crate::cell::state_stash::Incarnation) -> bool {
    grant_table_lock()
        .lock()
        .as_ref()
        .and_then(|table| table.shm(&handle))
        .map(|r| r.owner)
        == Some(shm_owner(owner))
}

/// Run `publish` for SHM handle `handle` while its row is held, if incarnation
/// `(cell, generation)` owns it or the cell is its receiver; `None` otherwise.
/// A caller in the owner's cell first records `receiver` (a live target cell)
/// as the receiver.
///
/// The publication runs under `PAGE_GRANT_TABLE`, the lock the owner's
/// retirement takes the row under (`release_shm_for`): a take that wins leaves
/// no row, so nothing is published; a take that comes after sees the
/// publication (the identity VA maps the frame) and unpublishes it before the
/// frame is freed. Checked and published under separate holds, a receiver
/// could map a frame USER RW after the retirement took its row, and keep it
/// after the free. The caller completes the publication's flush batch after
/// this returns, with the lock released.
/// Lock order: PAGE_GRANT_TABLE → whatever `publish` takes (FRAME_ALLOCATOR →
/// KERNEL_ROOT for `ShmMap`).
pub(super) fn with_accessible_shm<R>(
    handle: usize,
    cell: u64,
    generation: u64,
    receiver: Option<u64>,
    publish: impl FnOnce() -> R,
) -> Option<R> {
    let mut guard = grant_table_lock().lock();
    let region = guard.as_mut()?.shm_mut(&handle)?;
    if region.owner != RegionOwner::incarnation(cell, generation)
        && region.receiver != Receiver::Cell(cell)
    {
        return None;
    }
    if let Some(receiver) = receiver.filter(|_| region.owner.cell.0 == cell) {
        region.receiver = Receiver::Cell(receiver);
    }
    Some(publish())
}

/// Remove every `ShmAlloc` region of `owner` and return them.
pub(super) fn shm_take_for(owner: crate::cell::state_stash::Incarnation) -> Vec<Region> {
    grant_table_lock()
        .lock()
        .as_mut()
        .map(|table| table.take_shm_of(shm_owner(owner)))
        .unwrap_or_default()
}

/// Reclaim the `ShmAlloc` frames of a retired root's incarnation: its SHM
/// regions leave the grant table and their runs take the grant release path.
///
/// Called from the root-retirement funnel after the generation is quiescent.
/// `ShmMap` publishes under `PAGE_GRANT_TABLE` ([`with_accessible_shm`]), so
/// once the row is taken here no new USER mapping of the frame can appear, and
/// the identity-VA check below sees every earlier one.
/// `ShmMap` re-maps a frame's identity VA as USER. If that VA still maps the
/// frame, the USER mapping is removed; where the kernel reaches RAM through the
/// identity VA (`phys_to_virt` is the identity), the boot kernel mapping is
/// restored as in [`free_grant_pages`], since every Usable frame must stay
/// identity-mapped there. That restore runs only after
/// `unpublish_user_frames` has unmapped the page, flushed every hart and
/// drained in-flight Sas copies, so a copy that probed the USER page cannot
/// land in the frame through the fault-free kernel mapping after it is freed.
/// A frame an in-flight operation still holds (a pin, a lease, an export) is
/// withheld first, still mapped, exactly as a reclaimed grant is. On x86_64,
/// where the frame has no identity alias, the kernel-root tables `ShmMap`
/// created for its USER mapping are pruned in the same batch.
/// Returns the number of frames taken from the dead incarnation: each was
/// mapped USER in the shared root with no borrower record, which the
/// retirement verdict reports.
/// Lock order: PAGE_GRANT_TABLE (released); pin REGISTRY (released); KERNEL_ROOT
/// alone (unpublish, prune); then FRAME_ALLOCATOR → KERNEL_ROOT.
pub(crate) fn release_shm_for(cell_id: u64, generation: u64) -> usize {
    use crate::memory::frame_run::Batch;
    let regions = shm_take_for(crate::cell::state_stash::Incarnation {
        cell: cell_id,
        generation,
    });
    if regions.is_empty() {
        return 0;
    }
    let released = regions.len();
    // One batch for every frame: one shared invalidation, then each frame is
    // normalised and freed, or quarantined if a hart does not confirm.
    let mut batch = Batch::new();
    for region in regions {
        let Some(run) = region.into_run() else {
            continue;
        };
        let frame = run.base();
        // An identity VA that no longer maps this frame leaves only stale TLB
        // entries for it: nothing to unpublish.
        let mapped_here = crate::memory::paging::virt_to_phys(frame) == Some(frame);
        let Some(run) = withhold_if_pinned(run) else {
            continue;
        };
        batch.invalidate(frame);
        retire_published_run_into(&mut batch, run, mapped_here);
    }
    let _ = batch.complete();
    released
}

/// The `GrantAlloc` table: one [`Region`] per grant, keyed by its base address.
///
/// The owner is a task and the Cell/generation it ran as: the issuing task
/// identity remains necessary for the existing grant ABI, but the binding
/// prevents a stale task record from becoming a writable-output authority after
/// retirement or transfer. A domain owner's row carries its lifecycle record
/// (`Region::domain`: the owner/receiver roots, the exact receiver mapping
/// tuple and the `Live → Revoking → Revoked` state); a SAS owner keeps the
/// zero-copy path and leaves it `None`.
pub(super) fn grant_table_lock() -> &'static Spinlock<Option<RegionTable>> {
    &PAGE_GRANT_TABLE
}

/// Maximum pages in a single GrantAlloc or GrantRegister call (16 MiB ceiling).
/// Acts as a safety cap; cells are further bounded by available physical frames.
pub(super) const MAX_GRANT_PAGES: usize = 4096;

pub(super) fn grant_pages_for_size(size: usize) -> usize {
    size.div_ceil(4096)
}

pub(super) fn grant_allocated_bytes(size: usize) -> usize {
    grant_pages_for_size(size) * 4096
}

/// Phase-01 containment: is `tid` a live task whose address space is a private
/// domain root?
///
/// A domain-backed zero-copy grant cannot yet complete an owner/grantee mapping
/// or a synchronous revoke: `authorize_grant_slice_locked` publishes the receiver
/// PTE without copying `shared_to`'s rights into it, and no path unmaps that PTE
/// when the record is freed. Every grant entry point that could publish such a
/// record therefore refuses, at the common syscall gate, before any table row,
/// PTE, pin or frame exists. A retired or unknown tid is not a domain here, and
/// the SAS/Tier-1 paths keep their exact current behaviour.
#[cfg(all(feature = "native-domains", cellos_wide))]
pub(super) fn domain_grant_task(tid: usize) -> bool {
    task_domain_space(tid).is_some()
}

/// Without a private-root backend no task can be a domain.
#[cfg(not(all(feature = "native-domains", cellos_wide)))]
pub(super) fn domain_grant_task(_tid: usize) -> bool {
    false
}

/// Is the domain grant lifecycle implemented for this target tuple?
///
/// Phase 03 ratified the private-root lifecycle on RV64. Phase 02's AArch64
/// reopen (`loader::domain_admission::switch_ordering_qualified`) makes the same
/// lifecycle executable on AArch64 **test images**: that entry writes the
/// incoming root inside `Context::switch` (`switch_with_root` programs
/// `TTBR0_EL1` after the outgoing context is stored), and AArch64 invalidation
/// already uses non-global private leaves plus deferred release, so the state
/// machine needs no architecture-specific step beyond this cfg.
///
/// Phase 15 opens it on x86_64, production included, because x86 domain
/// admission is production-qualified (`switch_ordering_qualified`; witnessed by
/// `S22-X86-DOMAIN-LIVE`). x86 private leaves carry no `G` bit and a private
/// root's tag is retired by `INVPCID` (or a CR3 reload without PCID) plus the
/// IPI epoch, through the same probe-then-defer path. The one predicate is the
/// `cellos_domain_grants` cfg (`posture::domain_grants`); AArch64 production
/// builds are pinned below.
pub(super) const fn domain_grant_lifecycle_supported() -> bool {
    cfg!(cellos_domain_grants)
}

/// The phase-01 denial sentinels, named so the production pin below asserts the
/// exact values the entry points return rather than restating literals.
///
/// * `GrantAlloc`/`GrantRegister` deny with `Ok(0)` — a cell-side wrapper reads
///   any nonzero return as a grant id, so `0` is the alloc-safe refusal.
/// * `GrantSlice` denies with `usize::MAX` — the established "not authorized"
///   resolution sentinel.
/// * `GrantFree`/`GrantUnregister` deny with `Err(PermissionDenied)`.
pub(super) const GRANT_DENY_ALLOC: usize = 0;

pub(super) const GRANT_DENY_SLICE: usize = usize::MAX;

pub(super) const GRANT_DENY_ERROR: SyscallError = SyscallError::PermissionDenied;

/// The phase-01 gate as a capability check: may `tid` take part in the
/// private-root grant lifecycle?
///
/// Exactly one shape passes — a live task whose address space is a private root
/// that is still `Live`, on a target whose lifecycle is implemented. Every other
/// shape keeps the denial sentinel: a target without the lifecycle, an unknown
/// or retired tid, and a root already marked dying.
pub(super) fn domain_grant_capable(tid: usize) -> bool {
    domain_grant_lifecycle_supported() && live_domain_space(tid).is_some()
}

/// Does any live grant record name a domain endpoint the lifecycle cannot drain?
///
/// Such a record can pre-date the gate — an image update on a system that had
/// already granted one — and cannot be revoked, because the receiver PTE lives
/// in a private root reachable only through the caller's own ledger. Domain
/// admission therefore refuses while one exists. A record carried by a managed
/// domain record is revocable and does **not** block admission: the pair lane
/// must be able to admit its second cell while the first cell's grant is live.
/// Lock order is the documented `*_GRANT_TABLE → SCHEDULER`, and the two tables
/// are not held at the same time.
#[cfg(all(feature = "native-domains", cellos_wide))]
pub(crate) fn domain_grant_records_live() -> bool {
    let page_hit = {
        let table = grant_table_lock().lock();
        table.as_ref().is_some_and(|grants| {
            grants.values().any(|grant| {
                residual_domain_record(grant.owner.tid, grant.shared_to(), grant.domain.is_some())
            })
        })
    };
    if page_hit {
        return true;
    }
    let table = reg_grant_table_lock().lock();
    table.as_ref().is_some_and(|grants| {
        grants.values().any(|grant| {
            residual_domain_record(grant.owner.tid, grant.shared_to(), grant.domain.is_some())
        })
    })
}

/// No record can be domain-backed on an architecture without a private-root
/// backend. Compiled only for the configuration where the admission module exists
/// but the grant backend does not (`native-domains` on an uncovered target); a
/// build without the feature never calls this.
#[cfg(all(feature = "native-domains", not(cellos_wide)))]
pub(crate) fn domain_grant_records_live() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DmaGrantError {
    NotOwned,
    BdfNotOwned,
    QuotaExceeded,
    Pin(crate::memory::pin::PinError),
    PublicationBusy,
}

/// The `GrantRegister` table: persistent Grant buffers for a cell's lifetime,
/// one [`Region`] each, keyed by base address, with one grantee at a time via
/// `GrantShare`/`GrantSlice` as in [`grant_table_lock`]. An owner tid of 0
/// denotes an owner-dead VFS handoff awaiting reap.
pub(super) fn reg_grant_table_lock() -> &'static Spinlock<Option<RegionTable>> {
    &REG_GRANT_TABLE
}

/// Restore the boot identity mapping (kernel RW+NX) and deallocate a grant's
/// run of physical frames.
///
/// Grant frames are identity-mapped at boot (kernel RW+NX) and re-mapped USER RW by
/// `alloc_grant_pages`. On free we must RESTORE the kernel identity mapping — NOT
/// unmap it: in the SAS model every Usable frame must stay identity-mapped so the
/// cell loader can zero a reused frame through its identity address
/// (`phys_to_virt(frame)`). Unmapping here left freed grant frames with no PTE →
/// a later cell load store-faulted while zeroing BSS (and read wrong pages).
/// Dropping the USER bit also prevents a stale cell from touching a reused frame.
///
/// The kernel-alias restore happens only after `unpublish_user_frames` has left
/// the frames unmapped on every hart and drained in-flight Sas copies: a copy
/// that probed the grant earlier would otherwise write the reused frame
/// through the fault-free kernel mapping.
///
/// The restore applies only where the kernel reaches RAM through the identity
/// VA (`phys_to_virt` is the identity: RV64, AArch64). x86_64 reaches RAM
/// through the HHDM and has no boot identity map of RAM, so there the frames
/// are unpublished, the kernel-root tables their USER mapping needed are pruned
/// once empty, and the frames are freed; restoring would add a kernel alias
/// to the lower half that never existed.
///
/// Lock order: KERNEL_ROOT alone (unpublish), then FRAME_ALLOCATOR →
/// (map_page → KERNEL_ROOT). Mirrors `Stack::drop`. Must NOT be called while
/// already holding FRAME_ALLOCATOR.
pub(super) fn free_grant_pages(run: crate::memory::frame_run::FrameRun) {
    free_grant_run(
        run,
        #[cfg(feature = "p01-trace")]
        None,
    );
}

/// Release a removed grant row's frames: the caller took the row out of its
/// table under the table lock and checked that no pin holds it.
fn free_grant_row(row: Region) {
    if let Some(run) = row.into_run() {
        free_grant_pages(run);
    }
}

/// Release a removed grant row's frames, withheld while a pin, lease or export
/// still protects them.
fn release_row_unless_pinned(row: Region) {
    if let Some(run) = row.into_run() {
        release_unless_pinned(run);
    }
}

/// Resume the release of withheld frames whose key ended: the run comes back
/// `Mapped` from the frame-run quarantine and takes [`free_grant_pages`]'s path.
pub(super) fn free_released_range(range: crate::memory::pin::ReleasedRange) {
    let crate::memory::pin::ReleasedRange {
        run,
        #[cfg(feature = "p01-trace")]
        p01_range,
        #[cfg(feature = "p01-trace")]
        p01_obligation,
    } = range;
    free_grant_run(
        run,
        #[cfg(feature = "p01-trace")]
        Some((p01_range, p01_obligation)),
    );
}

/// Release a reclaimed grant's run: withheld, still mapped, while a pin, lease
/// or export protects it; otherwise unpublished, flushed and freed.
fn release_unless_pinned(run: crate::memory::frame_run::FrameRun) {
    if let Some(run) = withhold_if_pinned(run) {
        free_grant_run(
            run,
            #[cfg(feature = "p01-trace")]
            None,
        );
    }
}

/// Release the backing of `row`, a domain row whose record is revoked; only
/// the revoker whose removal took the row calls this.
///
/// The revoke removed the owner's and the receiver's private-root leaves and
/// confirmed both tags, and the frames never lost the kernel's own alias, so
/// the run has nothing to unpublish, flush or restore: it is only freed (RV64
/// and AArch64 skip the old unmap-and-restore round trip of the identity
/// alias). With `withhold`, a pin, lease or export that still protects the
/// range keeps the run, still mapped, with that alias. A row whose record is
/// not `Revoked` keeps its frames: they are leaked (counted), never freed.
#[cfg(cellos_domain_grants)]
pub(super) fn release_domain_backing(record: &DomainRecord, row: Region, withhold: bool) {
    let Some(run) = row.into_run() else {
        log::error!("[grant] revoked domain row released without its backing");
        return;
    };
    if record.state() != crate::task::domain_grant::DomainGrantState::Revoked {
        crate::memory::frame_run::PENDING
            .leak(run, "domain row taken before its record was revoked");
        return;
    }
    if withhold {
        release_unless_pinned(run);
    } else {
        free_grant_run(
            run,
            #[cfg(feature = "p01-trace")]
            None,
        );
    }
}

/// No row carries a domain record on this target ([`clone_domain_record`] is
/// always `None`), so no revoker reaches this; a row that did would keep its
/// frames (leaked, counted), never free them.
#[cfg(not(cellos_domain_grants))]
pub(super) fn release_domain_backing(_record: &DomainRecord, row: Region, _withhold: bool) {
    if let Some(run) = row.into_run() {
        crate::memory::frame_run::PENDING.leak(run, "no domain lifecycle on this target");
    }
}

/// A grant's frames as one run billed to nobody, still mapped: USER in the
/// shared root (and, on RV64/AArch64, over the kernel's identity alias, which
/// the release restores). Fixtures only; a cell's grant is billed to it
/// (`alloc_owned_grant`).
#[cfg(any(test, feature = "test-hooks"))]
pub(super) fn grant_run(base: usize, n_pages: usize) -> crate::memory::frame_run::FrameRun {
    use crate::memory::cell_quota::Charge;
    use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind};
    FrameRun::mapped(
        base,
        n_pages,
        Charge::KERNEL,
        RunKind::Leaf,
        KernelAlias::of_identity_remap(base),
    )
}

/// Refuse an owner-initiated teardown of `[base, base + size)` while an
/// in-flight asynchronous operation still holds any part of it.
///
/// `kind` and `id` name the request in the log; the frames must not go back to
/// the allocator while a device or the kernel can still write them.
///
/// # Errors
/// [`SyscallError::PermissionDenied`] when the region is pinned.
pub(super) fn refuse_if_pinned(
    kind: &str,
    id: usize,
    base: usize,
    size: usize,
) -> Result<(), SyscallError> {
    let pinned_span = grant_allocated_bytes(size);
    let Some(held) = crate::memory::pin::holder_of(base, pinned_span) else {
        return Ok(());
    };
    log::warn!(
        "[grant] {kind} {id:#x} refused: region {base:#x}+{pinned_span} overlaps a pinned region \
         {:#x}+{} pages owned by task {} with {} in-flight operation(s){}",
        held.base,
        held.pages,
        held.owner,
        held.holds,
        if held.quarantined {
            " (quarantined: owner died, awaiting driver acknowledgement)"
        } else {
            ""
        }
    );
    Err(SyscallError::PermissionDenied)
}

/// Remove a caller-owned registered grant after acquiring its ownership lock.
///
/// The registered-grant table is deliberately this operation's first
/// synchronization point: the final GetRandom output lease uses the same lock
/// to serialize validation, write, and teardown.
pub(super) fn unregister_registered_grant(
    caller_id: usize,
    reg_id: usize,
) -> Result<(), SyscallError> {
    // A private root whose lifecycle is not available (non-RV64, retired, or
    // already dying) keeps the phase-01 denial: this path cannot revoke a
    // receiver PTE for it, so the record must not leave the table.
    if domain_grant_task(caller_id) && !domain_grant_capable(caller_id) {
        log::warn!(
            "[grant] GrantUnregister denied: task {caller_id} is not a live private root \
             (phase-01 denial)"
        );
        return Err(GRANT_DENY_ERROR);
    }
    let taken = take_owned_row(reg_grant_table_lock(), "GrantUnregister", caller_id, reg_id)?
        .ok_or(SyscallError::PermissionDenied)?;
    match taken {
        TakenGrant::Domain(record) => {
            if !revoke_domain_record(&record, reg_id, "GrantUnregister") {
                return Err(SyscallError::PermissionDenied);
            }
            // A concurrent revoker of the same record (`CapRevoke`'s reclaim,
            // the deferred sweep after an ownerless retry) may have taken the
            // row first; it then owns the frames, and this unregister is
            // complete. A key that now names another cell's grant over reused
            // frames is left alone.
            release_if_row_taken(
                reg_grant_table_lock(),
                reg_id,
                |grant| carries_record(&grant.domain, &record),
                |row| release_domain_backing(&record, row, false),
            );
        }
        TakenGrant::Row(row) => free_grant_row(row),
    }
    Ok(())
}

/// What an owner's `GrantFree`/`GrantUnregister` took from its table.
enum TakenGrant {
    /// A domain row stays in the table while its record revokes; the revoker
    /// whose removal then takes the row releases the frames.
    Domain(DomainRecord),
    /// A SAS row, removed: its frames are the caller's to free.
    Row(Region),
}

/// Take `id` from `table` for its owner `caller_id`'s free, unless an
/// in-flight operation still holds the region (`refuse_if_pinned`); `None`
/// when the caller owns no such row.
///
/// The pin check runs inside the table lock (order: grant table → pin
/// `REGISTRY`, a leaf), so a concurrent VFS `GrantSlice` or `GrantDma` cannot
/// publish a pin between the check and the removal.
fn take_owned_row(
    table: &Spinlock<Option<RegionTable>>,
    kind: &str,
    caller_id: usize,
    id: usize,
) -> Result<Option<TakenGrant>, SyscallError> {
    let mut rows = table.lock();
    let Some(row) = rows
        .as_ref()
        .and_then(|rows| rows.get(&id))
        .filter(|row| row.owner.tid == caller_id)
    else {
        return Ok(None);
    };
    refuse_if_pinned(kind, id, row.base(), row.size)?;
    if let Some(record) = clone_domain_record(&row.domain) {
        return Ok(Some(TakenGrant::Domain(record)));
    }
    Ok(rows
        .as_mut()
        .and_then(|rows| rows.remove(&id))
        .map(TakenGrant::Row))
}

/// Reclaim all grant pages owned or held by a dying task.
///
/// Called from every task-exit code path (Exit syscall, ForceExit, scheduler watchdog, fault handler).
/// Three effects:
///   1. Owner death  — remove entry, unmap pages, return frames to allocator.
///   2. Grantee death — clear `shared_to` so the owner's grant becomes unshared.
///   3. Pinned frames — quarantined instead of freed, so a device still
///      programmed against them cannot write into the next cell allocated them.
///      The death itself is never delayed for a pin.
///
/// Lock order: PIN_TABLE (leaf) → PAGE_GRANT_TABLE collect → FRAME_ALLOCATOR →
/// KERNEL_ROOT (inside `unmap_page`/`map_page`). Never holds FRAME_ALLOCATOR
/// when calling free_grant_pages, and never holds PIN_TABLE across either.
pub(crate) fn reap_grants_for_task(dead_tid: usize) -> GrantReap {
    // Pins outlive their owner: a device authorised through the IOMMU keeps its
    // mapping after the cell is gone. Mark them before sweeping the tables so
    // the frames below are withheld rather than recycled.
    let pinned = crate::memory::pin::quarantine_task(dead_tid);
    if pinned > 0 {
        log::warn!(
            "[grant] task {dead_tid} died holding {pinned} pinned region(s); frames quarantined \
             ({} page(s) total withheld) until the driver acknowledges",
            crate::memory::pin::quarantined_pages()
        );
    }

    let (grantee_refs_cleared, writable_borrows) = clear_grantee_refs(dead_tid);
    let grants_reclaimed = reclaim_owned_grants(dead_tid);
    sweep_orphan_reg_grants();
    GrantReap {
        pins_quarantined: pinned,
        grantee_refs_cleared,
        writable_borrows,
        grants_reclaimed,
    }
}

/// What [`reap_grants_for_task`] did for one dead task (retirement verdict input).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GrantReap {
    /// Pin and cache-sync regions moved to quarantine.
    pub pins_quarantined: usize,
    /// Grants this task received that became unshared.
    pub grantee_refs_cleared: usize,
    /// Of those, grants it could write (`WriteOnly` or `ReadWrite`).
    pub writable_borrows: usize,
    /// Grants this task owned that were removed from either table.
    pub grants_reclaimed: usize,
}

/// Drop `tid`'s grantee-side references in both tables: a grant it *received*
/// becomes unshared again and its owner keeps both the entry and the frames.
/// Returns `(cleared, writable)`: references cleared and, of those, the ones
/// with write permission.
///
/// Death only. A runtime revoke must not touch the target's received grants —
/// those are governed by the granter's authority, not by the target's.
///
/// A domain receiver's PTE lives in the dying cell's private root and its record
/// retains that root, so the receiver half is drained here (PTE removal plus
/// acknowledgement) before the record is left unshared. The drain runs outside
/// every table lock: it awaits an invalidation the table lock must not hold.
pub(super) fn clear_grantee_refs(tid: usize) -> (usize, usize) {
    let mut cleared = 0usize;
    let mut writable = 0usize;
    let mut count = |perm: GrantPerm| {
        cleared += 1;
        if matches!(perm, GrantPerm::WriteOnly | GrantPerm::ReadWrite) {
            writable += 1;
        }
    };
    let mut drained: Vec<DomainRecord> = Vec::new();
    {
        let mut tbl = grant_table_lock().lock();
        if let Some(map) = tbl.as_mut() {
            for grant in map.values_mut() {
                if let Some((_, perm)) = grant.shared_to().filter(|(grantee, _)| *grantee == tid) {
                    count(perm);
                    grant.set_shared_to(None);
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        drained.push(record);
                    }
                }
            }
        }
    } // PAGE_GRANT_TABLE lock released

    {
        let mut tbl = reg_grant_table_lock().lock();
        if let Some(map) = tbl.as_mut() {
            for grant in map.values_mut() {
                if let Some((_, perm)) = grant.shared_to().filter(|(grantee, _)| *grantee == tid) {
                    count(perm);
                    grant.set_shared_to(None);
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        drained.push(record);
                    }
                }
            }
        }
    } // REG_GRANT_TABLE lock released

    for record in drained {
        drain_domain_receiver(&record);
    }
    (cleared, writable)
}

/// Remove and release every grant `tid` **owns**, in both tables.
///
/// The owner-side half of the reaper, shared by cell death and by runtime
/// revocation: a revoked Cell loses its owned grants while it keeps running.
/// Reclaiming an owned grant is also what unmaps it from a grantee — the
/// `shared_to` link is the derivation record, so no separate CDT is needed.
///
/// Frames an in-flight operation still holds are withheld (quarantine) instead
/// of freed, exactly as on the death path: the device the grant authorised may
/// still be programmed against them, and [`release_acked_frames`] returns them
/// once the driver acknowledges the teardown.
///
/// Returns the number of owned grants taken from `tid`: rows removed from
/// either table plus domain rows handed to deferred revocation. A grant whose
/// ownership passed to a live grantee is not counted.
///
/// Lock order (identical to the reaper's): PIN_TABLE (leaf) → grant table
/// collect → FRAME_ALLOCATOR → KERNEL_ROOT inside `free_grant_pages`. Never
/// holds FRAME_ALLOCATOR while calling `free_grant_pages`.
pub(crate) fn reclaim_owned_grants(tid: usize) -> usize {
    // ── PAGE_GRANT_TABLE pass ─────────────────────────────────────────────────
    // A domain row's record is cloned out and revoked after the table lock is
    // released: revocation awaits the receiver root's tag invalidation, which
    // must never hold this lock in the meantime.
    let mut page_deferred: Vec<(usize, DomainRecord, usize)> = Vec::new();
    let owned: alloc::vec::Vec<Region> = {
        let mut tbl = grant_table_lock().lock();
        let mut owned = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let owned_keys: alloc::vec::Vec<usize> = map
                .iter()
                .filter(|(_, g)| g.owner.tid == tid)
                .map(|(k, _)| *k)
                .collect();
            for key in owned_keys {
                match map.get(&key) {
                    Some(grant) if grant.domain.is_some() => {
                        page_deferred.push((
                            key,
                            clone_domain_record(&grant.domain).expect("checked above"),
                            grant.base(),
                        ));
                    }
                    Some(_) => {
                        if let Some(grant) = map.remove(&key) {
                            owned.push(grant);
                        }
                    }
                    None => {}
                }
            }
        }
        owned
    }; // PAGE_GRANT_TABLE lock released

    let mut reclaimed = owned.len() + page_deferred.len();
    for grant in owned {
        release_row_unless_pinned(grant);
    }

    for (key, record, base) in page_deferred {
        release_deferred_domain_row(key, record, base, false);
    }

    // ── REG_GRANT_TABLE pass ──────────────────────────────────────────────────
    let mut reg_deferred: Vec<(usize, DomainRecord, usize)> = Vec::new();
    let reg_owned: alloc::vec::Vec<Region> = {
        let mut tbl = reg_grant_table_lock().lock();
        let mut removed = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let mut owned_keys = alloc::vec::Vec::new();
            for (&key, grant) in map.iter_mut() {
                if grant.owner.tid != tid {
                    continue;
                }
                if grant.domain.is_some() {
                    reg_deferred.push((
                        key,
                        clone_domain_record(&grant.domain).expect("checked above"),
                        grant.base(),
                    ));
                    continue;
                }
                if let Some((grantee, _)) = grant.shared_to() {
                    // A live VFS request lease or `GrantSlice` holder lease
                    // keeps the frames: the row is orphaned and its frames go
                    // to the quarantine instead of passing to the grantee.
                    let (base, span) = (grant.base(), grant_allocated_bytes(grant.size));
                    if crate::memory::pin::vfs_holder_of_owner(base, span, tid).is_some()
                        || crate::memory::pin::grant_lease_holder_of_owner(base, span, tid)
                            .is_some()
                    {
                        grant.orphan();
                        owned_keys.push(key);
                    } else if let Some((cell_id, generation)) = live_task_binding(grantee) {
                        // Preserve the legacy transfer only to a live,
                        // generation-attested grantee.
                        grant.owner = RegionOwner::task(grantee, cell_id, generation);
                    } else {
                        grant.orphan();
                        owned_keys.push(key);
                    }
                } else {
                    owned_keys.push(key);
                }
            }
            removed = owned_keys.iter().filter_map(|k| map.remove(k)).collect();
        }
        removed
    }; // REG_GRANT_TABLE lock released

    reclaimed += reg_owned.len() + reg_deferred.len();
    for reg in reg_owned {
        release_row_unless_pinned(reg);
    }
    for (key, record, base) in reg_deferred {
        release_deferred_domain_row(key, record, base, true);
    }
    reclaimed
}

/// Retry owner-reclaimed domain rows whose invalidation was unacknowledged.
///
/// The scheduler's housekeeping pass runs this, not only another task's exit
/// (census F5). Completing the revoke is what finally releases the record's
/// backing; until then it stays with the record, still `Mapped`, so nothing
/// reuses memory a stale translation could still reach.
///
/// A row whose revoke waits on a tag the reaper has not confirmed is skipped
/// (one quarantine lookup per root) instead of retried: a retry would scan
/// every page again and still not complete. The reaper re-arms the pass when
/// it completes a tag wait. Returns `true` only while a retried row still
/// waits on something else, which re-arms the pass itself.
pub(crate) fn sweep_deferred_domain_grants() -> bool {
    let mut page_retry: Vec<(usize, DomainRecord)> = Vec::new();
    {
        let tbl = grant_table_lock().lock();
        if let Some(map) = tbl.as_ref() {
            for (&key, grant) in map.iter() {
                if grant.owner.tid == 0 {
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        page_retry.push((key, record));
                    }
                }
            }
        }
    }
    let mut waiting = false;
    for (key, record) in page_retry {
        if domain_revoke_awaits_tag(&record) {
            continue;
        }
        if !retry_domain_revoke(&record) {
            waiting |= !domain_revoke_awaits_tag(&record);
            continue;
        }
        // Another hart's sweep, or another revoker, may have completed the same
        // record: only the one whose removal took the row frees its frames.
        release_if_row_taken(
            grant_table_lock(),
            key,
            |grant| carries_record(&grant.domain, &record),
            |row| release_domain_backing(&record, row, true),
        );
    }

    let mut reg_retry: Vec<(usize, DomainRecord)> = Vec::new();
    {
        let tbl = reg_grant_table_lock().lock();
        if let Some(map) = tbl.as_ref() {
            for (&key, grant) in map.iter() {
                if grant.owner.tid == 0 {
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        reg_retry.push((key, record));
                    }
                }
            }
        }
    }
    for (key, record) in reg_retry {
        if domain_revoke_awaits_tag(&record) {
            continue;
        }
        if !retry_domain_revoke(&record) {
            waiting |= !domain_revoke_awaits_tag(&record);
            continue;
        }
        // Another hart's sweep, or another revoker, may have completed the same
        // record: only the one whose removal took the row frees its frames.
        release_if_row_taken(
            reg_grant_table_lock(),
            key,
            |grant| carries_record(&grant.domain, &record),
            |row| release_domain_backing(&record, row, true),
        );
    }
    waiting
}

/// Release registered grants with neither owner nor grantee: the residue of a
/// transfer that found no live grantee.
///
/// Death-only sweep — the state it clears is created by owner death, and a
/// runtime revoke neither creates nor resolves it.
pub(super) fn sweep_orphan_reg_grants() {
    let removed: alloc::vec::Vec<Region> = {
        let mut tbl = reg_grant_table_lock().lock();
        let mut removed = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let orphan_keys: alloc::vec::Vec<usize> = map
                .iter()
                .filter(|(_, g)| g.owner.tid == 0 && g.shared_to().is_none() && g.domain.is_none())
                .map(|(k, _)| *k)
                .collect();
            removed = orphan_keys.iter().filter_map(|k| map.remove(k)).collect();
        }
        removed
    }; // REG_GRANT_TABLE lock released

    for reg in removed {
        release_row_unless_pinned(reg);
    }
}

/// Monotonic boot-wide count of pages leaked (never reused): the frame-run
/// quarantine's `leaked` counter, which [`withhold_if_pinned`]'s unrecorded
/// holds and every other frame-run leak add to. A delta across a window counts
/// every leak in that window, including concurrent leaks attributed to other
/// tasks: it can over-report, never under-report.
pub(crate) fn leaked_pages() -> usize {
    crate::memory::frame_run::PENDING.counters().leaked()
}

/// Return quarantined frames owned by `tid` to the allocator once the driver has
/// acknowledged that nothing can still reach them.
///
/// The acknowledgement point is `iommu::revoke_dma_for_cell(tid)`: with the cell's IOTLB
/// entries flushed and its DDT/context entries zeroed, no device it authorised
/// can address the frames. Call this immediately after that teardown, and only
/// with a task id — the pin registry is keyed by task, not by cell.
///
/// Order-insensitive with respect to [`reap_grants_for_task`]: an acknowledgement
/// that arrives first drops the pins, so the reaper then frees the frames itself.
///
/// Must run with neither FRAME_ALLOCATOR nor SCHEDULER held.
pub(crate) fn release_acked_frames(tid: usize) {
    for range in crate::memory::pin::acknowledge(tid) {
        let (base, pages) = (range.base(), range.pages());
        log::info!(
            "[grant] task {tid} acknowledged: releasing {pages} quarantined page(s) at {base:#x}"
        );
        free_released_range(range);
    }
}

/// End every lease `tid` held as a holder: VFS request leases and `GrantSlice`
/// `LEASE` leases. Runs at holder death, outside every grant/scheduler lock.
pub(crate) fn release_vfs_holder_leases(tid: usize) {
    for range in crate::memory::pin::release_vfs_holder_leases(tid) {
        let (base, pages) = (range.base(), range.pages());
        log::info!(
            "[grant] VFS holder {tid} died: releasing {pages} quarantined page(s) at {base:#x}"
        );
        free_released_range(range);
    }
    free_released_grant_lease_frames(tid, crate::memory::pin::release_grant_holder_leases(tid));
}

/// End the caller's `GrantSlice` leases on Grants owned by the task a send
/// form is addressed to (ABI a3 `LEASE`, approved 2026-11-10).
///
/// Runs before the send takes effect, so the owner can never act on the
/// message while the lease still blocks its `GrantFree`. `Reply` is addressed
/// to the caller's current IPC caller, not to its a0. `IpcReply` resolves its
/// destination from an async token and is not a boundary; the holder's next
/// receive still ends the lease.
pub(super) fn end_grant_leases_at_send(caller_id: usize, syscall: &Syscall) {
    if !crate::memory::pin::grant_leases_live() {
        return;
    }
    let target = match *syscall {
        Syscall::Send { target, .. }
        | Syscall::SendGather { target, .. }
        | Syscall::IpcSubmit { target, .. } => target,
        // TrySend's target 0 is a self-wake.
        Syscall::TrySend { target: 0, .. } => caller_id,
        Syscall::TrySend { target, .. } => target,
        _ => return,
    };
    free_released_grant_lease_frames(
        caller_id,
        crate::memory::pin::release_grant_leases_to_owner(caller_id, target),
    );
}

/// Release the exact VFS lease whose owner death cleared the holder's caller
/// context. This runs outside SCHEDULER after grant reaping has quarantined any
/// owner-dead frames.
pub(crate) fn release_vfs_context_lease(release: crate::sched::scheduler::VfsLeaseRelease) {
    for range in crate::memory::pin::release_vfs_lease(
        release.holder_tid,
        release.grant_owner,
        release.request_generation,
    ) {
        free_released_range(range);
    }
}

/// 437-439 results: each refusal keeps its own `abi::dma_export::ERR_*`
/// sentinel (CP1 §5, E21); an export id is at most `isize::MAX`.
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_dma_export_result(result: SyscallResult) -> usize {
    use abi::dma_export as de;
    match result {
        Ok(value) => value,
        Err(SyscallError::InvalidInput | SyscallError::InvalidCommand) => de::ERR_INVALID_INPUT,
        Err(SyscallError::FileNotFound) => de::ERR_FILE_NOT_FOUND,
        Err(SyscallError::NotSupported) => de::ERR_NOT_SUPPORTED,
        Err(SyscallError::OutOfMemory) => de::ERR_OUT_OF_MEMORY,
        Err(SyscallError::TryAgain) => de::ERR_TRY_AGAIN,
        Err(SyscallError::BufferTooSmall) => de::ERR_BUFFER_TOO_SMALL,
        Err(SyscallError::Unknown) => de::ERR_UNKNOWN,
        Err(_) => de::ERR_PERMISSION_DENIED,
    }
}

#[cfg(not(target_arch = "riscv32"))]
pub(super) fn is_dma_export_call(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::ExportDmaRange { .. }
            | Syscall::MapDmaExport { .. }
            | Syscall::RevokeDmaExport { .. }
    )
}

/// A copied request needs no long-lived lease; the reply span must stay backed
/// until after the owner mutation and final write, including parked stream IO.
pub(super) struct SessionOutputPin {
    pub(super) span: Option<(usize, usize, usize)>,
}

#[derive(Clone, Copy)]
pub(in crate::task) struct VfsGrantContext {
    pub(super) grant_owner: usize,
    pub(super) grant_owner_cell_id: u64,
    pub(super) grant_owner_cell_generation: u64,
    pub(super) request_generation: u64,
    pub(super) pending_revoke: bool,
}

pub(in crate::task) enum VfsGrantLookup {
    NotVfs,
    MissingContext,
    Active(VfsGrantContext),
}

pub(in crate::task) fn current_vfs_grant_lookup(caller_id: usize) -> VfsGrantLookup {
    let sched_guard = crate::task::SCHEDULER.lock();
    let Some(task) = sched_guard
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
    else {
        return VfsGrantLookup::NotVfs;
    };
    if !crate::cell::service_registry::retains_outer_caller(task.cell_id.0, task.cell_generation) {
        return VfsGrantLookup::NotVfs;
    }
    match task.caller.current {
        Some(grant_owner) => VfsGrantLookup::Active(VfsGrantContext {
            grant_owner,
            grant_owner_cell_id: task.caller.cell_id,
            grant_owner_cell_generation: task.caller.cell_generation,
            request_generation: task.caller.request_generation,
            pending_revoke: crate::memory::pin::vfs_lease_pending_revoke(
                caller_id,
                grant_owner,
                task.caller.request_generation,
            ),
        }),
        None => VfsGrantLookup::MissingContext,
    }
}

pub(super) fn finish_vfs_send_release(
    caller_id: usize,
    target: usize,
    context: Option<VfsGrantContext>,
) {
    let Some(context) = context else {
        return;
    };
    if target != context.grant_owner {
        return;
    }
    let released = crate::memory::pin::release_vfs_lease(
        caller_id,
        context.grant_owner,
        context.request_generation,
    );
    for range in released {
        free_released_range(range);
    }
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            let _ = task
                .clear_current_caller_context_if(context.grant_owner, context.request_generation);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GrantSliceRequest {
    pub(super) caller_id: usize,
    pub(super) grant_id: usize,
    pub(super) size_out_ptr: usize,
    pub(super) expected_owner_tid: usize,
    /// ABI a3 `LEASE`; the handler has already required a nonzero
    /// `expected_owner_tid`, which keys the lease.
    pub(super) lease: bool,
    pub(super) vfs_context: Option<VfsGrantContext>,
}

pub(super) struct GrantSliceAccess {
    pub(super) base: usize,
    pub(super) size: usize,
    pub(super) size_out: Option<crate::task::user_out::UsizeOutSlot>,
}

/// GrantSlice ABI a2: a grantee (the compositor) cannot otherwise learn who
/// registered a grant shared to it, so a guessable grant id would let any
/// client present another client's buffer as its own. A nonzero
/// `expected_owner_tid` must name the row's owner; a mismatch is reported
/// exactly as an unknown id, so the probe reveals neither existence nor owner.
/// Zero keeps the pre-ABI behavior (no owner expectation).
pub(super) fn grant_owner_expected(expected_owner_tid: usize, owner: usize) -> bool {
    expected_owner_tid == 0 || expected_owner_tid == owner
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_registered_grant_owner(reg_id: usize) -> Option<usize> {
    let tbl = reg_grant_table_lock().lock();
    tbl.as_ref()
        .and_then(|map| map.get(&reg_id))
        .map(|grant| grant.owner.tid)
}

pub(super) fn reserve_dma_publication(
    tid: usize,
    quota: Option<crate::memory::cell_quota::Reservation>,
) -> Option<DmaPublicationGuard> {
    if tid == 0 {
        return None;
    }
    let mut publications = DMA_PUBLICATIONS.lock();
    if publications.contains(&tid) {
        return None;
    }
    let slot = publications.iter().position(|entry| *entry == 0)?;
    publications[slot] = tid;
    Some(DmaPublicationGuard { slot, tid, quota })
}

/// Log and drop a row [`RegionTable::insert`] refused. Its id is the address
/// of a frame a live row still owns, so its run names that row's frames: it is
/// leaked (counted and logged by `FrameRun`'s drop), never freed, and the live
/// row is untouched. Reaching this means the allocator handed out an owned
/// frame. Called with no table lock held.
fn refuse_duplicate_row(what: &str, id: usize, refused: Region) {
    log::error!("[grant] {what}: id {id:#x} already names a live row; the new row is refused");
    drop(refused);
}

/// Refuse a `GrantAlloc`/`GrantRegister` row after [`alloc_owned_grant`]
/// already published its frames. Those frames are a live row's
/// ([`refuse_duplicate_row`]), so the caller must keep no translation of them
/// once that row frees them: every translation this allocation published is
/// removed first, then the run is leaked (counted), never freed. Called with no
/// table lock held.
fn refuse_owned_grant(what: &str, id: usize, owner_space: &Option<DomainRoot>, refused: Region) {
    log::error!("[grant] {what}: id {id:#x} already names a live row; the new row is refused");
    let Some(run) = refused.into_run() else {
        return;
    };
    unpublish_owned_grant(owner_space, &run);
    crate::memory::frame_run::PENDING.leak(run, "refused grant row names a live row's frames");
}

/// Remove every translation [`alloc_owned_grant`] published for `run`; the run
/// itself stays with the caller.
///
/// A domain owner's pages leave its own root and its tag is confirmed once. An
/// unconfirmed tag is only logged: these frames cannot be held back behind it,
/// because the live row owns them. A SAS owner's USER leaves are unpublished
/// from the shared root (flushed, copy windows drained) and, where RAM has no
/// identity alias (x86_64), the tables they needed are detached. The identity
/// alias they replaced (RV64, AArch64) is not restored: the frames are not this
/// run's to hand back to the kernel, and a missing leaf faults rather than
/// exposes them.
fn unpublish_owned_grant(
    owner_space: &Option<DomainRoot>,
    run: &crate::memory::frame_run::FrameRun,
) {
    match owner_space {
        #[cfg(cellos_domain_grants)]
        Some(space) => {
            unmap_owner_pages(space, run.base(), run.pages());
            if crate::memory::tlb_shootdown::confirm_tag_invalidation(space.asid()).is_err() {
                log::warn!(
                    "[grant] refused row at {:#x}: owner tag {} unconfirmed after its unmap",
                    run.base(),
                    space.asid()
                );
            }
        }
        // `alloc_owned_grant` gives a domain owner no run on this target.
        #[cfg(not(cellos_domain_grants))]
        Some(_) => {}
        None => {
            let (base, pages) = (run.base(), run.pages());
            let mut batch = crate::memory::frame_run::Batch::new();
            crate::task::stack::unpublish_user_frames(&mut batch, base, pages);
            if run.alias() == crate::memory::frame_run::KernelAlias::Intact {
                prune_unpublished_tables(&mut batch, base, pages);
            }
            let _ = batch.complete();
        }
    }
}

/// Record `ShmAlloc` frame `frame`, `size` bytes requested, as a region of
/// `owner` that owns the frame and its `charge` (exactly the frame, refunded
/// when it is freed) from here on; `recipient` is the cell it was handed to,
/// if any. `false` when the table refused the row ([`refuse_duplicate_row`]).
///
/// The frame is still the kernel's identity-mapped frame (RV64, AArch64) or
/// unmapped at its identity VA (x86_64); `ShmMap` publishes it USER there, and
/// the region's release restores the identity alias where RAM is reached
/// through it (`KernelAlias::of_identity_remap`).
pub(super) fn shm_register(
    frame: usize,
    owner: crate::cell::state_stash::Incarnation,
    size: usize,
    charge: crate::memory::cell_quota::Reservation,
    recipient: Option<u64>,
) -> bool {
    use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind};
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    let run = FrameRun::mapped(
        frame,
        1,
        charge,
        RunKind::Leaf,
        KernelAlias::of_identity_remap(frame),
    );
    let mut region = Region::shm(shm_owner(owner), run, size);
    if let Some(cell) = recipient {
        region.receiver = Receiver::Cell(cell);
    }
    let inserted = grant_table_lock()
        .lock()
        .get_or_insert_with(RegionTable::new)
        .insert(frame, region);
    match inserted {
        None => true,
        Some(refused) => {
            refuse_duplicate_row("ShmAlloc", frame, refused);
            false
        }
    }
}

pub(super) fn dma_range_within(
    grant_base: usize,
    grant_size: usize,
    dma_base: usize,
    dma_size: usize,
) -> bool {
    let Some(grant_end) = grant_base.checked_add(grant_size) else {
        return false;
    };
    let Some(dma_end) = dma_base.checked_add(dma_size) else {
        return false;
    };
    grant_base <= dma_base && dma_end <= grant_end
}

/// Does `grant`, a row of either grant table, authorize DMA over
/// `[dma_base, dma_base + dma_size)` for `caller_id` bound as `caller_binding`?
pub(super) fn grant_authorizes_dma(
    grant: &Region,
    caller_id: usize,
    caller_binding: (CellId, u64),
    dma_base: usize,
    dma_size: usize,
) -> bool {
    grant.owner.tid == caller_id
        && grant.owner.binding() == caller_binding
        && dma_range_within(grant.base(), grant.size, dma_base, dma_size)
}

/// Reserve one already-authorized DMA publication while the caller still holds
/// the owning grant table and scheduler locks. The fixed-slot lifecycle marker
/// allocates no memory under `SCHEDULER`; pin publication remains protected by
/// the grant lock.
pub(super) fn reserve_authorized_dma_publication(
    caller_id: usize,
    caller: crate::memory::cell_quota::Charge,
    bdf: u32,
    base: usize,
    size: usize,
) -> Result<DmaPublicationGuard, DmaGrantError> {
    if !crate::dev::ownership::owns_pci(caller_id, bdf) {
        return Err(DmaGrantError::BdfNotOwned);
    }
    #[cfg(target_arch = "x86_64")]
    if let Some(dev) = crate::dev::pcie_ecam::device(bdf) {
        if !crate::dev::pcie_ecam::memory_bars_assigned(&dev.bars) {
            return Err(DmaGrantError::BdfNotOwned);
        }
    }
    let quota = crate::memory::cell_quota::Reservation::take(
        caller,
        crate::memory::cell_quota::Account::Dma,
        size,
    )
    .map_err(|_| DmaGrantError::QuotaExceeded)?;
    let publication =
        reserve_dma_publication(caller_id, Some(quota)).ok_or(DmaGrantError::PublicationBusy)?;
    crate::memory::pin::pin(base, size, caller_id).map_err(DmaGrantError::Pin)?;
    Ok(publication)
}

/// Validate and pin a caller-owned DMA span, then return a lifecycle marker
/// that prevents root retirement until IOMMU publication commits or rolls back.
pub(super) fn reserve_caller_owned_dma_range(
    caller_id: usize,
    bdf: u32,
    base: usize,
    size: usize,
) -> Result<DmaPublicationGuard, DmaGrantError> {
    {
        let page_grants = grant_table_lock().lock();
        let scheduler = crate::task::SCHEDULER.lock();
        let caller_binding = scheduler
            .as_ref()
            .and_then(|scheduler| scheduler.tasks.get(&caller_id))
            .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
            .map(|task| (task.cell_id, task.cell_generation))
            .ok_or(DmaGrantError::NotOwned)?;
        let authorized = page_grants.as_ref().is_some_and(|grants| {
            grants
                .values()
                .any(|grant| grant_authorizes_dma(grant, caller_id, caller_binding, base, size))
        });
        if authorized {
            return reserve_authorized_dma_publication(
                caller_id,
                crate::memory::cell_quota::Charge::new(caller_binding.0, caller_binding.1),
                bdf,
                base,
                size,
            );
        }
    }

    {
        let registered_grants = reg_grant_table_lock().lock();
        let scheduler = crate::task::SCHEDULER.lock();
        let caller_binding = scheduler
            .as_ref()
            .and_then(|scheduler| scheduler.tasks.get(&caller_id))
            .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
            .map(|task| (task.cell_id, task.cell_generation))
            .ok_or(DmaGrantError::NotOwned)?;
        let authorized = registered_grants.as_ref().is_some_and(|grants| {
            grants
                .values()
                .any(|grant| grant_authorizes_dma(grant, caller_id, caller_binding, base, size))
        });
        if authorized {
            return reserve_authorized_dma_publication(
                caller_id,
                crate::memory::cell_quota::Charge::new(caller_binding.0, caller_binding.1),
                bdf,
                base,
                size,
            );
        }
    }

    Err(DmaGrantError::NotOwned)
}

/// Allocate `n_pages` contiguous physical frames, zero them, and map them USER RW.
///
/// Returns the physical base address on success, or `None` on OOM, partial map
/// or an unconfirmed invalidation of a replaced translation.
/// Lock order: FRAME_ALLOCATOR (alloc) → FRAME_ALLOCATOR (map_page) → release;
/// the batch flushes and frees after FRAME_ALLOCATOR is released.
pub(super) fn alloc_grant_pages(n_pages: usize) -> Option<usize> {
    use crate::memory::frame_run::Batch;
    use crate::memory::paging::Flags;
    const PAGE_SIZE: usize = 4096;

    let user_flags =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;

    let paddr = crate::memory::frame::allocate_contiguous(n_pages)?;
    let identity_alias = crate::memory::frame::phys_to_virt(paddr) == paddr;
    // Zero through the frames' kernel alias before any USER mapping exists, as
    // `ShmAlloc` does: stale data from a previously freed grant never reaches
    // another cell (info-disclosure under G2), and the kernel never writes the
    // grant through its USER VA (SMAP on x86_64).
    // SAFETY: the run is freshly allocated and owned here; its kernel alias
    // (identity on RV64/AArch64, still kernel RW, the HHDM on x86_64) is
    // writable and contiguous.
    unsafe {
        core::ptr::write_bytes(
            crate::memory::frame::phys_to_virt(paddr) as *mut u8,
            0,
            n_pages * PAGE_SIZE,
        );
    }

    let mut batch = Batch::new();
    let mapped = crate::memory::frame::with(|alloc| {
        let mut mapped = 0usize;
        for i in 0..n_pages {
            let v = paddr + i * PAGE_SIZE;
            if crate::memory::paging::map_page(
                alloc,
                &mut batch,
                v,
                v,
                Flags::from_bits(user_flags),
            )
            .is_ok()
            {
                mapped += 1;
            } else {
                break;
            }
        }
        mapped
    })
    .unwrap_or(0);

    if mapped < n_pages {
        // Partial map: where RAM is identity-mapped (RV64, AArch64), restore the
        // kernel identity mapping for the pages we re-mapped USER (NOT a bare
        // unmap — every Usable frame must stay identity-mapped for the loader's
        // identity-address zeroing), then free. x86_64 has no identity alias
        // to restore, so a restore there would add a kernel lower-half
        // mapping (R3): its pages are left unmapped and the tables they needed
        // are pruned, as in `free_grant_pages`. Those pages were briefly
        // USER, so a Sas copy may have probed them: unpublish (unmap, flush,
        // drain copies) first on every target.
        crate::task::stack::unpublish_user_frames(&mut batch, paddr, mapped);
        if !identity_alias {
            // Pages `0..mapped` only: the page that failed (`mapped`) needs no
            // prune here, because `map_page`'s own failure path already pruned
            // every table it allocated for that page (on x86 into the batch),
            // and a table it shares with a mapped page is pruned with it.
            prune_unpublished_tables(&mut batch, paddr, mapped);
        }
        // The remapped prefix gets its identity alias back; the rest never
        // lost it.
        retire_grant_run(&mut batch, paddr, mapped, identity_alias);
        retire_grant_run(
            &mut batch,
            paddr + mapped * PAGE_SIZE,
            n_pages - mapped,
            false,
        );
        let _ = batch.complete();
        return None;
    }

    // A replaced translation (an earlier owner's alias of these frames) must be
    // gone on every hart before the cell can use the grant. If a hart does not
    // confirm, take the run back exactly as a failed map does.
    if !batch.complete().is_confirmed() {
        let mut batch = Batch::new();
        crate::task::stack::unpublish_user_frames(&mut batch, paddr, n_pages);
        if !identity_alias {
            prune_unpublished_tables(&mut batch, paddr, n_pages);
        }
        retire_grant_run(&mut batch, paddr, n_pages, identity_alias);
        let _ = batch.complete();
        return None;
    }

    Some(paddr)
}

/// Hand `n_pages` unpublished grant frames at `base` to `batch`.
/// `restore_alias`: their identity leaf was remapped USER and must be restored
/// kernel RW+NX before free (RV64, AArch64).
fn retire_grant_run(
    batch: &mut crate::memory::frame_run::Batch,
    base: usize,
    n_pages: usize,
    restore_alias: bool,
) {
    use crate::memory::cell_quota::Charge;
    use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind};
    if n_pages == 0 {
        return;
    }
    let alias = if restore_alias {
        KernelAlias::Removed
    } else {
        KernelAlias::Intact
    };
    batch.retire(FrameRun::unpublished(
        base,
        n_pages,
        Charge::KERNEL,
        RunKind::Leaf,
        alias,
    ));
}

/// Detach the kernel-root tables a USER mapping of `[base, base + n_pages)`
/// needed once it has been unpublished; `batch` invalidates before their
/// frames return (the `CellSegments::drop` pattern).
///
/// For targets without an identity alias of RAM (x86_64), where no kernel alias
/// is restored over the range and the tables may now be empty (R3).
fn prune_unpublished_tables(
    batch: &mut crate::memory::frame_run::Batch,
    base: usize,
    n_pages: usize,
) {
    const PAGE_SIZE: usize = 4096;
    for i in 0..n_pages {
        let f = base + i * PAGE_SIZE;
        crate::memory::paging::prune_empty_tables(f).retire_into(batch);
    }
}

#[cfg(feature = "p01-trace")]
type P01Release = (
    Option<crate::p01_retained::Token>,
    Option<crate::p01_resources::Obligation>,
);

fn free_grant_run(
    run: crate::memory::frame_run::FrameRun,
    #[cfg(feature = "p01-trace")] range: Option<P01Release>,
) {
    #[cfg(feature = "p01-trace")]
    const PAGE_SIZE: usize = 4096;
    #[cfg(feature = "p01-trace")]
    let (base, n_pages) = (run.base(), run.pages());
    #[cfg(feature = "p01-trace")]
    let _range_write = crate::p01_retained::release_write();
    #[cfg(feature = "p01-trace")]
    let allocated = crate::memory::frame::with(|alloc| {
        (0..n_pages).all(|i| alloc.p01_allocated(base + i * PAGE_SIZE))
    })
    .unwrap_or(false);
    // One batch: one shared invalidation for the run, copy windows drained,
    // then the alias restore and the free — or the quarantine of the run if a
    // hart does not confirm.
    let mut batch = crate::memory::frame_run::Batch::new();
    retire_grant_run_into(&mut batch, run);
    #[allow(unused_variables)] // reason: only the p01-trace witness reads it
    let flushed = batch.complete();
    #[cfg(feature = "p01-trace")]
    if let Some((p01_range, p01_obligation)) = range {
        if allocated && flushed.is_confirmed() {
            crate::p01_retained::retire(p01_range);
            crate::p01_resources::finish(p01_obligation, 1);
        } else {
            crate::p01_retained::fault();
        }
    }
}

/// [`free_grant_pages`] up to its flush: unpublish the run, detach the tables
/// its USER mapping needed (x86_64), and retire it into `batch`.
fn retire_grant_run_into(
    batch: &mut crate::memory::frame_run::Batch,
    run: crate::memory::frame_run::FrameRun,
) {
    const PAGE_SIZE: usize = 4096;
    let (base, n_pages) = (run.base(), run.pages());

    // Restore the kernel alias only where it is the identity map (RV64,
    // AArch64): the run's alias says which. x86_64 reaches RAM through the
    // HHDM and never had an identity alias for this frame, so a restore there
    // would add a new kernel mapping to the lower half.
    let identity_alias = run.alias() != crate::memory::frame_run::KernelAlias::Intact;
    // A domain owner's backing is never published in the kernel root: it is
    // zeroed through `phys_to_virt` and mapped only in private roots, whose tags
    // the revoke has already confirmed, and its run keeps the kernel's own alias
    // (`Intact`), from its allocation through any pin hold. Where that alias is
    // the identity VA (RV64, AArch64), the leaf found there is the alias itself,
    // not a publication; where it is the HHDM (x86_64), the identity VA has no
    // leaf at all. Either way there is nothing to unpublish (the shared
    // shootdown would only wait on, and fail-stop for, a mapping that never
    // existed) and no table the run ever needed: it is only deallocated. A SAS
    // grant's run on x86_64 is `Intact` too, and its USER leaf at the identity
    // VA is a publication.
    let published = identity_alias
        || (crate::memory::frame::phys_to_virt(base) != base
            && (0..n_pages)
                .any(|i| crate::memory::paging::virt_to_phys(base + i * PAGE_SIZE).is_some()));
    retire_published_run_into(batch, run, published);
}

/// Unpublish a grant or SHM run that is `published` in the shared root (its
/// USER leaves unmapped, invalidated, copy windows drained; on x86_64 the
/// tables those leaves needed detached), then retire it into `batch`, which
/// frees it — alias restored where the run says so — once the flush confirms.
fn retire_published_run_into(
    batch: &mut crate::memory::frame_run::Batch,
    mut run: crate::memory::frame_run::FrameRun,
    published: bool,
) {
    use crate::memory::frame_run::KernelAlias;
    let (base, n_pages) = (run.base(), run.pages());
    if published {
        crate::task::stack::unpublish_user_frames(batch, base, n_pages);
        if run.alias() == KernelAlias::Intact {
            prune_unpublished_tables(batch, base, n_pages);
        }
    }
    // Every leaf of the run is gone from the shared root (or never existed).
    // A run in any other state than `Mapped` cannot reach here; the batch
    // would leak it (counted), never free it.
    let unpublished = run.unpublish();
    debug_assert!(unpublished.is_ok(), "grant run released twice");
    batch.retire(run);
}

/// Test-hooks: resume the release of withheld grant frames a fixture took
/// back from the pin registry itself, as production callers do.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_free_released(ranges: Vec<crate::memory::pin::ReleasedRange>) {
    for range in ranges {
        free_released_range(range);
    }
}

/// Test-hooks: a grant run exactly as `GrantAlloc` builds it.
#[cfg(feature = "test-hooks")]
// reason: only the x86 silent-CPU fixture uses it.
#[allow(dead_code)]
pub(crate) fn test_alloc_grant_pages(n_pages: usize) -> Option<usize> {
    alloc_grant_pages(n_pages)
}

/// Test-hooks: [`free_grant_pages`] into the caller's `batch`, so one
/// operation retires a grant run together with other owners.
#[cfg(feature = "test-hooks")]
// reason: only the x86 silent-CPU fixture uses it.
#[allow(dead_code)]
pub(crate) fn test_retire_grant_pages_into(
    batch: &mut crate::memory::frame_run::Batch,
    base: usize,
    n_pages: usize,
) {
    retire_grant_run_into(batch, grant_run(base, n_pages));
}

/// Test-hooks: one page of grant backing for `caller_id`, mapped exactly as
/// `GrantAlloc` maps it before its row is recorded.
#[cfg(all(
    cellos_domain_grants,
    feature = "test-hooks",
    not(cellos_posture = "release")
))]
pub(crate) fn test_alloc_owned_grant(
    caller_id: usize,
) -> Option<crate::memory::frame_run::FrameRun> {
    let bill = reserve_grant_frames(caller_id, 1).ok().flatten()?;
    alloc_owned_grant(
        "refusal fixture",
        caller_id,
        &live_domain_space(caller_id),
        1,
        bill,
    )
}

/// Test-hooks: the translation removal a refused row takes
/// ([`refuse_owned_grant`]) before it leaks the run.
#[cfg(all(
    cellos_domain_grants,
    feature = "test-hooks",
    not(cellos_posture = "release")
))]
pub(crate) fn test_unpublish_refused_grant(
    caller_id: usize,
    run: &crate::memory::frame_run::FrameRun,
) {
    unpublish_owned_grant(&live_domain_space(caller_id), run);
}

/// Test-hooks: return a fixture run to the allocator after
/// [`test_unpublish_refused_grant`]. Unlike a refused row's, its frames name no
/// live row. A domain owner's leaves are already gone and its fixture task never
/// runs on a hart, so the run is released with nothing left to unmap.
#[cfg(all(
    cellos_domain_grants,
    feature = "test-hooks",
    not(cellos_posture = "release")
))]
pub(crate) fn test_free_refused_grant(caller_id: usize, run: crate::memory::frame_run::FrameRun) {
    match live_domain_space(caller_id) {
        Some(space) => release_owner_mapped_backing(&space, run, 0),
        None => free_grant_pages(run),
    }
}

/// Exercise registered-grant teardown without decoder or scheduler pre-locks.
///
/// Available only to the in-kernel race fixture; production callers use
/// `GrantUnregister` through `handle_syscall`.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_unregister_registered_grant_for_race(
    caller_id: usize,
    reg_id: usize,
) -> Result<(), SyscallError> {
    unregister_registered_grant(caller_id, reg_id)
}

/// Reissue an unregistered one-page race grant at its exact former frame.
///
/// The fixture has already removed `base` through the production unregister
/// path. Returns `true` only after the new caller-owned record is mapped and
/// its replacement bytes have been cleared.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_reregister_registered_grant_for_race(caller_id: usize, base: usize) -> bool {
    use crate::memory::paging::Flags;

    let claimed =
        crate::memory::frame::with(|allocator| allocator.claim_exact_frame_for_test(base))
            .unwrap_or(false);
    if !claimed {
        return false;
    }
    let user_flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY,
    );
    let mut batch = crate::memory::frame_run::Batch::new();
    let mapped = crate::memory::frame::with(|allocator| {
        let _ = crate::memory::paging::unmap_page(&mut batch, base);
        crate::memory::paging::map_page(allocator, &mut batch, base, base, user_flags).is_ok()
    })
    .unwrap_or(false);
    let mapped = batch.complete().is_confirmed() && mapped;
    let run = grant_run(base, 1);
    if !mapped {
        free_grant_pages(run);
        return false;
    }
    let zeroed = with_test_sum(|| unsafe {
        // SAFETY: the one-page race grant above is mapped USER RW at `base`.
        core::ptr::write_bytes(base as *mut u8, 0, 4096);
        core::slice::from_raw_parts((base + 32) as *const u8, 64)
            .iter()
            .all(|byte| *byte == 0)
    });
    if !zeroed {
        free_grant_pages(run);
        return false;
    }
    let mut table = reg_grant_table_lock().lock();
    let binding = live_task_binding(caller_id)
        .filter(|_| !table.as_ref().is_some_and(|grants| grants.occupied(&base)));
    let Some((owner_cell, owner_generation)) = binding else {
        drop(table);
        free_grant_pages(run);
        return false;
    };
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    // `occupied` was checked under this lock, so the insert takes the id.
    let inserted = table.get_or_insert_with(RegionTable::new).insert(
        base,
        Region::grant(
            RegionOwner::task(caller_id, owner_cell, owner_generation),
            run,
            128,
            None,
        ),
    );
    drop(table);
    match inserted {
        None => true,
        Some(refused) => {
            refuse_duplicate_row("GrantRegister fixture", base, refused);
            false
        }
    }
}

/// Free owner-dead frames whose last `GrantSlice` lease `holder` just ended.
///
/// Must run with no grant-table, scheduler or pin lock held: the frames go
/// back through `FRAME_ALLOCATOR` and `KERNEL_ROOT`.
pub(super) fn free_released_grant_lease_frames(
    holder: usize,
    released: Vec<crate::memory::pin::ReleasedRange>,
) {
    for range in released {
        let (base, pages) = (range.base(), range.pages());
        log::info!(
            "[grant] GrantSlice lease holder {holder} released {pages} quarantined page(s) at \
             {base:#x}"
        );
        free_released_range(range);
    }
}

/// Copy `len` bytes of a grant the caller owns into a kernel-owned ELF snapshot
/// (`SpawnFromElf` and session route 2). Admission must never read the grant
/// itself: the caller's other threads and grant peers can rewrite it between
/// signature verification and segment load.
pub(crate) fn snapshot_grant_elf(
    caller: usize,
    grant_id: usize,
    len: usize,
) -> Result<crate::loader::aligned_elf::ElfSnapshot, SyscallError> {
    if len == 0 {
        return Err(SyscallError::InvalidInput);
    }
    // Resolved before the table lock so SCHEDULER is not held across the copy.
    let view = caller_copy_view(caller)?;
    // The table lock is held across the copy because GrantFree and owner
    // reclaim remove the row under it before returning frames: the source
    // cannot be freed mid-copy. The copy is charged to the caller's cell, as
    // SpawnFromMem's is.
    let guard = grant_table_lock().lock();
    let table = guard.as_ref().ok_or(SyscallError::InvalidInput)?;
    let g = table.get(&grant_id).ok_or(SyscallError::InvalidInput)?;
    if g.owner.tid != caller {
        return Err(SyscallError::PermissionDenied);
    }
    if len > g.size {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(g.base(), len, MAX_USER_BUF)?;
    let mut snapshot = crate::loader::aligned_elf::ElfSnapshot::zeroed(len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    view.read_into(g.base(), snapshot.bytes_mut())
        .map_err(|_| SyscallError::InvalidInput)?;
    Ok(snapshot)
}

/// Install the exact VFS request lease while its scheduler context is live.
///
/// Production callers hold the matching grant-table lock. The complete order is
/// therefore `PAGE_GRANT_TABLE` or `REG_GRANT_TABLE` → `SCHEDULER` → pin
/// `REGISTRY` (a leaf). Task exit holds `SCHEDULER` only while marking
/// `REGISTRY`, and grant teardown never takes `SCHEDULER`, so no inverse edge
/// exists.
pub(super) fn install_vfs_lease_if_context_live(
    caller_id: usize,
    context: VfsGrantContext,
    base: usize,
    size: usize,
    grant_id: usize,
) -> bool {
    let sched_guard = crate::task::SCHEDULER.lock();
    let Some(sched) = sched_guard.as_ref() else {
        return false;
    };
    let Some(holder) = sched.tasks.get(&caller_id) else {
        return false;
    };
    if !crate::cell::service_registry::retains_outer_caller(
        holder.cell_id.0,
        holder.cell_generation,
    ) || holder.caller.current != Some(context.grant_owner)
        || holder.caller.cell_id != context.grant_owner_cell_id
        || holder.caller.cell_generation != context.grant_owner_cell_generation
        || holder.caller.request_generation != context.request_generation
    {
        return false;
    }
    let Some(owner) = sched.tasks.get(&context.grant_owner) else {
        return false;
    };
    if owner.cell_id.0 != context.grant_owner_cell_id
        || owner.cell_generation != context.grant_owner_cell_generation
    {
        return false;
    }
    crate::memory::pin::pin_vfs_lease(
        base,
        size,
        context.grant_owner,
        caller_id,
        grant_id,
        context.request_generation,
    )
    .is_ok()
}

#[cfg(feature = "test-hooks")]
pub(in crate::task) fn test_install_vfs_lease_if_context_live(
    caller_id: usize,
    context: VfsGrantContext,
    base: usize,
    size: usize,
    grant_id: usize,
) -> bool {
    install_vfs_lease_if_context_live(caller_id, context, base, size, grant_id)
}

/// Publish the `LEASE` requested by a `GrantSlice` while the caller holds the
/// grant-table lock that resolved the row.
///
/// Lock order grant table → pin `REGISTRY` (leaf), the same edge the VFS lease
/// uses: the owner's `GrantFree`/`GrantUnregister` and the owner-death reaper
/// both remove the row under that table lock, so they either run first (the
/// row is gone and the slice answers as unknown) or observe this lease.
/// A refusal is reported as the unknown-id sentinel, so it is only logged.
pub(super) fn lease_grant_slice_locked(
    caller_id: usize,
    grant_id: usize,
    grant_owner: usize,
    base: usize,
    size: usize,
) -> bool {
    match crate::memory::pin::lease_grant(base, size, grant_owner, caller_id, grant_id) {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "[grant] GrantSlice {grant_id:#x} lease refused for task {caller_id}: {error:?}"
            );
            false
        }
    }
}

pub(super) fn authorize_grant_slice_locked(
    request: GrantSliceRequest,
    grant_owner: usize,
    shared_to_tid: Option<usize>,
    base: usize,
    size: usize,
    domain_owned: bool,
) -> Result<Option<GrantSliceAccess>, SyscallError> {
    // Checked first, before any log, output-slot validation or lease, so the
    // refusal is indistinguishable from the not-found `Ok(None)` below.
    if !grant_owner_expected(request.expected_owner_tid, grant_owner) {
        return Ok(None);
    }
    // A record owned by a private root is not sliceable for a SAS receiver: the
    // owner's backing lives in a root the SAS path cannot see, and a SAS receiver
    // would receive a raw mapping the private-root owner cannot audit. A managed
    // domain record reaches this path only for a SAS caller (a capable domain
    // receiver is routed to the lifecycle-aware resolver) and is denied here; a
    // pre-gate record that names a domain endpoint keeps its phase-01 denial for
    // every caller. The caller sees the established `usize::MAX` sentinel.
    if domain_owned || domain_grant_task(grant_owner) {
        log::warn!(
            "[grant] GrantSlice denied: grant owner {grant_owner} is a private-root domain \
             (no domain lifecycle for the receiver)"
        );
        return Ok(None);
    }
    let authorized = if let Some(context) = request.vfs_context {
        grant_owner == context.grant_owner && shared_to_tid == Some(request.caller_id)
    } else {
        grant_owner == request.caller_id || shared_to_tid == Some(request.caller_id)
    };
    if !authorized {
        return Ok(None);
    }

    // Validate every fallible output before publishing a lease. Once the lease
    // is installed, returning the raw mapping and writing this slot (through
    // the caller's copy view, after the table lock drops) are infallible, so a
    // failed GrantSlice cannot strand a pin.
    let size_out = crate::task::user_out::resolve_optional_usize_slot(
        request.caller_id,
        request.size_out_ptr,
    )?;
    if request.lease
        && !lease_grant_slice_locked(request.caller_id, request.grant_id, grant_owner, base, size)
    {
        return Ok(None);
    }
    if request.vfs_context.is_some_and(|context| {
        !install_vfs_lease_if_context_live(request.caller_id, context, base, size, request.grant_id)
    }) {
        // Still under the resolving table lock: the row (and so the owner) is
        // alive, so taking the hold back cannot strand a quarantine entry.
        if request.lease {
            crate::memory::pin::unlease_grant_hold(
                request.caller_id,
                grant_owner,
                request.grant_id,
            );
        }
        return Ok(None);
    }
    Ok(Some(GrantSliceAccess {
        base,
        size,
        size_out,
    }))
}

/// Resolve a grant and publish its requested leases in one table transaction.
///
/// This is the GrantSlice/free linearization point. If teardown removes the
/// entry first, neither table can resolve it. If this lookup wins, the VFS
/// request lease and the `LEASE` holder lease reach the pin registry before the
/// table lock is released, so GrantFree or GrantUnregister observes the pin and
/// refuses, and an owner-death reap quarantines the frames.
pub(super) fn resolve_and_lease_grant(
    request: GrantSliceRequest,
) -> Result<Option<GrantSliceAccess>, SyscallError> {
    {
        let tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_ref().and_then(|map| map.get(&request.grant_id)) {
            return authorize_grant_slice_locked(
                request,
                grant.owner.tid,
                grant.shared_to().map(|(tid, _)| tid),
                grant.base(),
                grant.size,
                grant.domain.is_some(),
            );
        }
    }
    let tbl = reg_grant_table_lock().lock();
    let Some(grant) = tbl.as_ref().and_then(|map| map.get(&request.grant_id)) else {
        return Ok(None);
    };
    authorize_grant_slice_locked(
        request,
        grant.owner.tid,
        grant.shared_to().map(|(tid, _)| tid),
        grant.base(),
        grant.size,
        grant.domain.is_some(),
    )
}

/// Handler for `Syscall::ShmAlloc` (extracted from the dispatcher).
pub(super) fn shm_alloc(caller_id: usize, size: usize) -> SyscallResult {
    let Some((cell, generation)) = live_cell_identity(caller_id) else {
        return Err(SyscallError::PermissionDenied);
    };
    let owner = crate::cell::state_stash::Incarnation { cell, generation };
    let page_size = crate::memory::paging::PAGE_SIZE;
    let bytes = if size == 0 {
        page_size
    } else {
        size.div_ceil(page_size) * page_size
    };

    // The frame is billed before it is taken; its run refunds it when the
    // frame is freed (at the incarnation's retirement, `release_shm_for`).
    let Ok(charge) = crate::memory::cell_quota::Reservation::take(
        crate::memory::cell_quota::Charge::new(CellId(cell), generation),
        crate::memory::cell_quota::Account::Memory,
        page_size,
    ) else {
        return Err(SyscallError::BufferTooSmall);
    };

    if let Some(frame) = crate::memory::frame::allocate_frame() {
        // A free frame keeps whatever its last owner wrote, and `ShmMap`
        // publishes it USER RW: zero it before the handle exists (owner
        // decision F9), as `alloc_grant_pages` does for a grant.
        // SAFETY: the frame was just allocated and nothing maps it USER yet;
        // `phys_to_virt` is the kernel RW alias every free frame keeps.
        unsafe {
            core::ptr::write_bytes(
                crate::memory::frame::phys_to_virt(frame) as *mut u8,
                0,
                page_size,
            );
        }
        if shm_register(frame, owner, bytes, charge, None) {
            return Ok(frame);
        }
        return Err(SyscallError::Unknown);
    }
    Err(SyscallError::BufferTooSmall)
}

/// Handler for `Syscall::ShmMap` (extracted from the dispatcher).
pub(super) fn shm_map(caller_id: usize, handle: usize, target_pid: usize) -> SyscallResult {
    let Some((caller_cell, caller_gen)) = live_cell_identity(caller_id) else {
        return Err(SyscallError::PermissionDenied);
    };
    let target_cell = if target_pid != 0 {
        live_cell_identity(target_pid).map(|(cell, _)| cell)
    } else {
        None
    };

    let frame = handle;
    let vaddr = frame; // Identity map for SAS simplicity

    use crate::memory::paging::Flags;
    let flags =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;

    let mut batch = crate::memory::frame_run::Batch::new();
    // Published under the grant table lock, so the owner's retirement takes the
    // row either before (nothing is mapped) or after (it unpublishes this).
    let mapped = with_accessible_shm(handle, caller_cell, caller_gen, target_cell, || {
        crate::memory::frame::with(|allocator| {
            crate::memory::paging::map_page(
                allocator,
                &mut batch,
                vaddr,
                frame,
                Flags::from_bits(flags),
            )
            .is_ok()
        })
    })
    .ok_or(SyscallError::PermissionDenied)?;
    // A replaced translation must be gone everywhere before the cell relies on
    // the mapping; if a hart does not confirm, the map fails closed (the frame
    // stays with its region, which `release_shm_for` unpublishes later).
    if batch.complete().is_confirmed() && mapped == Some(true) {
        return Ok(vaddr);
    }
    Err(SyscallError::Unknown)
}

/// Handler for `Syscall::AnonMemory` (extracted from the dispatcher).
pub(super) fn anon_memory(
    caller_id: usize,
    op: usize,
    addr: usize,
    len: usize,
    flags: usize,
) -> SyscallResult {
    #[cfg(target_arch = "x86_64")]
    {
        let is_domain = {
            let guard = crate::task::SCHEDULER.lock();
            guard
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|t| {
                    matches!(
                        t.address_space,
                        crate::task::tcb::TaskAddressSpace::Domain(_)
                    )
                })
                .unwrap_or(false)
        };
        if !is_domain {
            return Ok(abi::anon_memory::ERR_NOT_SUPPORTED);
        }
        // No anonymous page can exist yet (no budget source, CP1 D-M4
        // default 0 = `NotSupported`), so every valid op is refused:
        // a 0 from UNMAP or PROTECT would claim a release or a guard
        // page that never took effect.
        match abi::anon_memory::decode_anon_request(op, addr, len, flags) {
            Ok(_) => Ok(abi::anon_memory::ERR_NOT_SUPPORTED),
            Err(err) => Ok(err),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (caller_id, op, addr, len, flags);
        Ok(abi::anon_memory::ERR_NOT_SUPPORTED)
    }
}

/// Handler for `Syscall::GrantDma` (extracted from the dispatcher).
pub(super) fn grant_dma(caller_id: usize, bdf: u32, phys: u64, size: usize) -> SyscallResult {
    // Phase-01 containment: DMA authority is bounded at admission for
    // domain-class cells, so this is the second lock on the same door —
    // a private-root caller must not publish an IOMMU mapping for a
    // record whose receiver PTEs this kernel does not revoke.
    if domain_grant_task(caller_id) {
        log::warn!(
            "[iommu] Cell {caller_id} DMA grant denied: private-root domain \
                     (phase-01 containment)"
        );
        return Err(SyscallError::PermissionDenied);
    }
    #[cfg(target_arch = "aarch64")]
    let granule = hal::dma_granule();
    #[cfg(not(target_arch = "aarch64"))]
    let granule = 4096usize;

    let mask = (granule - 1) as u64;
    if phys & mask != 0 || (size as u64) & mask != 0 || size == 0 {
        return Err(SyscallError::InvalidInput);
    }
    let Some(phys_end) = phys.checked_add(size as u64) else {
        return Err(SyscallError::InvalidInput);
    };
    let Ok(phys_base) = usize::try_from(phys) else {
        return Err(SyscallError::InvalidInput);
    };
    if usize::try_from(phys_end).is_err() {
        return Err(SyscallError::InvalidInput);
    }
    let publication = match reserve_caller_owned_dma_range(caller_id, bdf, phys_base, size) {
        Ok(publication) => publication,
        Err(DmaGrantError::NotOwned) => {
            log::warn!(
                "[iommu] Cell {caller_id} DMA grant denied: {phys:#x}+{size} is not \
                             contained in a live caller-owned Grant"
            );
            return Err(SyscallError::PermissionDenied);
        }
        Err(DmaGrantError::BdfNotOwned) => {
            log::warn!("[iommu] Cell {caller_id} DMA grant denied: BDF {bdf:08x} is not owned");
            return Err(SyscallError::PermissionDenied);
        }
        Err(DmaGrantError::QuotaExceeded) => {
            log::warn!("[iommu] Cell {caller_id} DMA quota exceeded (size={size})");
            return Err(SyscallError::PermissionDenied);
        }
        Err(DmaGrantError::Pin(error)) => {
            log::warn!(
                "[iommu] Cell {caller_id} DMA grant denied: cannot pin \
                             {phys:#x}+{size} ({error:?})"
            );
            return Err(SyscallError::PermissionDenied);
        }
        Err(DmaGrantError::PublicationBusy) => {
            log::warn!("[iommu] Cell {caller_id} DMA grant denied: publication already in flight");
            return Err(SyscallError::Unknown);
        }
    };
    let iova = match crate::dev::iommu::map_dma_for_cell(caller_id as u64, bdf, phys, size) {
        crate::dev::iommu::DmaMapResult::Mapped(iova) => iova,
        crate::dev::iommu::DmaMapResult::Rejected => {
            let rolled_back = crate::memory::pin::rollback_pin(phys_base, size, caller_id);
            debug_assert!(rolled_back, "rejected DMA map must release its exact pin");
            drop(publication);
            log::warn!("[iommu] Cell {caller_id} DMA grant denied: no active mapping backend");
            return Err(SyscallError::Unknown);
        }
        crate::dev::iommu::DmaMapResult::PublishedUnconfirmed => {
            // A context/SLPT entry is already hardware-visible. Keep
            // the pin and quota until task teardown clears and drains
            // that exact requester; do not enable bus mastering.
            publication.commit();
            log::warn!(
                "[iommu] Cell {caller_id} DMA mapping unconfirmed: \
                             retaining pin for BDF {bdf:08x}"
            );
            return Err(SyscallError::Unknown);
        }
    };
    crate::dev::pcie_ecam::enable_bus_master(bdf);
    publication.commit();
    // warn, not info: the only record of what this device may DMA to
    // (see the matching line in `iommu_x86.rs`); at info level it does
    // not survive to the serial log on the Tier-3 x86 lane.
    log::warn!(
        "[iommu] Cell {} granted DMA BDF={:02x}:{:02x}.{} phys={:#x} size={}",
        caller_id,
        (bdf >> 8) & 0xFF,
        (bdf >> 3) & 0x1F,
        bdf & 0x7,
        phys,
        size
    );
    Ok(iova as usize)
}

/// Handler for `Syscall::ExportDmaRange` (extracted from the dispatcher).
pub(super) fn export_dma_range(
    caller_id: usize,
    va: usize,
    len: usize,
    holder_service: usize,
    flags: usize,
) -> SyscallResult {
    dma_export::export(caller_id, va, len, holder_service, flags)
}

/// Handler for `Syscall::MapDmaExport` (extracted from the dispatcher).
pub(super) fn map_dma_export(
    caller_id: usize,
    export_id: usize,
    bdf: usize,
    out_ptr: usize,
    out_len: usize,
) -> SyscallResult {
    if out_len != abi::dma_export::DMA_EXPORT_RECORD_LEN {
        return Err(SyscallError::InvalidInput);
    }
    caller_copy_view(caller_id)?
        .validate_writable(out_ptr, out_len)
        .map_err(|_| SyscallError::InvalidInput)?;
    // Same retirement barrier as GrantDma: the holder's root cannot
    // retire (and tear its domain down) mid-publication.
    let publication = reserve_dma_publication(caller_id, None).ok_or(SyscallError::TryAgain)?;
    let record = dma_export::map(caller_id, export_id, bdf);
    drop(publication);
    let mut bytes = [0u8; abi::dma_export::DMA_EXPORT_RECORD_LEN];
    record?.encode(&mut bytes);
    write_user_slice(caller_id, out_ptr, &bytes, bytes.len())
        .map_err(|_| SyscallError::InvalidInput)?;
    Ok(0)
}

/// Handler for `Syscall::RevokeDmaExport` (extracted from the dispatcher).
pub(super) fn revoke_dma_export(
    caller_id: usize,
    export_id: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> SyscallResult {
    if a1 != 0 || a2 != 0 || a3 != 0 {
        return Err(SyscallError::InvalidInput);
    }
    dma_export::revoke(caller_id, export_id)
}

/// The memory-quota reservation a new grant of `n_pages` frames draws from
/// `caller_id`'s incarnation, taken before any frame is allocated. The run
/// carries it, so it is refunded exactly when the frames return to the
/// allocator (free, unregister or reap), never while a pin withholds them.
///
/// # Errors
/// [`SyscallError::PermissionDenied`] for a caller with no live binding;
/// `Ok(None)` when its quota cannot cover the run.
pub(super) fn reserve_grant_frames(
    caller_id: usize,
    n_pages: usize,
) -> Result<Option<crate::memory::cell_quota::Reservation>, SyscallError> {
    use crate::memory::cell_quota::{Account, Charge, Reservation};
    const PAGE_SIZE: usize = 4096;
    let (cell, generation) = live_task_binding(caller_id).ok_or(SyscallError::PermissionDenied)?;
    Ok(Reservation::take(
        Charge::new(cell, generation),
        Account::Memory,
        n_pages * PAGE_SIZE,
    )
    .ok())
}

/// A new grant's frames for `caller_id`, billed with `bill`, or `None` for the
/// `Ok(0)` refusal (which drops `bill` and so refunds it).
///
/// A SAS owner gets them USER in the shared root (`alloc_grant_pages`). A
/// domain owner gets a supervisor-only run, mapped RW+NX in its own root only;
/// every page is undone if any one fails, so no raw pointer is ever published
/// for a partial map. The row's region owns the run.
fn alloc_owned_grant(
    kind: &str,
    caller_id: usize,
    owner_space: &Option<DomainRoot>,
    n_pages: usize,
    bill: crate::memory::cell_quota::Reservation,
) -> Option<crate::memory::frame_run::FrameRun> {
    match owner_space {
        #[cfg(cellos_domain_grants)]
        Some(space) => {
            let backing = alloc_grant_pages_supervisor_only(n_pages, bill)?;
            match map_domain_owner_pages(space, backing) {
                Ok(backing) => Some(backing),
                Err(error) => {
                    log::warn!(
                        "[grant] {kind} owner mapping failed for task {caller_id}: {error:?}"
                    );
                    None
                }
            }
        }
        #[cfg(not(cellos_domain_grants))]
        Some(_) => {
            let _ = (kind, caller_id, bill);
            None
        }
        None => alloc_grant_pages(n_pages).map(|paddr| billed_grant_run(paddr, n_pages, bill)),
    }
}

/// A new SAS grant's frames at `base`, mapped USER in the shared root (over
/// the identity alias on RV64/AArch64), carrying `bill` as their charge.
pub(super) fn billed_grant_run(
    base: usize,
    n_pages: usize,
    bill: crate::memory::cell_quota::Reservation,
) -> crate::memory::frame_run::FrameRun {
    use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind};
    FrameRun::mapped(
        base,
        n_pages,
        bill,
        RunKind::Leaf,
        KernelAlias::of_identity_remap(base),
    )
}

/// Release a new grant whose caller lost its live binding before the row was
/// recorded.
///
/// A domain owner's pages are already mapped in its own private root, so they
/// are unmapped there and the owner's tag confirmed before the frames are
/// freed; an unconfirmed tag quarantines them behind it
/// ([`release_owner_mapped_backing`]). A SAS owner's frames take the grant
/// release (unpublish, flush, free).
fn release_unbound_grant(
    owner_space: &Option<DomainRoot>,
    run: crate::memory::frame_run::FrameRun,
) {
    match owner_space {
        #[cfg(cellos_domain_grants)]
        Some(space) => {
            let pages = run.pages();
            release_owner_mapped_backing(space, run, pages);
        }
        // `alloc_owned_grant` gives a domain owner no run on this target.
        #[cfg(not(cellos_domain_grants))]
        Some(_) => {
            crate::memory::frame_run::PENDING.leak(run, "unbound grant backing without its root")
        }
        None => free_grant_pages(run),
    }
}

/// The lifecycle record a domain owner's new row carries over its backing at
/// `base`.
fn owned_domain_record(
    owner_space: &Option<DomainRoot>,
    base: usize,
    size: usize,
) -> Option<DomainRecord> {
    owner_space
        .as_ref()
        .and_then(|space| new_domain_record(space, base, size))
}

/// Handler for `Syscall::GrantAlloc` (extracted from the dispatcher).
pub(super) fn grant_alloc(caller_id: usize, size: usize) -> SyscallResult {
    const PAGE_SIZE: usize = 4096;
    if size == 0 || size > MAX_GRANT_PAGES * PAGE_SIZE {
        return Ok(0);
    }
    // A private root whose lifecycle is unavailable (non-RV64, unknown or
    // retired tid, a root already dying) keeps the phase-01 denial before
    // a frame exists. `Ok(0)` is the alloc-safe sentinel: a cell-side
    // wrapper reads any nonzero return as a grant id.
    if domain_grant_task(caller_id) && !domain_grant_capable(caller_id) {
        log::warn!(
            "[grant] GrantAlloc denied: task {caller_id} is not a live private root \
                     (phase-01 denial)"
        );
        return Ok(GRANT_DENY_ALLOC);
    }
    let owner_space = live_domain_space(caller_id);
    let n_pages = size.div_ceil(PAGE_SIZE);
    let Some(bill) = reserve_grant_frames(caller_id, n_pages)? else {
        return Ok(0);
    };
    let Some(run) = alloc_owned_grant("GrantAlloc", caller_id, &owner_space, n_pages, bill) else {
        return Ok(0);
    };
    let paddr = run.base();
    let mut table = grant_table_lock().lock();
    let (owner_cell, owner_generation) = match live_task_binding(caller_id) {
        Some(binding) => binding,
        None => {
            drop(table);
            release_unbound_grant(&owner_space, run);
            return Err(SyscallError::PermissionDenied);
        }
    };
    let domain = owned_domain_record(&owner_space, paddr, size);
    if table.is_none() {
        *table = Some(RegionTable::new());
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    let inserted = table.as_mut().unwrap().insert(
        paddr,
        Region::grant(
            RegionOwner::task(caller_id, owner_cell, owner_generation),
            run,
            size,
            domain,
        ),
    );
    drop(table);
    if let Some(refused) = inserted {
        refuse_owned_grant("GrantAlloc", paddr, &owner_space, refused);
        return Err(SyscallError::Unknown);
    }
    Ok(paddr)
}

/// Handler for `Syscall::GrantShare` (extracted from the dispatcher).
pub(super) fn grant_share(
    caller_id: usize,
    grant_id: usize,
    target_cell: usize,
    perm: usize,
) -> SyscallResult {
    let perm = match GrantPerm::try_from(perm as u8) {
        Ok(p) => p,
        Err(_) => return Err(SyscallError::InvalidInput),
    };
    // A private-root pair is the only shape the lifecycle can publish:
    // both endpoints must be live private roots on the one architecture
    // whose lifecycle is implemented. SAS-only shares keep today's path;
    // a mixed pair, a non-RV64 target and a dead or unknown peer keep the
    // phase-01 denial.
    let owner_is_domain = domain_grant_task(caller_id);
    let target_is_domain = domain_grant_task(target_cell);
    #[cfg(cellos_domain_grants)]
    if owner_is_domain || target_is_domain {
        let owner_space = live_domain_space(caller_id);
        let target_space = live_domain_space(target_cell);
        let (Some(_owner_space), Some(target_space)) = (owner_space, target_space) else {
            log::warn!(
                "[grant] GrantShare denied: caller {caller_id} or target {target_cell} is \
                         not a live private root"
            );
            return Err(SyscallError::PermissionDenied);
        };
        // Any invalidation wait for the receiver this share supersedes happens
        // here, outside the table lock (risk R4).
        drain_superseded_receiver(caller_id, grant_id, target_cell, &target_space, perm);
        {
            let mut tbl = grant_table_lock().lock();
            if let Some(grant) = tbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
                return domain_grant_share_row(grant, caller_id, target_cell, &target_space, perm)
                    .map(|()| 0);
            }
        }
        let mut rtbl = reg_grant_table_lock().lock();
        return match rtbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
            None => Err(SyscallError::InvalidInput),
            Some(grant) => {
                domain_grant_share_row(grant, caller_id, target_cell, &target_space, perm)
                    .map(|()| 0)
            }
        };
    }
    // Without a private-root backend no task is a domain, so the residual
    // branch below is the SAS-only path.
    let _ = (owner_is_domain, target_is_domain);
    // Check PAGE_GRANT_TABLE first, then fall back to REG_GRANT_TABLE.
    {
        let mut tbl = grant_table_lock().lock();
        if tbl.is_none() {
            *tbl = Some(RegionTable::new());
        }
        if let Some(grant) = tbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
            if grant.owner.tid != caller_id {
                return Err(SyscallError::PermissionDenied);
            }
            grant.set_shared_to(Some((target_cell, perm)));
            return Ok(0);
        }
    }
    // Not in PAGE_GRANT_TABLE — try REG_GRANT_TABLE.
    let mut rtbl = reg_grant_table_lock().lock();
    match rtbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
        None => Err(SyscallError::InvalidInput),
        Some(grant) if grant.owner.tid != caller_id => Err(SyscallError::PermissionDenied),
        Some(grant) => {
            grant.set_shared_to(Some((target_cell, perm)));
            Ok(0)
        }
    }
}

/// Handler for `Syscall::GrantSlice` (extracted from the dispatcher).
pub(super) fn grant_slice(
    caller_id: usize,
    grant_id: usize,
    size_out_ptr: usize,
    expected_owner_tid: usize,
    flags: usize,
) -> SyscallResult {
    // ABI a3 (approved 2026-11-10): unknown flag bits, and a lease with
    // no owner to key it by, answer exactly as an unknown id — before
    // any lookup, log or `size_out` validation.
    if flags & !abi::syscall::grant_slice::KNOWN_FLAGS != 0 {
        return Ok(GRANT_DENY_SLICE);
    }
    let lease = flags & abi::syscall::grant_slice::LEASE != 0;
    if lease && expected_owner_tid == 0 {
        return Ok(GRANT_DENY_SLICE);
    }
    // A caller that is not a live private root keeps the phase-01 denial
    // (`usize::MAX`): a private root that is retired, unknown or on an
    // unsupported target has no root this path could map into.
    if domain_grant_task(caller_id) && !domain_grant_capable(caller_id) {
        log::warn!(
            "[grant] GrantSlice denied: task {caller_id} is not a live private root \
                     (phase-01 denial)"
        );
        return Ok(GRANT_DENY_SLICE);
    }
    #[cfg(cellos_domain_grants)]
    if let Some(receiver) = live_domain_space(caller_id) {
        // A private-root receiver resolves the owner's exact rights and
        // publishes the receiver mapping transactionally; a VFS context
        // cannot exist for it, and no SAS record may be resolved.
        return resolve_domain_grant_slice(
            caller_id,
            grant_id,
            size_out_ptr,
            expected_owner_tid,
            lease,
            &receiver,
        );
    }
    let vfs_context = match current_vfs_grant_lookup(caller_id) {
        VfsGrantLookup::NotVfs => None,
        VfsGrantLookup::MissingContext => return Ok(usize::MAX),
        VfsGrantLookup::Active(context) if context.pending_revoke => return Ok(usize::MAX),
        VfsGrantLookup::Active(context) => Some(context),
    };
    let Some(access) = resolve_and_lease_grant(GrantSliceRequest {
        caller_id,
        grant_id,
        size_out_ptr,
        expected_owner_tid,
        lease,
        vfs_context,
    })?
    else {
        return Ok(usize::MAX);
    };
    crate::task::user_out::write_resolved_optional_usize(access.size_out, access.size);
    Ok(access.base)
}

/// Handler for `Syscall::GrantFree` (extracted from the dispatcher).
pub(super) fn grant_free(caller_id: usize, grant_id: usize) -> SyscallResult {
    // A private root whose lifecycle is unavailable keeps the phase-01
    // denial: releasing its record would return frames whose receiver PTE
    // this path cannot revoke.
    if domain_grant_task(caller_id) && !domain_grant_capable(caller_id) {
        log::warn!(
            "[grant] GrantFree denied: task {caller_id} is not a live private root \
                     (phase-01 denial)"
        );
        return Err(GRANT_DENY_ERROR);
    }
    // Owner-only, and only while no in-flight operation holds the region. A
    // domain row stays while the revoke drains so a retry is idempotent; a
    // `Revoking` row refuses new slices.
    let taken = take_owned_row(grant_table_lock(), "GrantFree", caller_id, grant_id)?
        .ok_or(SyscallError::PermissionDenied)?;
    match taken {
        TakenGrant::Domain(record) => {
            if !revoke_domain_record(&record, grant_id, "GrantFree") {
                // Bounded existing error: the frames stay retained and the
                // record stays `Revoking` for an idempotent retry.
                return Err(SyscallError::PermissionDenied);
            }
            // A concurrent revoker of the same record (owner exit, the
            // deferred sweep on another hart, `CapRevoke`'s reclaim) may have
            // taken the row first; it then owns the frames, and this free is
            // already complete.
            release_if_row_taken(
                grant_table_lock(),
                grant_id,
                |grant| carries_record(&grant.domain, &record),
                |row| release_domain_backing(&record, row, false),
            );
        }
        TakenGrant::Row(row) => free_grant_row(row),
    }
    Ok(0)
}

/// Handler for `Syscall::GrantCacheSyncBegin` (extracted from the dispatcher).
pub(super) fn grant_cache_sync_begin(
    caller_id: usize,
    grant_id: usize,
    offset: usize,
    len: usize,
) -> SyscallResult {
    let (token, base) = {
        let table = grant_table_lock().lock();
        let grant = table
            .as_ref()
            .and_then(|entries| entries.get(&grant_id))
            .ok_or(SyscallError::InvalidInput)?;
        if grant.owner.tid != caller_id {
            return Err(SyscallError::PermissionDenied);
        }
        if len == 0 || offset > grant.size || len > grant.size.saturating_sub(offset) {
            return Err(SyscallError::InvalidInput);
        }
        let base = grant
            .base()
            .checked_add(offset)
            .ok_or(SyscallError::InvalidInput)?;
        let token = crate::memory::pin::begin_cache_sync(base, len, caller_id)
            .map_err(|_| SyscallError::InvalidInput)?;
        (token, base)
    };
    #[cfg(target_arch = "aarch64")]
    {
        hal::cache::clean_invalidate_data_cache_range(base, len);
        Ok(token)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = base;
        let _ = crate::memory::pin::cancel_cache_sync(token, caller_id);
        Err(SyscallError::Unknown)
    }
}

/// Handler for `Syscall::GrantCacheSyncComplete` (extracted from the dispatcher).
pub(super) fn grant_cache_sync_complete(caller_id: usize, token: usize) -> SyscallResult {
    let (base, len) = crate::memory::pin::begin_cache_sync_completion(token, caller_id)
        .ok_or(SyscallError::PermissionDenied)?;
    #[cfg(target_arch = "aarch64")]
    {
        hal::cache::invalidate_data_cache_range(base, len);
        if !crate::memory::pin::complete_cache_sync(token, caller_id) {
            return Err(SyscallError::PermissionDenied);
        }
        Ok(0)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (base, len);
        Err(SyscallError::Unknown)
    }
}

/// Handler for `Syscall::GrantRegister` (extracted from the dispatcher).
pub(super) fn grant_register(caller_id: usize, size: usize) -> SyscallResult {
    const PAGE_SIZE: usize = 4096;
    if size == 0 || size > MAX_GRANT_PAGES * PAGE_SIZE {
        return Ok(0);
    }
    // A private root whose lifecycle is unavailable keeps the phase-01
    // denial: its buffer would be mapped into a root this path cannot
    // revoke a receiver from, so no frame or PTE may exist.
    if domain_grant_task(caller_id) && !domain_grant_capable(caller_id) {
        log::warn!(
            "[grant] GrantRegister denied: task {caller_id} is not a live private root \
                     (phase-01 denial)"
        );
        return Ok(GRANT_DENY_ALLOC);
    }
    let owner_space = live_domain_space(caller_id);
    let n_pages = size.div_ceil(PAGE_SIZE);
    let Some(bill) = reserve_grant_frames(caller_id, n_pages)? else {
        return Ok(0);
    };
    let Some(run) = alloc_owned_grant("GrantRegister", caller_id, &owner_space, n_pages, bill)
    else {
        return Ok(0);
    };
    let paddr = run.base();
    let mut table = reg_grant_table_lock().lock();
    let (owner_cell, owner_generation) = match live_task_binding(caller_id) {
        Some(binding) => binding,
        None => {
            drop(table);
            release_unbound_grant(&owner_space, run);
            return Err(SyscallError::PermissionDenied);
        }
    };
    let domain = owned_domain_record(&owner_space, paddr, size);
    if table.is_none() {
        *table = Some(RegionTable::new());
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    let inserted = table.as_mut().unwrap().insert(
        paddr,
        Region::grant(
            RegionOwner::task(caller_id, owner_cell, owner_generation),
            run,
            size,
            domain,
        ),
    );
    drop(table);
    if let Some(refused) = inserted {
        refuse_owned_grant("GrantRegister", paddr, &owner_space, refused);
        return Err(SyscallError::Unknown);
    }
    Ok(paddr)
}

/// Handler for `Syscall::GrantUnregister` (extracted from the dispatcher).
pub(super) fn grant_unregister(caller_id: usize, reg_id: usize) -> SyscallResult {
    unregister_registered_grant(caller_id, reg_id)?;
    Ok(0)
}
