// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: grant (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, ipc::*, misc::*, spawn::*, vm::*};

/// Physical frames currently issued via `ShmAlloc`, each bound to the Cell
/// incarnation that allocated it.
///
/// `ShmMap` accepts only a handle that appears here AND belongs to the
/// caller's live incarnation, so a cell can neither forge a physical address
/// ("ShmMap kernel_text_phys") nor map a peer's or a dead predecessor's frame.
/// Root retirement reclaims the dead incarnation's frames
/// ([`release_shm_for`]). A leaf lock: never held while taking another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShmRecord {
    pub(crate) owner: crate::cell::state_stash::Incarnation,
    pub(crate) recipient: Option<u64>,
    pub(crate) size: usize,
    pub(crate) rights: u32,
}

pub(super) const MAX_IN_FLIGHT_DMA_PUBLICATIONS: usize = 64;

pub(crate) struct DmaPublicationGuard {
    pub(super) slot: usize,
    pub(super) tid: usize,
    pub(super) quota: Option<crate::memory::cell_quota::DmaQuotaReservation>,
}

pub(crate) fn dma_publication_in_flight(tid: usize) -> bool {
    DMA_PUBLICATIONS.lock().contains(&tid)
}

#[cfg(test)]
pub(crate) fn test_reserve_dma_publication(tid: usize) -> Option<DmaPublicationGuard> {
    let quota = crate::memory::cell_quota::try_reserve_dma(0, 0)?;
    reserve_dma_publication(tid, quota)
}

#[cfg_attr(not(test), allow(dead_code))]
/// Whether `handle` is an outstanding `ShmAlloc` frame owned by exactly `owner`
/// (same Cell and generation).
pub(super) fn shm_owned_by(handle: usize, owner: crate::cell::state_stash::Incarnation) -> bool {
    SHM_HANDLES.lock().get(&handle).map(|r| r.owner) == Some(owner)
}

pub(super) fn shm_accessible_by(handle: usize, cell: u64, generation: u64) -> Option<ShmRecord> {
    let guard = SHM_HANDLES.lock();
    let record = guard.get(&handle).copied()?;
    if (record.owner.cell == cell && record.owner.generation == generation)
        || record.recipient == Some(cell)
    {
        Some(record)
    } else {
        None
    }
}

/// Remove every `ShmAlloc` record of `owner` and return its frames and total bytes.
pub(super) fn shm_take_for(owner: crate::cell::state_stash::Incarnation) -> (Vec<usize>, usize) {
    let mut frames = Vec::new();
    let mut total_bytes = 0usize;
    SHM_HANDLES.lock().retain(|&frame, record| {
        if record.owner == owner {
            frames.push(frame);
            total_bytes = total_bytes.saturating_add(record.size);
            false
        } else {
            true
        }
    });
    (frames, total_bytes)
}

/// Reclaim the `ShmAlloc` frames of a retired root's incarnation.
///
/// Called from the root-retirement funnel after the generation is quiescent.
/// `ShmMap` re-maps a frame's identity VA as USER. If that VA still maps the
/// frame, the USER mapping is removed; where the kernel reaches RAM through the
/// identity VA (`phys_to_virt` is the identity), the boot kernel mapping is
/// restored as in [`free_grant_pages`], since every Usable frame must stay
/// identity-mapped there. That restore runs only after
/// `unpublish_user_frames` has unmapped the page, flushed every hart and
/// drained in-flight Sas copies, so a copy that probed the USER page cannot
/// land in the frame through the fault-free kernel mapping after it is freed.
/// Returns the number of frames released: each was mapped USER in the shared
/// root with no borrower record, which the retirement verdict reports.
/// Lock order: SHM_HANDLES (released); KERNEL_ROOT alone (unpublish); then
/// FRAME_ALLOCATOR → KERNEL_ROOT.
pub(crate) fn release_shm_for(cell_id: u64, generation: u64) -> usize {
    use crate::memory::frame::FRAME_ALLOCATOR;
    use crate::memory::paging::Flags;
    let (frames, total_bytes) = shm_take_for(crate::cell::state_stash::Incarnation {
        cell: cell_id,
        generation,
    });
    if frames.is_empty() {
        return 0;
    }
    crate::memory::cell_quota::refund(cell_id as usize, total_bytes);
    let kernel_rwx = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );
    for &frame in &frames {
        if crate::memory::paging::virt_to_phys(frame) != Some(frame) {
            // The identity VA no longer maps this frame; only stale TLB
            // entries for it can remain.
            crate::memory::tlb_shootdown::flush_page(frame);
            continue;
        }
        crate::task::stack::unpublish_user_frames(frame, 1);
        if crate::memory::frame::phys_to_virt(frame) == frame {
            if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
                let _ = crate::memory::paging::map_page(allocator, frame, frame, kernel_rwx);
            }
        }
    }
    if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
        for &frame in &frames {
            allocator.deallocate_frame(frame);
        }
    }
    frames.len()
}

/// Kernel-managed zero-copy memory region.
///
/// Distinct from `tcb::GrantEntry`, which tracks per-task grants from the kernel.
/// The issuing task identity remains necessary for the existing grant ABI, but
/// the Cell/generation binding prevents a stale task record from becoming a
/// writable-output authority after retirement or transfer.
pub(super) struct PageGrant {
    pub(super) base: usize,
    pub(super) size: usize,
    pub(super) owner: usize,
    // Read by the RV64 SAS ownership ledger; retained in every build so a
    // grant cannot change its security identity across target tuples.
    #[allow(dead_code)]
    pub(super) owner_cell: CellId,
    #[allow(dead_code)]
    pub(super) owner_generation: u64,
    pub(super) shared_to: Option<(usize, GrantPerm)>,
    /// One kernel-owned lifecycle record, present only while the owner is a
    /// private-root domain. It carries the owner/receiver roots, the exact
    /// receiver mapping tuple and the `Live → Revoking → Revoked` state; a SAS
    /// owner keeps today's zero-copy path and leaves this `None`.
    pub(super) domain: Option<DomainRecord>,
}

pub(super) fn grant_table_lock() -> &'static Spinlock<Option<BTreeMap<usize, PageGrant>>> {
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
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(super) fn domain_grant_task(tid: usize) -> bool {
    task_domain_space(tid).is_some()
}

/// Without a private-root backend no task can be a domain.
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
)))]
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
/// x86_64 stays closed: it grew the same in-switch root write but has no
/// root-switch witness, so no entry point may publish a domain record there.
/// AArch64 production builds are pinned below.
pub(super) const fn domain_grant_lifecycle_supported() -> bool {
    cfg!(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            all(target_arch = "aarch64", feature = "test-hooks")
        )
    ))
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
/// that is still `Live`, on the one architecture whose lifecycle is
/// implemented. Every other shape keeps the denial sentinel: a non-RV64 target,
/// an unknown or retired tid, and a root already marked dying.
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
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) fn domain_grant_records_live() -> bool {
    let page_hit = {
        let table = grant_table_lock().lock();
        table.as_ref().is_some_and(|grants| {
            grants.values().any(|grant| {
                residual_domain_record(grant.owner, grant.shared_to, grant.domain.is_some())
            })
        })
    };
    if page_hit {
        return true;
    }
    let table = reg_grant_table_lock().lock();
    table.as_ref().is_some_and(|grants| {
        grants.values().any(|grant| {
            residual_domain_record(grant.owner, grant.shared_to, grant.domain.is_some())
        })
    })
}

/// No record can be domain-backed on an architecture without a private-root
/// backend. Compiled only for the configuration where the admission module exists
/// but the grant backend does not (`native-domains` on an uncovered target); a
/// build without the feature never calls this.
#[cfg(all(
    feature = "native-domains",
    not(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))
))]
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

/// Persistent kernel-managed Grant buffer for a cell's lifetime.
///
/// Supports one grantee at a time via `GrantShare`/`GrantSlice` (same as
/// `PageGrant`). `owner == 0` denotes an owner-dead VFS handoff awaiting reap.
pub(super) struct RegGrant {
    pub(super) base: usize,
    pub(super) size: usize,
    pub(super) owner: usize,
    pub(super) owner_cell: CellId,
    pub(super) owner_generation: u64,
    pub(super) shared_to: Option<(usize, GrantPerm)>,
    /// See [`PageGrant::domain`]: the same single lifecycle record behind the
    /// registered-grant table.
    pub(super) domain: Option<DomainRecord>,
}

pub(super) fn reg_grant_table_lock() -> &'static Spinlock<Option<BTreeMap<usize, RegGrant>>> {
    &REG_GRANT_TABLE
}

/// Restore the boot identity mapping (kernel RWX) and deallocate `n_pages`
/// physical frames starting at `base`.
///
/// Grant frames are identity-mapped at boot (RWX kernel) and re-mapped USER RW by
/// `alloc_grant_pages`. On free we must RESTORE the kernel identity mapping — NOT
/// unmap it: in the SAS model every Usable frame must stay identity-mapped so the
/// cell loader can zero a reused frame through its identity address
/// (`phys_to_virt(frame)`). Unmapping here left freed grant frames with no PTE →
/// a later cell load store-faulted while zeroing BSS (and read wrong pages).
/// Dropping the USER bit also prevents a stale cell from touching a reused frame.
///
/// The kernel-RWX restore happens only after `unpublish_user_frames` has left
/// the frames unmapped on every hart and drained in-flight Sas copies: a copy
/// that probed the grant earlier would otherwise write the reused frame
/// through the fault-free kernel mapping.
///
/// Lock order: KERNEL_ROOT alone (unpublish), then FRAME_ALLOCATOR →
/// (map_page → KERNEL_ROOT). Mirrors `Stack::drop`. Must NOT be called while
/// already holding FRAME_ALLOCATOR.
pub(super) fn free_grant_pages(base: usize, n_pages: usize) {
    free_grant_pages_inner(
        base,
        n_pages,
        #[cfg(feature = "p01-trace")]
        None,
    );
}

pub(super) fn free_released_range(range: crate::memory::pin::ReleasedRange) {
    free_grant_pages_inner(
        range.base,
        range.pages,
        #[cfg(feature = "p01-trace")]
        Some(range),
    );
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
    let entry = {
        let mut table = reg_grant_table_lock().lock();
        let owned = table
            .as_ref()
            .and_then(|grants| grants.get(&reg_id))
            .filter(|grant| grant.owner == caller_id)
            .map(|grant| (grant.base, grant.size, clone_domain_record(&grant.domain)));
        match owned {
            Some((base, size, domain)) => {
                refuse_if_pinned("GrantUnregister", reg_id, base, size)?;
                if domain.is_some() {
                    Some((base, size, domain))
                } else {
                    table
                        .as_mut()
                        .and_then(|grants| grants.remove(&reg_id))
                        .map(|grant| (grant.base, grant.size, None))
                }
            }
            None => None,
        }
    }
    .ok_or(SyscallError::PermissionDenied)?;
    let n_pages = grant_pages_for_size(entry.1);
    if let Some(record) = &entry.2 {
        if !revoke_domain_record(record, reg_id, "GrantUnregister") {
            return Err(SyscallError::PermissionDenied);
        }
        let mut table = reg_grant_table_lock().lock();
        table.as_mut().and_then(|grants| grants.remove(&reg_id));
    }
    free_grant_pages(entry.0, n_pages);
    Ok(())
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
    sweep_deferred_domain_grants();
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
                if let Some((_, perm)) = grant.shared_to.filter(|(grantee, _)| *grantee == tid) {
                    count(perm);
                    grant.shared_to = None;
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
                if let Some((_, perm)) = grant.shared_to.filter(|(grantee, _)| *grantee == tid) {
                    count(perm);
                    grant.shared_to = None;
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
    let mut page_deferred: Vec<(usize, DomainRecord, usize, usize)> = Vec::new();
    let owned: alloc::vec::Vec<PageGrant> = {
        let mut tbl = grant_table_lock().lock();
        let mut owned = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let owned_keys: alloc::vec::Vec<usize> = map
                .iter()
                .filter(|(_, g)| g.owner == tid)
                .map(|(k, _)| *k)
                .collect();
            for key in owned_keys {
                match map.get(&key) {
                    Some(grant) if grant.domain.is_some() => {
                        page_deferred.push((
                            key,
                            clone_domain_record(&grant.domain).expect("checked above"),
                            grant.base,
                            grant.size,
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

    for grant in &owned {
        if withhold_or_free(grant.base, grant_pages_for_size(grant.size)) {
            continue;
        }
        free_grant_pages(grant.base, grant_pages_for_size(grant.size));
    }

    let mut reclaimed = owned.len() + page_deferred.len();
    for (key, record, base, size) in page_deferred {
        release_deferred_domain_row(key, record, base, size, false);
    }

    // ── REG_GRANT_TABLE pass ──────────────────────────────────────────────────
    let mut reg_deferred: Vec<(usize, DomainRecord, usize, usize)> = Vec::new();
    let reg_owned: alloc::vec::Vec<RegGrant> = {
        let mut tbl = reg_grant_table_lock().lock();
        let mut removed = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let mut owned_keys = alloc::vec::Vec::new();
            for (&key, grant) in map.iter_mut() {
                if grant.owner != tid {
                    continue;
                }
                if grant.domain.is_some() {
                    reg_deferred.push((
                        key,
                        clone_domain_record(&grant.domain).expect("checked above"),
                        grant.base,
                        grant.size,
                    ));
                    continue;
                }
                if let Some((grantee, _)) = grant.shared_to {
                    // A live VFS request lease or `GrantSlice` holder lease
                    // keeps the frames: the row is orphaned and its frames go
                    // to the quarantine instead of passing to the grantee.
                    let span = grant_allocated_bytes(grant.size);
                    if crate::memory::pin::vfs_holder_of_owner(grant.base, span, tid).is_some()
                        || crate::memory::pin::grant_lease_holder_of_owner(grant.base, span, tid)
                            .is_some()
                    {
                        grant.owner = 0;
                        grant.owner_cell = CellId(0);
                        grant.owner_generation = 0;
                        grant.shared_to = None;
                        owned_keys.push(key);
                    } else if let Some((cell_id, generation)) = live_task_binding(grantee) {
                        // Preserve the legacy transfer only to a live,
                        // generation-attested grantee.
                        grant.owner = grantee;
                        grant.owner_cell = cell_id;
                        grant.owner_generation = generation;
                    } else {
                        grant.owner = 0;
                        grant.owner_cell = CellId(0);
                        grant.owner_generation = 0;
                        grant.shared_to = None;
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

    for reg in &reg_owned {
        if withhold_or_free(reg.base, grant_pages_for_size(reg.size)) {
            continue;
        }
        free_grant_pages(reg.base, grant_pages_for_size(reg.size));
    }
    reclaimed += reg_owned.len() + reg_deferred.len();
    for (key, record, base, size) in reg_deferred {
        release_deferred_domain_row(key, record, base, size, true);
    }
    reclaimed
}

/// Retry owner-reclaimed domain rows whose invalidation was unacknowledged.
///
/// Every task exit runs this for both tables. Completing the revoke is what
/// finally releases the retained frames; until then they stay quarantined with
/// the record so nothing reuses memory a stale translation could still reach.
pub(super) fn sweep_deferred_domain_grants() {
    let mut page_retry: Vec<(usize, DomainRecord, usize, usize)> = Vec::new();
    {
        let tbl = grant_table_lock().lock();
        if let Some(map) = tbl.as_ref() {
            for (&key, grant) in map.iter() {
                if grant.owner == 0 {
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        page_retry.push((key, record, grant.base, grant.size));
                    }
                }
            }
        }
    }
    for (key, record, base, size) in page_retry {
        if !revoke_domain_record(&record, base, "deferred-sweep") {
            continue;
        }
        let pages = grant_pages_for_size(size);
        {
            let mut tbl = grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        }
        if !withhold_or_free(base, pages) {
            free_grant_pages(base, pages);
        }
    }

    let mut reg_retry: Vec<(usize, DomainRecord, usize, usize)> = Vec::new();
    {
        let tbl = reg_grant_table_lock().lock();
        if let Some(map) = tbl.as_ref() {
            for (&key, grant) in map.iter() {
                if grant.owner == 0 {
                    if let Some(record) = clone_domain_record(&grant.domain) {
                        reg_retry.push((key, record, grant.base, grant.size));
                    }
                }
            }
        }
    }
    for (key, record, base, size) in reg_retry {
        if !revoke_domain_record(&record, base, "deferred-sweep") {
            continue;
        }
        let pages = grant_pages_for_size(size);
        {
            let mut tbl = reg_grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        }
        if !withhold_or_free(base, pages) {
            free_grant_pages(base, pages);
        }
    }
}

/// Release registered grants with neither owner nor grantee: the residue of a
/// transfer that found no live grantee.
///
/// Death-only sweep — the state it clears is created by owner death, and a
/// runtime revoke neither creates nor resolves it.
pub(super) fn sweep_orphan_reg_grants() {
    let removed: alloc::vec::Vec<RegGrant> = {
        let mut tbl = reg_grant_table_lock().lock();
        let mut removed = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            let orphan_keys: alloc::vec::Vec<usize> = map
                .iter()
                .filter(|(_, g)| g.owner == 0 && g.shared_to.is_none() && g.domain.is_none())
                .map(|(k, _)| *k)
                .collect();
            removed = orphan_keys.iter().filter_map(|k| map.remove(k)).collect();
        }
        removed
    }; // REG_GRANT_TABLE lock released

    for reg in &removed {
        if withhold_or_free(reg.base, grant_pages_for_size(reg.size)) {
            continue;
        }
        free_grant_pages(reg.base, grant_pages_for_size(reg.size));
    }
}

/// Monotonic boot-wide count of pages leaked by [`withhold_or_free`]. A delta
/// across a window counts every leak in that window, including concurrent
/// leaks attributed to other tasks: it can over-report, never under-report.
pub(crate) fn leaked_pages() -> usize {
    LEAKED_PAGES.load(core::sync::atomic::Ordering::Relaxed)
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
        let (base, pages) = (range.base, range.pages);
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
        let (base, pages) = (range.base, range.pages);
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
        Syscall::Reply { .. } => {
            let current_caller = crate::task::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .and_then(|task| task.current_caller);
            let Some(target) = current_caller else {
                return;
            };
            target
        }
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

/// 437-439 results: each refusal keeps its own `api::dma_export::ERR_*`
/// sentinel (CP1 §5, E21); an export id is at most `isize::MAX`.
#[cfg(not(target_arch = "riscv32"))]
pub(super) fn encode_dma_export_result(result: SyscallResult) -> usize {
    use api::dma_export as de;
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
    if !crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize) {
        return VfsGrantLookup::NotVfs;
    }
    match task.current_caller {
        Some(grant_owner) => VfsGrantLookup::Active(VfsGrantContext {
            grant_owner,
            grant_owner_cell_id: task.current_caller_cell_id,
            grant_owner_cell_generation: task.current_caller_cell_generation,
            request_generation: task.current_caller_request_generation,
            pending_revoke: crate::memory::pin::vfs_lease_pending_revoke(
                caller_id,
                grant_owner,
                task.current_caller_request_generation,
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
    pub(super) size_out: Option<*mut usize>,
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
        .map(|grant| grant.owner)
}

pub(super) fn reserve_dma_publication(
    tid: usize,
    quota: crate::memory::cell_quota::DmaQuotaReservation,
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
    Some(DmaPublicationGuard {
        slot,
        tid,
        quota: Some(quota),
    })
}

pub(super) fn shm_register(
    frame: usize,
    owner: crate::cell::state_stash::Incarnation,
    size: usize,
    recipient: Option<u64>,
    rights: u32,
) {
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    SHM_HANDLES.lock().insert(
        frame,
        ShmRecord {
            owner,
            recipient,
            size,
            rights,
        },
    );
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

pub(super) fn page_grant_authorizes_dma(
    grant: &PageGrant,
    caller_id: usize,
    caller_binding: (CellId, u64),
    dma_base: usize,
    dma_size: usize,
) -> bool {
    grant.owner == caller_id
        && (grant.owner_cell, grant.owner_generation) == caller_binding
        && dma_range_within(grant.base, grant.size, dma_base, dma_size)
}

pub(super) fn reg_grant_authorizes_dma(
    grant: &RegGrant,
    caller_id: usize,
    caller_binding: (CellId, u64),
    dma_base: usize,
    dma_size: usize,
) -> bool {
    grant.owner == caller_id
        && (grant.owner_cell, grant.owner_generation) == caller_binding
        && dma_range_within(grant.base, grant.size, dma_base, dma_size)
}

/// Reserve one already-authorized DMA publication while the caller still holds
/// the owning grant table and scheduler locks. The fixed-slot lifecycle marker
/// allocates no memory under `SCHEDULER`; pin publication remains protected by
/// the grant lock.
pub(super) fn reserve_authorized_dma_publication(
    caller_id: usize,
    caller_cell: CellId,
    bdf: u32,
    base: usize,
    size: usize,
) -> Result<DmaPublicationGuard, DmaGrantError> {
    if crate::resource_registry::owner_of_bdf(bdf) != Some(caller_id) {
        return Err(DmaGrantError::BdfNotOwned);
    }
    #[cfg(target_arch = "x86_64")]
    if let Some(dev) = crate::dev::pcie_ecam::device(bdf) {
        if !crate::dev::pcie_ecam::memory_bars_assigned(&dev.bars) {
            return Err(DmaGrantError::BdfNotOwned);
        }
    }
    let quota = crate::memory::cell_quota::try_reserve_dma(caller_cell.0 as usize, size)
        .ok_or(DmaGrantError::QuotaExceeded)?;
    let publication =
        reserve_dma_publication(caller_id, quota).ok_or(DmaGrantError::PublicationBusy)?;
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
            grants.values().any(|grant| {
                page_grant_authorizes_dma(grant, caller_id, caller_binding, base, size)
            })
        });
        if authorized {
            return reserve_authorized_dma_publication(
                caller_id,
                caller_binding.0,
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
                .any(|grant| reg_grant_authorizes_dma(grant, caller_id, caller_binding, base, size))
        });
        if authorized {
            return reserve_authorized_dma_publication(
                caller_id,
                caller_binding.0,
                bdf,
                base,
                size,
            );
        }
    }

    Err(DmaGrantError::NotOwned)
}

/// Allocate `n_pages` contiguous physical frames, map them USER RW, and zero them.
///
/// Returns the physical base address on success, or `None` on OOM or partial map.
/// Lock order: FRAME_ALLOCATOR (alloc) → FRAME_ALLOCATOR (map_page) → release →
///             FRAME_ALLOCATOR (partial-failure dealloc).
pub(super) fn alloc_grant_pages(n_pages: usize) -> Option<usize> {
    use crate::memory::frame::FRAME_ALLOCATOR;
    use crate::memory::paging::Flags;
    const PAGE_SIZE: usize = 4096;

    let user_flags =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;

    let paddr = {
        let mut g = FRAME_ALLOCATOR.lock();
        g.as_mut().and_then(|a| a.allocate_contiguous(n_pages))?
    };

    let mut mapped = 0usize;
    {
        let mut guard = FRAME_ALLOCATOR.lock();
        if let Some(alloc) = guard.as_mut() {
            for i in 0..n_pages {
                let v = paddr + i * PAGE_SIZE;
                if crate::memory::paging::map_page(alloc, v, v, Flags::from_bits(user_flags))
                    .is_ok()
                {
                    mapped += 1;
                } else {
                    break;
                }
            }
        }
    }

    if mapped < n_pages {
        // Partial map: restore the kernel identity mapping for the pages we
        // re-mapped USER (NOT a bare unmap — every Usable frame must stay
        // identity-mapped for the loader's identity-address zeroing), then free.
        // Those pages were briefly USER, so a Sas copy may have probed them:
        // unpublish (unmap, flush, drain copies) before the kernel restore.
        crate::task::stack::unpublish_user_frames(paddr, mapped);
        let kernel_rwx = Flags::VALID
            | Flags::READ
            | Flags::WRITE
            | Flags::EXECUTE
            | Flags::ACCESSED
            | Flags::DIRTY;
        let mut fa = FRAME_ALLOCATOR.lock();
        if let Some(a) = fa.as_mut() {
            for i in 0..mapped {
                let f = paddr + i * PAGE_SIZE;
                let _ = crate::memory::paging::map_page(a, f, f, Flags::from_bits(kernel_rwx));
            }
            for k in 0..n_pages {
                a.deallocate_frame(paddr + k * PAGE_SIZE);
            }
        }
        crate::memory::paging::tlb_flush_all();
        return None;
    }

    // Zero every mapped page before handing to user: prevents stale data from a
    // previously-freed grant leaking to a different cell (info-disclosure under G2).
    // SAFETY: frames are identity-mapped USER RW; SUM=1 allows S-mode writes.
    unsafe {
        core::ptr::write_bytes(paddr as *mut u8, 0, n_pages * PAGE_SIZE);
    }

    Some(paddr)
}

pub(super) fn free_grant_pages_inner(
    base: usize,
    n_pages: usize,
    #[cfg(feature = "p01-trace")] range: Option<crate::memory::pin::ReleasedRange>,
) {
    use crate::memory::frame::FRAME_ALLOCATOR;
    use crate::memory::paging::Flags;
    const PAGE_SIZE: usize = 4096;
    let kernel_rwx = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );

    crate::task::stack::unpublish_user_frames(base, n_pages);
    #[cfg(feature = "p01-trace")]
    let _range_write = crate::p01_retained::release_write();
    #[cfg(feature = "p01-trace")]
    let mut released = false;
    let mut fa = FRAME_ALLOCATOR.lock();
    if let Some(alloc) = fa.as_mut() {
        #[cfg(feature = "p01-trace")]
        {
            released = true;
        }
        for i in 0..n_pages {
            let f = base + i * PAGE_SIZE;
            let _ = crate::memory::paging::map_page(alloc, f, f, kernel_rwx);
            #[cfg(feature = "p01-trace")]
            {
                released &= alloc.p01_allocated(f);
            }
            alloc.deallocate_frame(f);
        }
    }
    crate::memory::paging::tlb_flush_all();
    #[cfg(feature = "p01-trace")]
    if let Some(range) = range {
        if released {
            crate::p01_retained::retire(range.p01_range);
            crate::p01_resources::finish(range.p01_obligation, 1);
        } else {
            crate::p01_retained::fault();
        }
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
    use crate::memory::{frame::FRAME_ALLOCATOR, paging::Flags};

    let claimed = FRAME_ALLOCATOR
        .lock()
        .as_mut()
        .is_some_and(|allocator| allocator.claim_exact_frame_for_test(base));
    if !claimed {
        return false;
    }
    let user_flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY,
    );
    let mapped = FRAME_ALLOCATOR.lock().as_mut().is_some_and(|allocator| {
        let _ = crate::memory::paging::unmap_page(base);
        crate::memory::paging::map_page(allocator, base, base, user_flags).is_ok()
    });
    if !mapped {
        free_grant_pages(base, 1);
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
        free_grant_pages(base, 1);
        return false;
    }
    let registered = {
        let mut table = reg_grant_table_lock().lock();
        match live_task_binding(caller_id) {
            Some((owner_cell, owner_generation))
                if !table
                    .as_ref()
                    .is_some_and(|grants| grants.contains_key(&base)) =>
            {
                if table.is_none() {
                    *table = Some(BTreeMap::new());
                }
                table.as_mut().is_some_and(|grants| {
                    #[cfg(feature = "p01-trace")]
                    let _semantic = crate::memory::heap::p01_owner::creation(
                        None,
                        crate::memory::heap::p01_owner::GRANT_SHARED,
                    );
                    grants
                        .insert(
                            base,
                            RegGrant {
                                base,
                                size: 128,
                                owner: caller_id,
                                owner_cell,
                                owner_generation,
                                shared_to: None,
                                domain: None,
                            },
                        )
                        .is_none()
                })
            }
            _ => false,
        }
    };
    if !registered {
        free_grant_pages(base, 1);
    }
    registered
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
        let (base, pages) = (range.base, range.pages);
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
    if g.owner != caller {
        return Err(SyscallError::PermissionDenied);
    }
    if len > g.size {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(g.base, len, MAX_USER_BUF)?;
    let mut snapshot = crate::loader::aligned_elf::ElfSnapshot::zeroed(len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    view.read_into(g.base, snapshot.bytes_mut())
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
    if !crate::fast_ipc::is_registered_vfs_cell(holder.cell_id.0 as usize)
        || holder.current_caller != Some(context.grant_owner)
        || holder.current_caller_cell_id != context.grant_owner_cell_id
        || holder.current_caller_cell_generation != context.grant_owner_cell_generation
        || holder.current_caller_request_generation != context.request_generation
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
    // is installed, returning the raw mapping and writing this slot are
    // infallible, so a failed GrantSlice cannot strand a pin.
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
                grant.owner,
                grant.shared_to.as_ref().map(|(tid, _)| *tid),
                grant.base,
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
        grant.owner,
        grant.shared_to.as_ref().map(|(tid, _)| *tid),
        grant.base,
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

    if !crate::memory::cell_quota::charge(cell as usize, bytes) {
        return Err(SyscallError::BufferTooSmall);
    }

    let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
    if let Some(allocator) = frame_guard.as_mut() {
        if let Some(frame) = allocator.allocate_frame() {
            drop(frame_guard);
            shm_register(frame, owner, bytes, None, 0x3);
            return Ok(frame);
        }
    }
    crate::memory::cell_quota::refund(cell as usize, bytes);
    Err(SyscallError::BufferTooSmall)
}

/// Handler for `Syscall::ShmMap` (extracted from the dispatcher).
pub(super) fn shm_map(caller_id: usize, handle: usize, target_pid: usize) -> SyscallResult {
    let Some((caller_cell, caller_gen)) = live_cell_identity(caller_id) else {
        return Err(SyscallError::PermissionDenied);
    };
    let Some(record) = shm_accessible_by(handle, caller_cell, caller_gen) else {
        return Err(SyscallError::PermissionDenied);
    };

    if target_pid != 0 && record.owner.cell == caller_cell {
        if let Some((target_cell, _)) = live_cell_identity(target_pid) {
            if let Some(r) = SHM_HANDLES.lock().get_mut(&handle) {
                r.recipient = Some(target_cell);
            }
        }
    }

    let frame = handle;
    let vaddr = frame; // Identity map for SAS simplicity

    use crate::memory::paging::Flags;
    let flags =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;

    let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
    if let Some(allocator) = frame_guard.as_mut() {
        if crate::memory::paging::map_page(allocator, vaddr, frame, Flags::from_bits(flags)).is_ok()
        {
            return Ok(vaddr);
        }
    }
    Err(SyscallError::Unknown)
}

/// Handler for `Syscall::Grant` (extracted from the dispatcher).
pub(super) fn grant(
    caller_id: usize,
    target: usize,
    ptr: usize,
    len: usize,
    flags: usize,
) -> SyscallResult {
    crate::task::ipc_grant(caller_id, target, ptr, len, flags as u32)
        .map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::Map` (extracted from the dispatcher).
pub(super) fn map(caller_id: usize, grant_id: usize) -> SyscallResult {
    crate::task::ipc_map(caller_id, grant_id).map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::Lend` (extracted from the dispatcher).
pub(super) fn lend(
    caller_id: usize,
    target: usize,
    ptr: usize,
    len: usize,
    flags: usize,
) -> SyscallResult {
    crate::task::ipc_lend(caller_id, target, ptr, len, flags as u32)
        .map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::BorrowRead` (extracted from the dispatcher).
pub(super) fn borrow_read(
    caller_id: usize,
    lease_id: usize,
    offset: usize,
    ptr: usize,
    len: usize,
) -> SyscallResult {
    crate::task::ipc_borrow_read(caller_id, lease_id, offset, ptr, len)
        .map_err(|_| SyscallError::PermissionDenied)
}

/// Handler for `Syscall::BorrowWrite` (extracted from the dispatcher).
pub(super) fn borrow_write(
    caller_id: usize,
    lease_id: usize,
    offset: usize,
    ptr: usize,
    len: usize,
) -> SyscallResult {
    crate::task::ipc_borrow_write(caller_id, lease_id, offset, ptr, len)
        .map_err(|_| SyscallError::PermissionDenied)
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
            return Ok(api::anon_memory::ERR_NOT_SUPPORTED);
        }
        // No anonymous page can exist yet (no budget source, CP1 D-M4
        // default 0 = `NotSupported`), so every valid op is refused:
        // a 0 from UNMAP or PROTECT would claim a release or a guard
        // page that never took effect.
        match api::anon_memory::decode_anon_request(op, addr, len, flags) {
            Ok(_) => Ok(api::anon_memory::ERR_NOT_SUPPORTED),
            Err(err) => Ok(err),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (caller_id, op, addr, len, flags);
        Ok(api::anon_memory::ERR_NOT_SUPPORTED)
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
    if out_len != api::dma_export::DMA_EXPORT_RECORD_LEN {
        return Err(SyscallError::InvalidInput);
    }
    caller_copy_view(caller_id)?
        .validate_writable(out_ptr, out_len)
        .map_err(|_| SyscallError::InvalidInput)?;
    // Same retirement barrier as GrantDma: the holder's root cannot
    // retire (and tear its domain down) mid-publication.
    let barrier = crate::memory::cell_quota::try_reserve_dma(0, 0).ok_or(SyscallError::Unknown)?;
    let publication = reserve_dma_publication(caller_id, barrier).ok_or(SyscallError::TryAgain)?;
    let record = dma_export::map(caller_id, export_id, bdf);
    drop(publication);
    let mut bytes = [0u8; api::dma_export::DMA_EXPORT_RECORD_LEN];
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
    let paddr = match &owner_space {
        #[cfg(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                all(target_arch = "aarch64", feature = "test-hooks")
            )
        ))]
        Some(_) => match alloc_grant_pages_supervisor_only(n_pages) {
            Some(paddr) => paddr,
            None => return Ok(0),
        },
        #[cfg(not(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                all(target_arch = "aarch64", feature = "test-hooks")
            )
        )))]
        Some(_) => return Ok(0),
        None => match alloc_grant_pages(n_pages) {
            Some(paddr) => paddr,
            None => return Ok(0),
        },
    };
    // A domain owner's backing is mapped only in its own root, RW+NX, and
    // stays supervisor-only in the SAS root. Every page is undone if any
    // one fails, so no raw pointer is ever published for a partial map.
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            all(target_arch = "aarch64", feature = "test-hooks")
        )
    ))]
    if let Some(space) = &owner_space {
        if let Err(error) = map_domain_owner_pages(space, paddr, n_pages) {
            log::warn!("[grant] GrantAlloc owner mapping failed for task {caller_id}: {error:?}");
            free_grant_pages(paddr, n_pages);
            return Ok(0);
        }
    }
    let domain = owner_space
        .as_ref()
        .and_then(|space| new_domain_record(space, paddr, size));
    let mut table = grant_table_lock().lock();
    let (owner_cell, owner_generation) = match live_task_binding(caller_id) {
        Some(binding) => binding,
        None => {
            drop(table);
            free_grant_pages(paddr, n_pages);
            return Err(SyscallError::PermissionDenied);
        }
    };
    if table.is_none() {
        *table = Some(BTreeMap::new());
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    table.as_mut().unwrap().insert(
        paddr,
        PageGrant {
            base: paddr,
            size,
            owner: caller_id,
            owner_cell,
            owner_generation,
            shared_to: None,
            domain,
        },
    );
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
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            all(target_arch = "aarch64", feature = "test-hooks")
        )
    ))]
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
        {
            let mut tbl = grant_table_lock().lock();
            if let Some(grant) = tbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
                return domain_grant_share_row(
                    grant.owner,
                    grant.base,
                    grant.domain.as_ref(),
                    &mut grant.shared_to,
                    caller_id,
                    target_cell,
                    &target_space,
                    perm,
                )
                .map(|()| 0);
            }
        }
        let mut rtbl = reg_grant_table_lock().lock();
        return match rtbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
            None => Err(SyscallError::InvalidInput),
            Some(grant) => domain_grant_share_row(
                grant.owner,
                grant.base,
                grant.domain.as_ref(),
                &mut grant.shared_to,
                caller_id,
                target_cell,
                &target_space,
                perm,
            )
            .map(|()| 0),
        };
    }
    // Without a private-root backend no task is a domain, so the residual
    // branch below is the SAS-only path.
    let _ = (owner_is_domain, target_is_domain);
    // Check PAGE_GRANT_TABLE first, then fall back to REG_GRANT_TABLE.
    {
        let mut tbl = grant_table_lock().lock();
        if tbl.is_none() {
            *tbl = Some(BTreeMap::new());
        }
        if let Some(grant) = tbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
            if grant.owner != caller_id {
                return Err(SyscallError::PermissionDenied);
            }
            grant.shared_to = Some((target_cell, perm));
            return Ok(0);
        }
    }
    // Not in PAGE_GRANT_TABLE — try REG_GRANT_TABLE.
    let mut rtbl = reg_grant_table_lock().lock();
    match rtbl.as_mut().and_then(|m| m.get_mut(&grant_id)) {
        None => Err(SyscallError::InvalidInput),
        Some(grant) if grant.owner != caller_id => Err(SyscallError::PermissionDenied),
        Some(grant) => {
            grant.shared_to = Some((target_cell, perm));
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
    if flags & !api::syscall::grant_slice::KNOWN_FLAGS != 0 {
        return Ok(GRANT_DENY_SLICE);
    }
    let lease = flags & api::syscall::grant_slice::LEASE != 0;
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
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            all(target_arch = "aarch64", feature = "test-hooks")
        )
    ))]
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
    // Owner-only, and only while no in-flight operation holds the region.
    // The pin check runs inside the table lock (order: PAGE_GRANT_TABLE →
    // pin REGISTRY, a leaf), so a concurrent VFS GrantSlice or GrantDma
    // cannot publish a pin between the check and removal.
    let entry = {
        let mut tbl = grant_table_lock().lock();
        let owned = tbl
            .as_ref()
            .and_then(|m| m.get(&grant_id))
            .filter(|g| g.owner == caller_id)
            .map(|g| (g.base, g.size, clone_domain_record(&g.domain)));
        match owned {
            None => None,
            Some((base, size, domain)) => {
                refuse_if_pinned("GrantFree", grant_id, base, size)?;
                if domain.is_some() {
                    // The row stays while the revoke drains so a retry is
                    // idempotent; a `Revoking` row refuses new slices.
                    Some((base, size, domain))
                } else {
                    tbl.as_mut()
                        .and_then(|m| m.remove(&grant_id))
                        .map(|g| (g.base, g.size, None))
                }
            }
        }
    };
    let Some((base, size, domain)) = entry else {
        return Err(SyscallError::PermissionDenied);
    };
    if let Some(record) = &domain {
        if !revoke_domain_record(record, grant_id, "GrantFree") {
            // Bounded existing error: the frames stay retained and the
            // record stays `Revoking` for an idempotent retry.
            return Err(SyscallError::PermissionDenied);
        }
        let mut tbl = grant_table_lock().lock();
        tbl.as_mut().and_then(|m| m.remove(&grant_id));
    }
    free_grant_pages(base, grant_pages_for_size(size));
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
        if grant.owner != caller_id {
            return Err(SyscallError::PermissionDenied);
        }
        if len == 0 || offset > grant.size || len > grant.size.saturating_sub(offset) {
            return Err(SyscallError::InvalidInput);
        }
        let base = grant
            .base
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
    let paddr = match &owner_space {
        #[cfg(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                all(target_arch = "aarch64", feature = "test-hooks")
            )
        ))]
        Some(_) => match alloc_grant_pages_supervisor_only(n_pages) {
            Some(paddr) => paddr,
            None => return Ok(0),
        },
        #[cfg(not(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                all(target_arch = "aarch64", feature = "test-hooks")
            )
        )))]
        Some(_) => return Ok(0),
        None => match alloc_grant_pages(n_pages) {
            Some(paddr) => paddr,
            None => return Ok(0),
        },
    };
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            all(target_arch = "aarch64", feature = "test-hooks")
        )
    ))]
    if let Some(space) = &owner_space {
        if let Err(error) = map_domain_owner_pages(space, paddr, n_pages) {
            log::warn!(
                "[grant] GrantRegister owner mapping failed for task {caller_id}: {error:?}"
            );
            free_grant_pages(paddr, n_pages);
            return Ok(0);
        }
    }
    let domain = owner_space
        .as_ref()
        .and_then(|space| new_domain_record(space, paddr, size));
    let mut table = reg_grant_table_lock().lock();
    let (owner_cell, owner_generation) = match live_task_binding(caller_id) {
        Some(binding) => binding,
        None => {
            drop(table);
            free_grant_pages(paddr, n_pages);
            return Err(SyscallError::PermissionDenied);
        }
    };
    if table.is_none() {
        *table = Some(BTreeMap::new());
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    table.as_mut().unwrap().insert(
        paddr,
        RegGrant {
            base: paddr,
            size,
            owner: caller_id,
            owner_cell,
            owner_generation,
            shared_to: None,
            domain,
        },
    );
    Ok(paddr)
}

/// Handler for `Syscall::GrantUnregister` (extracted from the dispatcher).
pub(super) fn grant_unregister(caller_id: usize, reg_id: usize) -> SyscallResult {
    unregister_registered_grant(caller_id, reg_id)?;
    Ok(0)
}
