//! IPC System Calls (Inspired by Tock OS)
//!
//! This module defines the interface between "Cells/Silos" and the Kernel.
//! See [docs/architecture/03-driver-strategy.md] for the full rationale.

use super::tcb::TaskState;
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use alloc::string::String;
/// Only the private-root lifecycle helpers below name `Arc`; every other
/// target/feature combination has nothing to import.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
use alloc::sync::Arc;
use alloc::vec::Vec;
use api::syscall::ViSpawnArgs;
// use log::info;
use super::copy_glue::TaskCopyView;
use types::*;

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_ARMED_CALLER: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_ENTERED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_PROBED_BUSY: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_NO_EARLY_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static GETRANDOM_RACE_PROBE_TIMED_OUT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

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

static SHM_HANDLES: Spinlock<BTreeMap<usize, ShmRecord>> = Spinlock::new(BTreeMap::new());

const MAX_IN_FLIGHT_DMA_PUBLICATIONS: usize = 64;
static DMA_PUBLICATIONS: Spinlock<[usize; MAX_IN_FLIGHT_DMA_PUBLICATIONS]> =
    Spinlock::new([0; MAX_IN_FLIGHT_DMA_PUBLICATIONS]);

pub(crate) struct DmaPublicationGuard {
    slot: usize,
    tid: usize,
    quota: Option<crate::memory::cell_quota::DmaQuotaReservation>,
}

impl DmaPublicationGuard {
    fn commit(mut self) {
        if let Some(quota) = self.quota.take() {
            quota.commit();
        }
    }
}

impl Drop for DmaPublicationGuard {
    fn drop(&mut self) {
        // Roll quota back before making retirement eligible to proceed.
        drop(self.quota.take());
        let mut publications = DMA_PUBLICATIONS.lock();
        debug_assert_eq!(publications[self.slot], self.tid);
        publications[self.slot] = 0;
    }
}

fn reserve_dma_publication(
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

pub(crate) fn dma_publication_in_flight(tid: usize) -> bool {
    DMA_PUBLICATIONS.lock().contains(&tid)
}

#[cfg(test)]
pub(crate) fn test_reserve_dma_publication(tid: usize) -> Option<DmaPublicationGuard> {
    let quota = crate::memory::cell_quota::try_reserve_dma(0, 0)?;
    reserve_dma_publication(tid, quota)
}

fn shm_register(
    frame: usize,
    owner: crate::cell::state_stash::Incarnation,
    size: usize,
    recipient: Option<u64>,
    rights: u32,
) {
    #[cfg(cellos_p01_trace)]
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

#[cfg_attr(not(test), allow(dead_code))]
/// Whether `handle` is an outstanding `ShmAlloc` frame owned by exactly `owner`
/// (same Cell and generation).
fn shm_owned_by(handle: usize, owner: crate::cell::state_stash::Incarnation) -> bool {
    SHM_HANDLES.lock().get(&handle).map(|r| r.owner) == Some(owner)
}

fn shm_accessible_by(handle: usize, cell: u64, generation: u64) -> Option<ShmRecord> {
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
fn shm_take_for(owner: crate::cell::state_stash::Incarnation) -> (Vec<usize>, usize) {
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
        super::stack::unpublish_user_frames(frame, 1);
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

// ADR-0033 CP1 §5-§6: client-memory DMA export leases (437/438/439). A child
// module so its ownership proof reads the grant tables under their own locks.
#[path = "dma_export.rs"]
pub(crate) mod dma_export;

// ── Zero-Copy Grant Table ─────────────────────────────────────────────────────

/// Kernel-managed zero-copy memory region.
///
/// Distinct from `tcb::GrantEntry`, which tracks per-task grants from the kernel.
/// The issuing task identity remains necessary for the existing grant ABI, but
/// the Cell/generation binding prevents a stale task record from becoming a
/// writable-output authority after retirement or transfer.
struct PageGrant {
    base: usize,
    size: usize,
    owner: usize,
    // Read by the RV64 SAS ownership ledger; retained in every build so a
    // grant cannot change its security identity across target tuples.
    #[allow(dead_code)]
    owner_cell: CellId,
    #[allow(dead_code)]
    owner_generation: u64,
    shared_to: Option<(usize, GrantPerm)>,
    /// One kernel-owned lifecycle record, present only while the owner is a
    /// private-root domain. It carries the owner/receiver roots, the exact
    /// receiver mapping tuple and the `Live → Revoking → Revoked` state; a SAS
    /// owner keeps today's zero-copy path and leaves this `None`.
    domain: Option<DomainRecord>,
}

static PAGE_GRANT_TABLE: Spinlock<Option<BTreeMap<usize, PageGrant>>> = Spinlock::new(None);

fn grant_table_lock() -> &'static Spinlock<Option<BTreeMap<usize, PageGrant>>> {
    &PAGE_GRANT_TABLE
}

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

/// Maximum pages in a single GrantAlloc or GrantRegister call (16 MiB ceiling).
/// Acts as a safety cap; cells are further bounded by available physical frames.
const MAX_GRANT_PAGES: usize = 4096;

fn grant_pages_for_size(size: usize) -> usize {
    size.div_ceil(4096)
}

fn grant_allocated_bytes(size: usize) -> usize {
    grant_pages_for_size(size) * 4096
}

#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn task_domain_space(
    tid: usize,
) -> Option<alloc::sync::Arc<crate::memory::address_space::AddressSpace>> {
    let guard = super::SCHEDULER.lock();
    guard.as_ref().and_then(|sched| {
        sched
            .tasks
            .get(&tid)
            .and_then(|task| match &task.address_space {
                super::tcb::TaskAddressSpace::Domain(space) => Some(alloc::sync::Arc::clone(space)),
                super::tcb::TaskAddressSpace::Sas => None,
            })
    })
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
fn domain_grant_task(tid: usize) -> bool {
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
fn domain_grant_task(_tid: usize) -> bool {
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
const fn domain_grant_lifecycle_supported() -> bool {
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
const GRANT_DENY_ALLOC: usize = 0;
const GRANT_DENY_SLICE: usize = usize::MAX;
const GRANT_DENY_ERROR: SyscallError = SyscallError::PermissionDenied;

/// The phase-03 AArch64 reopen is confined to test images, and the shapes that
/// stay denied keep the phase-01 sentinels byte-for-byte.
///
/// A production AArch64 build (no `test-hooks`) has no domain grant lifecycle,
/// so [`domain_grant_capable`] is always `false` and every domain shape takes
/// the sentinel branch ahead of the lifecycle: `Ok(0)` for allocation, `usize::MAX`
/// for a slice, `Err(PermissionDenied)` for free/unregister. Const-evaluated, so
/// a build that widened the cfg by accident fails to compile rather than booting
/// a domain grant path.
///
/// `GrantShare` has no sentinel branch — its gate lives in the lifecycle-only
/// block, so on a closed target it is reachable only for a domain task, and no
/// domain can be admitted on a closed target (`switch_ordering_qualified`).
#[cfg(all(
    feature = "native-domains",
    target_arch = "aarch64",
    not(feature = "test-hooks")
))]
const _: () = {
    assert!(!domain_grant_lifecycle_supported());
    assert!(GRANT_DENY_ALLOC == 0);
    assert!(GRANT_DENY_SLICE == usize::MAX);
    assert!(matches!(GRANT_DENY_ERROR, SyscallError::PermissionDenied));
};

/// A private root, on the targets that have one.
///
/// The `memory::address_space` module exists only behind `native-domains` on a
/// private-root target, so the helpers below name this alias and every disabled
/// variant stays compilable without the feature.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
type DomainRoot = alloc::sync::Arc<crate::memory::address_space::AddressSpace>;
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
)))]
type DomainRoot = ();

/// The live private root of `tid`, if it is a domain right now.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn live_domain_space(tid: usize) -> Option<DomainRoot> {
    task_domain_space(tid).filter(|space| space.is_live())
}

#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
)))]
fn live_domain_space(_tid: usize) -> Option<DomainRoot> {
    None
}

/// The phase-01 gate as a capability check: may `tid` take part in the
/// private-root grant lifecycle?
///
/// Exactly one shape passes — a live task whose address space is a private root
/// that is still `Live`, on the one architecture whose lifecycle is
/// implemented. Every other shape keeps the denial sentinel: a non-RV64 target,
/// an unknown or retired tid, and a root already marked dying.
fn domain_grant_capable(tid: usize) -> bool {
    domain_grant_lifecycle_supported() && live_domain_space(tid).is_some()
}

/// Map a domain owner's freshly allocated backing into its own root, RW+NX,
/// undoing every page already mapped if one fails.
///
/// The frames stay supervisor-only in the SAS/global root: this path never
/// publishes USER access there, so a SAS cell cannot reach a domain owner's
/// backing even if the grant later collapses.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn map_domain_owner_pages(
    space: &DomainRoot,
    base: usize,
    n_pages: usize,
) -> Result<(), crate::memory::address_space::AddressSpaceError> {
    const PAGE_SIZE: usize = 4096;
    let rights = crate::memory::paging::Flags::from_bits(
        crate::memory::paging::Flags::READ | crate::memory::paging::Flags::WRITE,
    );
    let mut mapped = 0usize;
    for index in 0..n_pages {
        let va = base + index * PAGE_SIZE;
        match space.map_grant_page(va, va, rights) {
            Ok(()) => {
                // The owner's own hart writes this PTE and then uses the
                // pointer, so it must invalidate the page locally first.
                crate::hal::paging::flush_tlb_page(va);
                mapped += 1;
            }
            Err(error) => {
                for undo in 0..mapped {
                    let _ = space.unmap_grant_page(base + undo * PAGE_SIZE);
                }
                // The unmaps above queued this tag's invalidation if it could not
                // be confirmed; the deferred-release reaper completes it, so the
                // rollback only re-probes once instead of waiting a budget.
                let _ = crate::memory::tlb_shootdown::confirm_tag_invalidation(space.asid());
                return Err(error);
            }
        }
    }
    Ok(())
}

/// Allocate `n_pages` contiguous frames, zero them through the supervisor
/// identity map, and leave the SAS/global root untouched.
///
/// Domain owners must not have their backing published USER in the shared root
/// (the phase-01 leak). The boot identity map already covers every usable
/// frame supervisor RWX, so zeroing needs no remap at all; the caller then maps
/// the owner's own private root over the returned base.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn alloc_grant_pages_supervisor_only(n_pages: usize) -> Option<usize> {
    use crate::memory::frame::FRAME_ALLOCATOR;
    const PAGE_SIZE: usize = 4096;
    let paddr = {
        let mut guard = FRAME_ALLOCATOR.lock();
        guard
            .as_mut()
            .and_then(|alloc| alloc.allocate_contiguous(n_pages))?
    };
    // SAFETY: every usable frame is identity-mapped supervisor RWX in the
    // current (kernel) root, and the syscall runs on that root.
    unsafe {
        core::ptr::write_bytes(paddr as *mut u8, 0, n_pages * PAGE_SIZE);
    }
    Some(paddr)
}

/// The lifecycle record behind a table row, where this target can hold one.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
type DomainRecord = Arc<super::domain_grant::DomainGrant>;

/// On every other target a row can never carry a lifecycle record:
/// [`clone_domain_record`] is the only producer and always yields `None`, so the
/// stubs below are unreachable rather than a second implementation.
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
type DomainRecord = ();

/// Clone the lifecycle record a row carries, if any.
///
/// The lifecycle exists on one target tuple only; on every other target the
/// record type has no inhabitants, so this is the sole producer of `None`.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn clone_domain_record(domain: &Option<DomainRecord>) -> Option<DomainRecord> {
    domain.clone()
}
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
fn clone_domain_record(_domain: &Option<DomainRecord>) -> Option<DomainRecord> {
    None
}

/// Build the lifecycle record for a domain owner that has just published its
/// own mapping. Absent — `None` — wherever the lifecycle does not exist.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn new_domain_record(space: &DomainRoot, base: usize, size: usize) -> Option<DomainRecord> {
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::GRANT_SHARED,
    );
    Some(Arc::new(super::domain_grant::DomainGrant::new(
        space, base, size,
    )))
}
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
fn new_domain_record(_space: &DomainRoot, _base: usize, _size: usize) -> Option<DomainRecord> {
    None
}

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
fn revoke_domain_record(record: &DomainRecord, id: usize, kind: &str) -> bool {
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
fn revoke_domain_record(_record: &DomainRecord, _id: usize, _kind: &str) -> bool {
    false
}

/// Drop the receiver half of a record left unshared by receiver death.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn drain_domain_receiver(record: &DomainRecord) {
    if record.drain_receiver().is_err() {
        log::warn!(
            "[grant] receiver exit: domain receiver PTE invalidation unacknowledged; record keeps \
             the receiver mapping until the next share or teardown retries"
        );
    }
}
#[cfg(not(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
)))]
fn drain_domain_receiver(_record: &DomainRecord) {}

/// A record the lifecycle cannot drain: it names a private-root endpoint but is
/// not carried by a managed domain record. Pre-gate residue, or a target where
/// the lifecycle is not implemented at all.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn residual_domain_record(
    owner: usize,
    shared_to: Option<(usize, GrantPerm)>,
    managed: bool,
) -> bool {
    if managed && domain_grant_lifecycle_supported() {
        return false;
    }
    domain_grant_task(owner) || shared_to.is_some_and(|(tid, _)| domain_grant_task(tid))
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
enum DmaGrantError {
    NotOwned,
    BdfNotOwned,
    QuotaExceeded,
    Pin(crate::memory::pin::PinError),
    PublicationBusy,
}

fn dma_range_within(
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

fn page_grant_authorizes_dma(
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

fn reg_grant_authorizes_dma(
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
fn reserve_authorized_dma_publication(
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
    if let Some(dev) = crate::task::drivers::pcie_ecam::device(bdf) {
        if !crate::task::drivers::pcie_ecam::memory_bars_assigned(&dev.bars) {
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
fn reserve_caller_owned_dma_range(
    caller_id: usize,
    bdf: u32,
    base: usize,
    size: usize,
) -> Result<DmaPublicationGuard, DmaGrantError> {
    {
        let page_grants = grant_table_lock().lock();
        let scheduler = super::SCHEDULER.lock();
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
        let scheduler = super::SCHEDULER.lock();
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

// ── Registered Grant Table (GrantRegister / GrantUnregister, syscalls 215/216) ──

/// Persistent kernel-managed Grant buffer for a cell's lifetime.
///
/// Supports one grantee at a time via `GrantShare`/`GrantSlice` (same as
/// `PageGrant`). `owner == 0` denotes an owner-dead VFS handoff awaiting reap.
struct RegGrant {
    base: usize,
    size: usize,
    owner: usize,
    owner_cell: CellId,
    owner_generation: u64,
    shared_to: Option<(usize, GrantPerm)>,
    /// See [`PageGrant::domain`]: the same single lifecycle record behind the
    /// registered-grant table.
    domain: Option<DomainRecord>,
}

static REG_GRANT_TABLE: Spinlock<Option<BTreeMap<usize, RegGrant>>> = Spinlock::new(None);

fn reg_grant_table_lock() -> &'static Spinlock<Option<BTreeMap<usize, RegGrant>>> {
    &REG_GRANT_TABLE
}

// ── Shared allocation/deallocation helpers ────────────────────────────────────

/// Allocate `n_pages` contiguous physical frames, map them USER RW, and zero them.
///
/// Returns the physical base address on success, or `None` on OOM or partial map.
/// Lock order: FRAME_ALLOCATOR (alloc) → FRAME_ALLOCATOR (map_page) → release →
///             FRAME_ALLOCATOR (partial-failure dealloc).
fn alloc_grant_pages(n_pages: usize) -> Option<usize> {
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
        super::stack::unpublish_user_frames(paddr, mapped);
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
fn free_grant_pages(base: usize, n_pages: usize) {
    free_grant_pages_inner(
        base,
        n_pages,
        #[cfg(cellos_p01_trace)]
        None,
    );
}

fn free_released_range(range: crate::memory::pin::ReleasedRange) {
    free_grant_pages_inner(
        range.base,
        range.pages,
        #[cfg(cellos_p01_trace)]
        Some(range),
    );
}

fn free_grant_pages_inner(
    base: usize,
    n_pages: usize,
    #[cfg(cellos_p01_trace)] range: Option<crate::memory::pin::ReleasedRange>,
) {
    use crate::memory::frame::FRAME_ALLOCATOR;
    use crate::memory::paging::Flags;
    const PAGE_SIZE: usize = 4096;
    let kernel_rwx = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );

    super::stack::unpublish_user_frames(base, n_pages);
    #[cfg(cellos_p01_trace)]
    let _range_write = crate::p01_retained::release_write();
    #[cfg(cellos_p01_trace)]
    let mut released = false;
    let mut fa = FRAME_ALLOCATOR.lock();
    if let Some(alloc) = fa.as_mut() {
        #[cfg(cellos_p01_trace)]
        {
            released = true;
        }
        for i in 0..n_pages {
            let f = base + i * PAGE_SIZE;
            let _ = crate::memory::paging::map_page(alloc, f, f, kernel_rwx);
            #[cfg(cellos_p01_trace)]
            {
                released &= alloc.p01_allocated(f);
            }
            alloc.deallocate_frame(f);
        }
    }
    crate::memory::paging::tlb_flush_all();
    #[cfg(cellos_p01_trace)]
    if let Some(range) = range {
        if released {
            crate::p01_retained::retire(range.p01_range);
            crate::p01_resources::finish(range.p01_obligation, 1);
        } else {
            crate::p01_retained::fault();
        }
    }
}

/// Refuse an owner-initiated teardown of `[base, base + size)` while an
/// in-flight asynchronous operation still holds any part of it.
///
/// `kind` and `id` name the request in the log; the frames must not go back to
/// the allocator while a device or the kernel can still write them.
///
/// # Errors
/// [`SyscallError::PermissionDenied`] when the region is pinned.
fn refuse_if_pinned(kind: &str, id: usize, base: usize, size: usize) -> Result<(), SyscallError> {
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
fn unregister_registered_grant(caller_id: usize, reg_id: usize) -> Result<(), SyscallError> {
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
                    #[cfg(cellos_p01_trace)]
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

// ── Grant Reaper ──────────────────────────────────────────────────────────────

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
fn clear_grantee_refs(tid: usize) -> (usize, usize) {
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

/// Complete one owner-reclaimed domain row: revoke, then either release the
/// frames or leave an ownerless `Revoking` row for [`sweep_deferred_domain_grants`].
///
/// An unacknowledged invalidation never frees a frame. The row loses its owner
/// so no caller can free it out from under the retry.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn release_deferred_domain_row(
    key: usize,
    record: DomainRecord,
    base: usize,
    size: usize,
    registered: bool,
) {
    let pages = grant_pages_for_size(size);
    if revoke_domain_record(&record, base, "task-exit") {
        if registered {
            let mut tbl = reg_grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        } else {
            let mut tbl = grant_table_lock().lock();
            tbl.as_mut().and_then(|map| map.remove(&key));
        }
        if !withhold_or_free(base, pages) {
            free_grant_pages(base, pages);
        }
        return;
    }
    if registered {
        let mut tbl = reg_grant_table_lock().lock();
        if let Some(grant) = tbl.as_mut().and_then(|map| map.get_mut(&key)) {
            grant.owner = 0;
            grant.owner_cell = CellId(0);
            grant.owner_generation = 0;
            grant.shared_to = None;
        }
    } else {
        let mut tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_mut().and_then(|map| map.get_mut(&key)) {
            grant.owner = 0;
            grant.owner_cell = CellId(0);
            grant.owner_generation = 0;
            grant.shared_to = None;
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
fn release_deferred_domain_row(
    _key: usize,
    _record: DomainRecord,
    _base: usize,
    _size: usize,
    _registered: bool,
) {
}

/// Retry owner-reclaimed domain rows whose invalidation was unacknowledged.
///
/// Every task exit runs this for both tables. Completing the revoke is what
/// finally releases the retained frames; until then they stay quarantined with
/// the record so nothing reuses memory a stale translation could still reach.
fn sweep_deferred_domain_grants() {
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
fn sweep_orphan_reg_grants() {
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

/// Whether the reaper must withhold `pages` frames at `base` instead of freeing
/// them, because an in-flight operation still holds part of the region.
///
/// Pin lookup and the quarantine transfer are one registry transaction. This
/// is essential for VFS leases: if the holder completes the exact lease before
/// the transaction, the reaper must free these frames rather than install an
/// orphaned release key; if the transfer wins, that exact release owns it.
///
/// Returns `true` when the frames must not be freed. A full quarantine also
/// returns `true`: leaking the frames is the only safe answer left, and the
/// leak is counted in [`leaked_pages`].
fn withhold_or_free(base: usize, pages: usize) -> bool {
    match crate::memory::pin::withhold_pinned_frames(base, pages) {
        crate::memory::pin::FrameTransfer::Free => false,
        crate::memory::pin::FrameTransfer::Withheld => true,
        crate::memory::pin::FrameTransfer::Full => {
            LEAKED_PAGES.fetch_add(pages, core::sync::atomic::Ordering::Relaxed);
            log::error!(
                "[grant] quarantine full: leaking {pages} page(s) at {base:#x} protected by a live pin"
            );
            true
        }
    }
}

/// Pages leaked for the life of the boot because the quarantine was full.
static LEAKED_PAGES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

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

/// Free owner-dead frames whose last `GrantSlice` lease `holder` just ended.
///
/// Must run with no grant-table, scheduler or pin lock held: the frames go
/// back through `FRAME_ALLOCATOR` and `KERNEL_ROOT`.
fn free_released_grant_lease_frames(
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

/// End the caller's `GrantSlice` leases on Grants owned by the task a send
/// form is addressed to (ABI a3 `LEASE`, approved 2026-11-10).
///
/// Runs before the send takes effect, so the owner can never act on the
/// message while the lease still blocks its `GrantFree`. `Reply` is addressed
/// to the caller's current IPC caller, not to its a0. `IpcReply` resolves its
/// destination from an async token and is not a boundary; the holder's next
/// receive still ends the lease.
fn end_grant_leases_at_send(caller_id: usize, syscall: &Syscall) {
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
            let current_caller = super::SCHEDULER
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
pub(crate) fn release_vfs_context_lease(release: super::scheduler::VfsLeaseRelease) {
    for range in crate::memory::pin::release_vfs_lease(
        release.holder_tid,
        release.grant_owner,
        release.request_generation,
    ) {
        free_released_range(range);
    }
}

/// Result of a System Call
pub type SyscallResult = core::result::Result<usize, SyscallError>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SyscallError {
    /// The other end of a stream is gone: a pipe write with no reader left.
    BrokenPipe,
    InvalidDriverId,
    InvalidCommand,
    BufferTooSmall,
    PermissionDenied,
    FileNotFound,
    TryAgain,
    Unknown,
    NotSupported,
    InvalidInput,
    OutOfMemory,
}

fn ipc_error(error: super::async_ipc::Failure) -> SyscallError {
    match error {
        super::async_ipc::Failure::Busy => SyscallError::TryAgain,
        super::async_ipc::Failure::PeerGone => SyscallError::FileNotFound,
        super::async_ipc::Failure::Invalid => SyscallError::InvalidInput,
        super::async_ipc::Failure::TooSmall => SyscallError::BufferTooSmall,
    }
}

fn encode_ipc_result(result: SyscallResult) -> usize {
    match result {
        Ok(value) => value,
        Err(SyscallError::TryAgain) => usize::MAX - 1,
        Err(SyscallError::FileNotFound) => usize::MAX - 2,
        Err(SyscallError::InvalidInput | SyscallError::InvalidCommand) => usize::MAX - 3,
        Err(SyscallError::BufferTooSmall) => usize::MAX - 4,
        Err(_) => usize::MAX,
    }
}

/// 434/435 results: the typed sentinels of [`encode_ipc_result`] plus
/// `NotSupported` and `OutOfMemory` (vectors exhausted), which a driver must
/// tell apart to fall back to polling (`api::pcie_function::ERR_*`).
#[cfg(not(target_arch = "riscv32"))]
fn encode_pcie_function_result(result: SyscallResult) -> usize {
    match result {
        Err(SyscallError::NotSupported) => api::pcie_function::ERR_NOT_SUPPORTED,
        Err(SyscallError::OutOfMemory) => api::pcie_function::ERR_NO_VECTORS,
        other => encode_ipc_result(other),
    }
}

#[cfg(not(target_arch = "riscv32"))]
fn is_pcie_function_call(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::QueryPcieFunction { .. } | Syscall::AllocPcieIrq { .. }
    )
}

/// 436 results: each refusal keeps its own `api::syscall::system_power`
/// sentinel. Success never returns to the caller.
#[cfg(not(target_arch = "riscv32"))]
fn encode_system_power_result(result: SyscallResult) -> usize {
    use api::syscall::system_power as sp;
    match result {
        Ok(value) => value,
        Err(SyscallError::InvalidInput) => sp::ERR_INVALID_INPUT,
        Err(SyscallError::NotSupported) => sp::ERR_NOT_SUPPORTED,
        Err(_) => sp::ERR_PERMISSION_DENIED,
    }
}

/// 437-439 results: each refusal keeps its own `api::dma_export::ERR_*`
/// sentinel (CP1 §5, E21); an export id is at most `isize::MAX`.
#[cfg(not(target_arch = "riscv32"))]
fn encode_dma_export_result(result: SyscallResult) -> usize {
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
fn is_dma_export_call(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::ExportDmaRange { .. }
            | Syscall::MapDmaExport { .. }
            | Syscall::RevokeDmaExport { .. }
    )
}

/// Syscalls whose errors use the typed sentinels of [`encode_ipc_result`]
/// (`usize::MAX - 1..=4`) instead of the single generic error.
fn has_typed_errors(syscall: &Syscall) -> bool {
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

/// Encode the additive spawn-OOM result while preserving all other legacy errors.
fn encode_syscall_result(
    result: SyscallResult,
    error_sentinel: usize,
    supports_typed_oom: bool,
) -> usize {
    match result {
        Ok(value) => value,
        Err(SyscallError::OutOfMemory) if supports_typed_oom => error_sentinel - 1,
        // A broken pipe is a distinct, actionable condition for a stream write, so
        // it gets its own sentinel rather than collapsing into the generic error.
        Err(SyscallError::BrokenPipe) => error_sentinel - 2,
        Err(_) => error_sentinel,
    }
}

/// Syscalls whose callers decode the typed out-of-memory sentinel
/// (`error_sentinel - 1`); every other syscall keeps the single generic error.
fn supports_typed_oom(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::SpawnFromMem { .. }
            | Syscall::SpawnFromPath { .. }
            | Syscall::SpawnFromElf { .. }
            | Syscall::SpawnReplacement { .. }
            | Syscall::SpawnPinned { .. }
            | Syscall::TelemetrySnapshot { .. }
    )
}

/// Maximum bytes a single syscall may read/write through a user buffer.
/// Bounds kernel work per syscall and acts as a coarse sanity check against
/// `len = usize::MAX` style attacks. 64 MiB is well above any legitimate
/// caller need today; tighten further for specific syscalls (see MAX_LOG_MSG).
const MAX_USER_BUF: usize = 64 * 1024 * 1024;

/// Tighter cap for `Syscall::Log` since the kernel holds locks while printing.
const MAX_LOG_MSG: usize = 4096;

/// Returns `true` if the calling task satisfies the given capability check.
///
/// Lock-ordering: acquires SCHEDULER, drops before returning.
/// Gate on the caller's **Cell** rights (`Scheduler::cell_rights`, the root's
/// incarnation record), never on a per-thread copy: a revoke aimed at any
/// thread of the Cell is seen by all of them. A missing task fails closed.
fn caller_has_cap<F: Fn(&super::cap::CapSet) -> bool>(caller_id: usize, check: F) -> bool {
    super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.contains_key(&caller_id) && check(&sched.cell_rights(caller_id))
    })
}

fn caller_has_block_io(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.block_io)
}

/// `WaitIrq` admission for its `mmio_base` argument.
///
/// Zero means "no InterruptACK write". A nonzero base is stored and later
/// dereferenced by the riscv64/aarch64 ISR, so it must be the exact base of an
/// MMIO window the resource registry granted to the caller.
fn wait_irq_base_permitted(caller_id: usize, mmio_base: usize) -> bool {
    mmio_base == 0 || crate::resource_registry::lookup_mmio_owner(mmio_base) == Some(caller_id)
}

/// Read a task's live Cell identity `(cell_id, generation)`.
///
/// Takes the scheduler lock itself, so a caller must not already hold it. The
/// service registry records this identity so `LookupServiceBound` can answer with
/// a binding instead of a bare tid; a task whose identity is missing yields
/// `None` and the caller decides whether that is a refusal or an unbindable route.
fn live_cell_identity(tid: usize) -> Option<(u64, u64)> {
    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&tid))
        .map(|t| (t.cell_id.0, t.cell_generation))
}

/// The caller's incarnation for the state stash; a task without a live record
/// owns nothing and is refused.
fn stash_incarnation(tid: usize) -> Result<crate::cell::state_stash::Incarnation, SyscallError> {
    live_cell_identity(tid)
        .map(|(cell, generation)| crate::cell::state_stash::Incarnation { cell, generation })
        .ok_or(SyscallError::PermissionDenied)
}

/// Per-cell block-I/O range gate (Milestone 2.5 P03).
///
/// Replaces the old global `sector >= CELL_TABLE_BASE_LBA` check: the caller's
/// `block_regions` bitmask (from its manifest PART_* bits, or the legacy VFS
/// grant) defines which MBR partitions its raw block syscalls may address.
/// Deny-by-default: a sector outside every granted partition is rejected —
/// which structurally protects P2 (cell table) and P3 (snapshot), since no
/// bit exists for them. Logs every denial (silent denials cost us a day once).
fn check_block_access(caller_id: usize, sector: u64, count: u64) -> bool {
    use crate::loader::disk_layout as dl;
    let regions = super::SCHEDULER
        .lock()
        .as_ref()
        .map_or(0, |s| s.cell_rights(caller_id).block_regions);
    let end = match sector.checked_add(count) {
        Some(e) => e,
        None => return false,
    };
    const GRANTABLE: [(u8, u64, u64); 4] = [
        (0b001, dl::PART_FAT32_BASE_LBA, dl::PART_FAT32_SECTORS), // P1 (PART_DATA)
        (0b010, dl::PART_LFS_BASE_LBA, dl::PART_LFS_SECTORS),     // P4 (PART_LFS)
        (0b100, dl::PART_SRV_BASE_LBA, dl::PART_SRV_SECTORS), // P5 (SRV, CellosFS Native, co-granted w/ LFS)
        (
            0b1000,
            dl::PART_CELLSTORE_BASE_LBA,
            dl::PART_CELLSTORE_SECTORS,
        ), // P6 (cell-store, read-only; VFS /bin overlay)
    ];
    for (bit, base, size) in GRANTABLE {
        if regions & bit != 0 && sector >= base && end <= base + size {
            return true;
        }
    }
    log::warn!(
        "[blk] sector {}..{} denied for tid {} (regions={:#04b})",
        sector,
        end,
        caller_id,
        regions
    );
    false
}

fn caller_has_network(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.network)
}

fn caller_has_hypervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.hypervisor)
}
#[cfg(feature = "test-hooks")]
fn caller_has_development_silo_registration(caller_id: usize) -> bool {
    // A launch-route token on the root task, not a Cell right (not in CapSet).
    super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&caller_id).is_some_and(|task| {
            task.root_tid == caller_id && task.development_silo_registration_cap.is_some()
        })
    })
}

fn caller_has_spawn(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.spawn)
}

/// Tid of trusted init, published once by `loader::governed_spawn::spawn_init`
/// right after init's task is committed; zero before. It keys the
/// `MACHINE_PIN` registration restriction, so that restriction never trusts a
/// spawner-chosen task name. Tids are never reused, so a dead init's tid
/// cannot be taken over.
static TRUSTED_INIT_TID: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Record `tid` as trusted init. Only the first publication counts.
pub(crate) fn publish_trusted_init(tid: usize) {
    use core::sync::atomic::Ordering;
    let _first_wins =
        TRUSTED_INIT_TID.compare_exchange(0, tid, Ordering::AcqRel, Ordering::Acquire);
}

fn caller_is_trusted_init(caller_id: usize) -> bool {
    caller_id != 0 && TRUSTED_INIT_TID.load(core::sync::atomic::Ordering::Acquire) == caller_id
}

const SPAWN_ARGV_KEY: u64 = 0x0061_7267_7600_0000;

fn consumes_staged_spawn_argv(syscall: &Syscall) -> bool {
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
struct StagedSpawnArgvCleanup {
    caller_id: usize,
}

impl Drop for StagedSpawnArgvCleanup {
    fn drop(&mut self) {
        crate::cell::state_stash::discard_spawn_argv(self.caller_id);
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
fn staged_spawn_argv_cleanup(
    syscall: &Syscall,
    caller_id: usize,
) -> Option<StagedSpawnArgvCleanup> {
    if consumes_staged_spawn_argv(syscall) {
        Some(StagedSpawnArgvCleanup { caller_id })
    } else {
        None
    }
}

fn governed_spawn_request(
    caller_id: usize,
    route: crate::loader::launch_profile::LaunchRoute,
    child_ceiling: super::cap::CapSet,
    priority: u8,
    #[cfg(cellos_p01_trace)] trace: Option<crate::p01_measurement::Attempt>,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (incarnation, caller_authority, caller_root) = super::SCHEDULER
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

/// Whether the child may be named by its signed image (`Task::launch_named`):
/// the kernel resolved the path (Path, Pinned), or trusted init supplied the
/// bytes it read from its own `/bin` (init starts drivers from VFS grants).
/// Any other caller's ELF/Mem bytes carry an advisory path only.
fn names_child(caller_root: usize, route: crate::loader::launch_profile::LaunchRoute) -> bool {
    use crate::loader::launch_profile::LaunchRoute;
    matches!(route, LaunchRoute::Path | LaunchRoute::Pinned) || caller_is_trusted_init(caller_root)
}

fn governed_replacement_request(
    caller_id: usize,
    route_ceiling: super::cap::CapSet,
    replacement: crate::cell::hotswap::ReplacementReservation,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (incarnation, caller_authority) = super::SCHEDULER
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

/// The caller's launch identity, spawn and supervisor flags.
///
/// The identity is **never the task name** (the basename of a spawner-chosen
/// path, architecture review C-1). It is `"init"` for any task of the Cell
/// whose root the kernel published as trusted init, otherwise the program name
/// from the verified `__ViCell_image` section of the caller's Cell root
/// (`Task::image`) when the kernel itself resolved that root's install path
/// (`Task::launch_named`), and empty otherwise — unsigned or unnamed images,
/// caller-supplied ELF/Mem bytes, sandbox paths and aliases match no launch row. Threads share their Cell root's identity.
fn caller_launch_state(caller_id: usize) -> Option<(String, bool, bool)> {
    super::SCHEDULER.lock().as_ref().and_then(|sched| {
        let identity = launch_identity_in(sched, caller_id)?;
        let rights = sched.cell_rights(caller_id);
        Some((identity, rights.spawn, rights.supervisor))
    })
}

/// The launch identity of `caller_id` (see [`caller_launch_state`]) read under
/// a scheduler the caller already holds. `None` when the task is gone.
pub(super) fn launch_identity_in(
    sched: &super::scheduler::Scheduler,
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

/// Measured program names allowed to execute raw opcode 502 (platform
/// power-off): the signed `/bin/shutdown` worker only.
const RAW_SHUTDOWN_CALLERS: &[&str] = &["shutdown"];

fn raw_shutdown_permitted(name: &str) -> bool {
    RAW_SHUTDOWN_CALLERS.contains(&name)
}

/// Gate for raw opcode 502. Runs before any platform shutdown code.
fn authorize_raw_shutdown(caller_id: usize) -> Result<(), SyscallError> {
    let (name, _, _) =
        caller_launch_state(caller_id).unwrap_or((String::from("<unknown>"), false, false));
    if raw_shutdown_permitted(&name) {
        return Ok(());
    }
    log::warn!("[kernel] raw opcode 502 denied for {}", name);
    Err(SyscallError::PermissionDenied)
}

/// AP-PZ: allow/deny decision for raw opcode 502 over the reviewed names and
/// the live-task lookup (an unknown caller is denied without side effects).
#[cfg(feature = "test-hooks")]
pub(crate) fn test_raw_shutdown_gate() -> bool {
    raw_shutdown_permitted("shutdown")
        && !raw_shutdown_permitted("cellbox")
        && !raw_shutdown_permitted("init")
        && !raw_shutdown_permitted("supervisor")
        && !raw_shutdown_permitted("")
        && !raw_shutdown_permitted("shutdown2")
        && authorize_raw_shutdown(usize::MAX) == Err(SyscallError::PermissionDenied)
}

fn authorize_launch_edge(
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

pub(super) fn session_launch_request(
    caller: usize,
    path: &str,
    route: crate::loader::launch_profile::LaunchRoute,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let profile = authorize_launch_edge(caller, route, path)?;
    let guard = super::SCHEDULER.lock();
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
        .map_or(super::cap::CapSet::EMPTY, |s| s.cell_rights(caller));
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

/// Copy `len` bytes of a grant the caller owns into a kernel-owned ELF snapshot
/// (`SpawnFromElf` and session route 2). Admission must never read the grant
/// itself: the caller's other threads and grant peers can rewrite it between
/// signature verification and segment load.
pub(super) fn snapshot_grant_elf(
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

fn caller_has_supervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.supervisor)
}

fn caller_has_pcie_driver(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.pcie_driver)
}

fn caller_has_usb_driver(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.usb_driver)
}

fn caller_has_serial_port(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.serial_port)
}

/// The 16550 mechanism exists only where a 16550 exists.
///
/// These shims keep the serial syscalls compilable on every architecture while
/// making them inert off x86: a machine with no port-I/O mechanism cannot offer a
/// port, so the handlers fail closed instead of reaching for hardware that is not
/// there.
mod serial_port {
    #[cfg(target_arch = "x86_64")]
    pub(super) fn present(index: u32) -> bool {
        crate::hal::uart_16550::port_base_at(index as usize).is_some()
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn present(_index: u32) -> bool {
        false
    }

    /// The declared port at `index` as `(base, irq)`, or `None` when the board
    /// profile does not declare it. Off x86 there is no port profile to consult.
    #[cfg(target_arch = "x86_64")]
    pub(super) fn declared(index: u32) -> Option<(u16, u8)> {
        crate::board::selected_x86_64_soc()
            .serial_port(index as usize)
            .map(|port| (port.base, port.irq))
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn declared(_index: u32) -> Option<(u16, u8)> {
        None
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn write_byte(index: u32, byte: u8) -> bool {
        crate::hal::uart_16550::write_to(index as usize, byte)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn write_byte(_index: u32, _byte: u8) -> bool {
        false
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn read_byte(index: u32) -> Option<u8> {
        crate::hal::uart_16550::read_from(index as usize)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn read_byte(_index: u32) -> Option<u8> {
        None
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn configure_baud(index: u32, baud: u32) -> bool {
        crate::hal::uart_16550::configure_baud(index as usize, baud)
    }
    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn configure_baud(_index: u32, _baud: u32) -> bool {
        false
    }
}

fn register_driver_service(
    caller_id: usize,
    service_id: u16,
    publish_role: fn(usize),
    rollback_role: fn(usize),
) -> Result<usize, SyscallError> {
    let scheduler = crate::task::SCHEDULER.lock();
    let authorized = scheduler
        .as_ref()
        .and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            Some((scheduler.tasks.get(&caller_id)?, rights))
        })
        .is_some_and(|(task, rights)| {
            (rights.pcie_driver || rights.usb_driver)
                && !matches!(
                    &task.state,
                    crate::task::tcb::TaskState::Retiring | crate::task::tcb::TaskState::Terminated
                )
        });
    if !authorized {
        return Err(SyscallError::PermissionDenied);
    }

    // The identity is read under the same scheduler lock that authorized the
    // caller, so the published binding names an incarnation the kernel can stand
    // behind. A task with no live Cell identity is refused rather than recorded
    // as an unbindable route.
    let Some((cell_id, generation)) = scheduler
        .as_ref()
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|task| (task.cell_id.0, task.cell_generation))
    else {
        return Err(SyscallError::InvalidInput);
    };

    let published = crate::task::drivers::driver_cell::publish_role_or_rollback(
        caller_id,
        publish_role,
        || crate::cell::service_registry::register(service_id, caller_id, cell_id, generation),
        rollback_role,
    );
    drop(scheduler);
    if published {
        Ok(0)
    } else {
        Err(SyscallError::InvalidInput)
    }
}

fn caller_has_mmio_device(caller_id: usize, device: u8) -> bool {
    caller_has_cap(caller_id, |t| t.mmio_devices & device != 0)
}

/// 423: authorize and publish the USB HID event producer role.
///
/// Deliberately separate from [`register_driver_service`]: that helper accepts
/// either `PcieDriverCap` or `UsbDriverCap` and publishes the singleton NIC
/// role. A USB host cell on a PC that already has e1000/virtio-net must hold
/// only `UsbDriverCap` and must never overwrite `service::NIC_DRIVER`, so this
/// path checks `usb_driver_cap` alone and publishes `service::USB_HID_PRODUCER`.
fn register_usb_hid_producer_role(caller_id: usize) -> Result<usize, SyscallError> {
    let scheduler = crate::task::SCHEDULER.lock();
    let authorized = scheduler
        .as_ref()
        .and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            Some((scheduler.tasks.get(&caller_id)?, rights))
        })
        .is_some_and(|(task, rights)| {
            rights.usb_driver
                && !matches!(
                    &task.state,
                    crate::task::tcb::TaskState::Retiring | crate::task::tcb::TaskState::Terminated
                )
        });
    if !authorized {
        return Err(SyscallError::PermissionDenied);
    }

    let Some((cell_id, generation)) = scheduler
        .as_ref()
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|task| (task.cell_id.0, task.cell_generation))
    else {
        return Err(SyscallError::InvalidInput);
    };

    let published = crate::task::drivers::driver_cell::publish_role_or_rollback(
        caller_id,
        crate::task::drivers::driver_cell::register_usb_hid_producer,
        || {
            crate::cell::service_registry::register(
                api::syscall::service::USB_HID_PRODUCER,
                caller_id,
                cell_id,
                generation,
            )
        },
        crate::task::drivers::driver_cell::deregister_usb_hid_producer,
    );
    drop(scheduler);
    if published {
        Ok(0)
    } else {
        Err(SyscallError::InvalidInput)
    }
}

/// 416: bind the route of the block device the caller owns.
///
/// Checks in order, each before any effect: decode (`Malformed` →
/// `InvalidInput`), a live PcieDriverCap/UsbDriverCap caller (`PermissionDenied`),
/// ownership of the named device (`NotOwner` → `FileNotFound`), then the route
/// slot under `ROLE_PUBLICATION` (a refusal is `PermissionDenied`).
///
/// Ownership is read without SCHEDULER held: `request_mmio` takes the MMIO
/// registry before SCHEDULER, so the opposite order here could deadlock. Both
/// owner tables key the claim by the claiming task id (`RequestMmio` records
/// `CellId(tid)`), and a claim is released only at that task's retirement, so
/// it still holds when the caller is found live again under SCHEDULER, where
/// its `(cell, generation)` is read and the slot is published so exit teardown
/// cannot interleave. No service-registry entry is written.
fn register_block_device_route(
    caller_id: usize,
    kind: usize,
    id: usize,
    reserved: (usize, usize),
) -> Result<usize, SyscallError> {
    use api::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};
    let key = u16::try_from(kind)
        .ok()
        .filter(|_| reserved == (0, 0))
        .and_then(|kind| DeviceKey::new(kind, id as u64))
        .ok_or(SyscallError::InvalidInput)?;

    let live_driver = |scheduler: &Option<super::Scheduler>| {
        scheduler.as_ref().and_then(|scheduler| {
            let rights = scheduler.cell_rights(caller_id);
            let task = scheduler.tasks.get(&caller_id)?;
            ((rights.pcie_driver || rights.usb_driver)
                && !matches!(&task.state, TaskState::Retiring | TaskState::Terminated))
            .then_some((task.cell_id.0, task.cell_generation))
        })
    };
    if live_driver(&crate::task::SCHEDULER.lock()).is_none() {
        return Err(SyscallError::PermissionDenied);
    }

    let owned = match key.kind {
        BLOCK_DEVICE_PCI => {
            crate::resource_registry::owner_of_bdf(key.id as u32) == Some(caller_id)
        }
        BLOCK_DEVICE_MMIO => {
            crate::resource_registry::lookup_mmio_owner(key.id as usize) == Some(caller_id)
        }
        _ => false,
    };
    if !owned {
        return Err(SyscallError::FileNotFound);
    }

    let scheduler = crate::task::SCHEDULER.lock();
    let Some((cell_id, generation)) = live_driver(&scheduler) else {
        return Err(SyscallError::PermissionDenied);
    };
    let published = crate::task::drivers::driver_cell::publish_block_route(
        key,
        (caller_id, cell_id, generation),
    );
    drop(scheduler);
    published.map(|_| 0).ok_or(SyscallError::PermissionDenied)
}

fn caller_has_platform(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.platform)
}

/// ADR-0031 CP1 §6: 418/424 offer a function that is unowned or already the
/// caller's, and skip one a live task owns, so a second instance of one driver
/// reaches the next identical function.
/// H-3: a PCI query (418/424) must fall inside the caller's catalog row,
/// keyed by the caller Cell's measured program name (`catalog::PCI_DRIVERS`).
/// Logs the refused query so a missing row is visible in the boot log.
fn caller_pci_query_allowed(caller_id: usize, allowed: impl Fn(&str) -> bool) -> bool {
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

fn pcie_usable_by(caller_id: usize, bdf: u32) -> bool {
    crate::resource_registry::owner_of_bdf(bdf).is_none_or(|owner| owner == caller_id)
}

#[cfg(target_arch = "x86_64")]
fn pcie_function_error(error: crate::task::drivers::pcie_msi::PcieFunctionError) -> SyscallError {
    use crate::task::drivers::pcie_msi::PcieFunctionError as E;
    match error {
        E::InvalidInput => SyscallError::InvalidInput,
        E::PermissionDenied => SyscallError::PermissionDenied,
        E::NotFound => SyscallError::FileNotFound,
        E::NotSupported => SyscallError::NotSupported,
        E::NoVectors => SyscallError::OutOfMemory,
    }
}

/// 434 record bytes for an owned BDF.
#[cfg(target_arch = "x86_64")]
fn query_pcie_function(
    caller_id: usize,
    bdf: usize,
) -> Result<[u8; api::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
    let bdf = u32::try_from(bdf).map_err(|_| SyscallError::InvalidInput)?;
    crate::task::drivers::pcie_msi::query(caller_id, bdf)
        .map(|record| record.encode())
        .map_err(pcie_function_error)
}

/// 434 has no producer off x86: no function is reported.
#[cfg(not(target_arch = "x86_64"))]
fn query_pcie_function(
    _caller_id: usize,
    _bdf: usize,
) -> Result<[u8; api::pcie_function::PCIE_FUNCTION_RECORD_LEN], SyscallError> {
    Err(SyscallError::FileNotFound)
}

/// 435 packed result for an owned BDF.
#[cfg(target_arch = "x86_64")]
fn alloc_pcie_irq(caller_id: usize, bdf: usize, want: usize, mode: usize) -> SyscallResult {
    let bdf = u32::try_from(bdf).map_err(|_| SyscallError::InvalidInput)?;
    crate::task::drivers::pcie_msi::allocate(caller_id, bdf, want, mode)
        .map(|irq| irq.pack())
        .map_err(pcie_function_error)
}

/// MSI is x86-only (CP1 §1).
#[cfg(not(target_arch = "x86_64"))]
fn alloc_pcie_irq(_caller_id: usize, _bdf: usize, _want: usize, _mode: usize) -> SyscallResult {
    Err(SyscallError::NotSupported)
}

/// 436 after the `SupervisorCap` and reserved-argument gates. Reboot does not
/// return; poweroff returns only when the machine is still running.
#[cfg(target_arch = "x86_64")]
fn system_power(caller_id: usize, action: usize) -> SyscallResult {
    use api::syscall::system_power as sp;
    match action {
        sp::ACTION_REBOOT => {
            log::warn!(
                "[kernel] SystemPower: reboot requested by tid {}",
                caller_id
            );
            crate::acpi::power::reboot()
        }
        sp::ACTION_POWEROFF => {
            log::warn!(
                "[kernel] SystemPower: poweroff requested by tid {}",
                caller_id
            );
            crate::acpi::power::poweroff();
            Err(SyscallError::NotSupported)
        }
        _ => Err(SyscallError::InvalidInput),
    }
}

/// Reset and S5 come from ACPI, which only x86_64 parses (CP1 §5).
#[cfg(not(target_arch = "x86_64"))]
fn system_power(_caller_id: usize, action: usize) -> SyscallResult {
    use api::syscall::system_power as sp;
    match action {
        sp::ACTION_REBOOT | sp::ACTION_POWEROFF => Err(SyscallError::NotSupported),
        _ => Err(SyscallError::InvalidInput),
    }
}

/// Serialize a matched PCIe device into the 48-byte `PcieDeviceInfo` wire record
/// (padded from the 44 meaningful bytes by `vendor_id`/`device_id` alignment) and
/// write it to the caller's `out_ptr`.
///
/// Shared by the class-triple (`FindPcieDevice`) and exact-identity
/// (`FindPcieDeviceByVendor`) queries so the two hand-out paths cannot drift.
/// Claims BDF ownership first and returns 0 without writing when a live Driver
/// Cell already owns the device.
fn offer_pcie_device(
    caller_id: usize,
    out_ptr: usize,
    dev: crate::task::drivers::pcie_ecam::PciDevice,
) -> Result<usize, SyscallError> {
    let bdf = dev.canonical_bdf();
    let bar0_base = dev.bars[0].base_addr();
    let bar0_len: u64 = match dev.bars[0] {
        crate::task::drivers::pcie_ecam::Bar::Memory32 { size, .. } => size as u64,
        crate::task::drivers::pcie_ecam::Bar::Memory64 { size, .. } => size,
        _ => 0x4000, // fallback 16 KiB
    };
    // First *memory* BAR with a non-zero base, for devices whose BAR0 is I/O or
    // empty (ICH9's and VMware's AHCI decode their ABAR at BAR5). Every recorded
    // function carries its full BAR set: the boot scan decodes bus 0, and
    // `register_device` decodes a Platform-Cell-registered function (bus ≥ 1,
    // e.g. behind VMware's `pciBridge0`) the same way. `bar0_*` keep their
    // existing first-BAR meaning; NVMe/e1000 are unaffected because their BAR0
    // is memory.
    let (bar_mem_base, bar_mem_len): (u64, u64) =
        crate::task::drivers::pcie_ecam::first_memory_bar(&dev.bars)
            .map(|(base, size)| (base, if size == 0 { 0x4000 } else { size }))
            .unwrap_or((bar0_base, bar0_len));
    // Under VT-d, a virtio function without ACCESS_PLATFORM or a PVSCSI
    // adapter would DMA around the translation; no cell may own it (ADR-0031
    // phase 05 review HIGH; H-S3-13). Judged only before the first claim: the
    // virtio probe remaps the common-config page kernel-only, which must not
    // happen under a live owner's mapping.
    #[cfg(target_arch = "x86_64")]
    if crate::resource_registry::owner_of_bdf(bdf).is_none()
        && crate::task::drivers::pcie_msi::dma_bypasses_iommu(&dev)
    {
        return Ok(0);
    }
    // Claim without displacing a live Driver Cell. A competing instance observes
    // "not found" and may retry after reap.
    if !crate::resource_registry::claim_bdf_owner(bdf, caller_id) {
        return Ok(0);
    }
    // The owner may now reach its BARs: memory decoding on, bus mastering
    // still off until its first GrantDma, after the cell has reset the device.
    // Only when firmware placed every memory BAR: an unassigned one would
    // decode at address 0 (the cell then fails closed on base 0, as before).
    if crate::task::drivers::pcie_ecam::memory_bars_assigned(&dev.bars) {
        crate::task::drivers::pcie_ecam::enable_memory_decode(bdf);
    }
    if out_ptr != 0 {
        let mut dev_bytes = [0u8; 48];
        dev_bytes[0..4].copy_from_slice(&bdf.to_ne_bytes());
        dev_bytes[4..8].copy_from_slice(&1u32.to_ne_bytes());
        dev_bytes[8..16].copy_from_slice(&bar0_base.to_ne_bytes());
        dev_bytes[16..24].copy_from_slice(&bar0_len.to_ne_bytes());
        dev_bytes[24..32].copy_from_slice(&bar_mem_base.to_ne_bytes());
        dev_bytes[32..40].copy_from_slice(&bar_mem_len.to_ne_bytes());
        dev_bytes[40..42].copy_from_slice(&dev.vendor_id.to_ne_bytes());
        dev_bytes[42..44].copy_from_slice(&dev.device_id.to_ne_bytes());
        write_user_slice(caller_id, out_ptr, &dev_bytes, MAX_USER_BUF)?;
    }
    Ok(1)
}

/// Validate a user-supplied (ptr, len) buffer descriptor.
///
/// Rejects: NULL pointer, zero-length when expected non-empty, lengths above
/// `max`, and pointer+length arithmetic overflow.
///
/// Does NOT walk the page table to confirm the U-bit. The trap handler enables
/// SUM only for the duration of `handle_syscall`, so a kernel-space `ptr`
/// supplied by user code will fault on access — but the fault is far more
/// graceful when we reject obvious garbage up front.
#[inline]
pub(super) fn validate_user_buf(ptr: usize, len: usize, max: usize) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidInput);
    }
    if len > max {
        return Err(SyscallError::BufferTooSmall);
    }
    if ptr.checked_add(len).is_none() {
        return Err(SyscallError::InvalidInput);
    }
    Ok(())
}

/// Derive the phase-03 recoverable copy view for a live caller task.
///
/// Acquires SCHEDULER, clones the view, and releases the lock. A missing task
/// is rejected so an unavailable task record cannot become SAS authority.
fn caller_copy_view(caller_id: usize) -> Result<TaskCopyView, SyscallError> {
    TaskCopyView::for_task(caller_id).ok_or(SyscallError::InvalidInput)
}

/// Does this task hold the pipe endpoint? Takes `SCHEDULER`, so it must not be
/// called while that lock is held.
fn caller_owns_pipe(caller_id: usize, handle: super::pipe::PipeHandle) -> bool {
    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .is_some_and(|task| task.pipe_handles.contains(&handle))
}

/// Copy view for a caller, for kernel-internal users outside this module (the
/// futex word read). Same validation as every other user-pointer access.
pub(crate) fn caller_copy_view_for(caller_id: usize) -> Result<TaskCopyView, SyscallError> {
    caller_copy_view(caller_id)
}
/// Return the live Cell-generation binding for a task.
///
/// Callers holding a grant table preserve the documented
/// `*_GRANT_TABLE → SCHEDULER` order while taking this snapshot.
fn live_task_binding(task_id: usize) -> Option<(CellId, u64)> {
    let scheduler = super::SCHEDULER.lock();
    scheduler
        .as_ref()?
        .tasks
        .get(&task_id)
        .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
        .map(|task| (task.cell_id, task.cell_generation))
}

#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn record_end_containing(base: usize, size: usize, ptr: usize) -> Option<usize> {
    let end = base.checked_add(size)?;
    (base <= ptr && ptr < end).then_some(end)
}

#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn sas_record_end(
    sched: &super::scheduler::Scheduler,
    page_grants: Option<&BTreeMap<usize, PageGrant>>,
    registered_grants: Option<&BTreeMap<usize, RegGrant>>,
    caller_id: usize,
    ptr: usize,
) -> Option<usize> {
    let (cell_id, generation, root_tid) = {
        let caller = sched.tasks.get(&caller_id)?;
        if matches!(caller.state, TaskState::Retiring | TaskState::Terminated) {
            return None;
        }
        (caller.cell_id, caller.cell_generation, caller.root_tid)
    };
    let stack_end = sched
        .tasks
        .values()
        .filter(|task| {
            !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
                && task.cell_id == cell_id
                && task.cell_generation == generation
                && task.root_tid == root_tid
        })
        .filter_map(|task| {
            task.user_stack.as_ref().and_then(|stack| {
                record_end_containing(stack.usable_start(), stack.usable_bytes(), ptr)
            })
        })
        .max();
    let segment_end = sched.tasks.get(&root_tid).and_then(|root| {
        (!matches!(root.state, TaskState::Retiring | TaskState::Terminated)
            && root.cell_id == cell_id
            && root.cell_generation == generation
            && root.root_tid == root_tid)
            .then_some(root.segment_mem.as_ref())?
            .and_then(|segments| segments.writable_page_end_containing(ptr))
    });
    let page_grant_end = page_grants.and_then(|grants| {
        grants
            .values()
            .filter(|grant| {
                grant.owner == caller_id
                    && grant.owner_cell == cell_id
                    && grant.owner_generation == generation
            })
            .filter_map(|grant| record_end_containing(grant.base, grant.size, ptr))
            .max()
    });
    let registered_grant_end = registered_grants.and_then(|grants| {
        grants
            .values()
            .filter(|grant| {
                grant.owner == caller_id
                    && grant.owner_cell == cell_id
                    && grant.owner_generation == generation
            })
            .filter_map(|grant| record_end_containing(grant.base, grant.size, ptr))
            .max()
    });
    [stack_end, segment_end, page_grant_end, registered_grant_end]
        .into_iter()
        .flatten()
        .max()
}

/// Verify contiguous coverage by live caller-owned writable records.
///
/// Each loop consumes at least one record. A gap, peer record, stale root, or
/// overflow fails closed without allocating or consulting page writability as
/// an ownership signal.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn sas_caller_owned_span(
    sched: &super::scheduler::Scheduler,
    page_grants: Option<&BTreeMap<usize, PageGrant>>,
    registered_grants: Option<&BTreeMap<usize, RegGrant>>,
    caller_id: usize,
    ptr: usize,
    len: usize,
) -> bool {
    let Some(end) = ptr.checked_add(len) else {
        return false;
    };
    let mut cursor = ptr;
    while cursor < end {
        let Some(next) = sas_record_end(sched, page_grants, registered_grants, caller_id, cursor)
        else {
            return false;
        };
        if next <= cursor {
            return false;
        }
        cursor = next.min(end);
    }
    true
}

/// Preflight an exact caller-owned output span without moving user bytes.
///
/// SAS ownership comes only from caller stack/root-segment/grant records.
/// Callers that consume or otherwise act on external state must repeat the
/// ownership check under their operation-specific final-commit lock set.
fn preflight_user_output(caller_id: usize, ptr: usize, len: usize) -> Result<(), SyscallError> {
    let view = caller_copy_view(caller_id)?;
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    ))]
    if view.is_sas() {
        // Lease order: PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER. This
        // extends the established grant-table → scheduler order used by
        // GrantSlice; no removal path may unmap or reuse a contributing record
        // while this final-authorization lock set is held.
        let page_grants = grant_table_lock().lock();
        let registered_grants = reg_grant_table_lock().lock();
        let scheduler = super::SCHEDULER.lock();
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

/// A copied request needs no long-lived lease; the reply span must stay backed
/// until after the owner mutation and final write, including parked stream IO.
struct SessionOutputPin {
    span: Option<(usize, usize, usize)>,
}
impl SessionOutputPin {
    fn reserve(
        caller: usize,
        ptr: usize,
        len: usize,
        view: &TaskCopyView,
    ) -> Result<Self, SyscallError> {
        // Same gate as `preflight_user_output`: the SAS ownership lease exists
        // only in builds with a domain backend.
        #[cfg(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )
        ))]
        {
            if !view.is_sas() {
                return Ok(Self { span: None });
            }
            let page_grants = grant_table_lock().lock();
            let registered = reg_grant_table_lock().lock();
            let guard = super::SCHEDULER.lock();
            let sched = guard.as_ref().ok_or(SyscallError::InvalidInput)?;
            if !sas_caller_owned_span(
                sched,
                page_grants.as_ref(),
                registered.as_ref(),
                caller,
                ptr,
                len,
            ) {
                return Err(SyscallError::InvalidInput);
            }
            view.validate_writable(ptr, len)
                .map_err(|_| SyscallError::InvalidInput)?;
            crate::memory::pin::pin(ptr, len, caller).map_err(|_| SyscallError::OutOfMemory)?;
            Ok(Self {
                span: Some((ptr, len, caller)),
            })
        }
        #[cfg(not(all(
            feature = "native-domains",
            any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )
        )))]
        {
            let _ = caller;
            view.validate_writable(ptr, len)
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(Self { span: None })
        }
    }
}
impl Drop for SessionOutputPin {
    fn drop(&mut self) {
        if let Some((ptr, len, caller)) = self.span {
            // A killed parked syscall never resumes: the canonical quiescent
            // DMA/grant retirement funnel acknowledges its ordinary pin.
            crate::memory::pin::rollback_pin(ptr, len, caller);
        }
    }
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
fn pause_getrandom_final_write_for_revoke_race(caller_id: usize) -> bool {
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
fn verify_getrandom_revoke_race_done_state(was_armed: bool) {
    use core::sync::atomic::Ordering;

    if !was_armed {
        return;
    }
    GETRANDOM_RACE_NO_EARLY_DONE.store(
        !GETRANDOM_RACE_DONE.load(Ordering::Acquire),
        Ordering::Release,
    );
}

fn write_getrandom_output(caller_id: usize, ptr: usize, bytes: &[u8]) -> Result<(), SyscallError> {
    let view = caller_copy_view(caller_id)?;
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    ))]
    if view.is_sas() {
        // This scoped, no-allocation lease covers every record contributing to
        // a contiguous span. Removal waits for its member lock before it can
        // mark retiring/revoked, unmap, or return backing to the allocator.
        let page_grants = grant_table_lock().lock();
        let registered_grants = reg_grant_table_lock().lock();
        let scheduler = super::SCHEDULER.lock();
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

fn read_user_string(
    caller_id: usize,
    ptr: usize,
    len: usize,
    max: usize,
) -> Result<alloc::string::String, SyscallError> {
    if len == 0 || len > max {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(ptr, len, max)?;
    let view = caller_copy_view(caller_id)?;
    let bytes = view
        .read_bytes(ptr, len)
        .map_err(|_| SyscallError::InvalidInput)?;
    alloc::string::String::from_utf8(bytes).map_err(|_| SyscallError::InvalidInput)
}

fn read_user_slice(
    caller_id: usize,
    ptr: usize,
    len: usize,
    max: usize,
) -> Result<alloc::vec::Vec<u8>, SyscallError> {
    if len > max {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(ptr, len, max)?;
    let view = caller_copy_view(caller_id)?;
    view.read_bytes(ptr, len)
        .map_err(|_| SyscallError::InvalidInput)
}

fn write_user_slice(
    caller_id: usize,
    ptr: usize,
    bytes: &[u8],
    max: usize,
) -> Result<(), SyscallError> {
    if bytes.len() > max {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(ptr, bytes.len(), max)?;
    let view = caller_copy_view(caller_id)?;
    view.write_bytes(ptr, bytes)
        .map_err(|_| SyscallError::InvalidInput)
}

fn read_cell_owner_request(
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

/// Receive-side staging copy of a queued payload: inline up to
/// [`MSG_PAYLOAD_INLINE`] bytes, so small receives never touch the heap.
#[derive(Clone, Debug)]
pub(super) enum MsgPayload {
    Inline {
        len: u8,
        bytes: [u8; MSG_PAYLOAD_INLINE],
    },
    Heap(alloc::vec::Vec<u8>),
}

/// Largest payload [`MsgPayload`] stores without allocating.
const MSG_PAYLOAD_INLINE: usize = 32;

impl MsgPayload {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len as usize,
            Self::Heap(vec) => vec.len(),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { len, bytes } => &bytes[..*len as usize],
            Self::Heap(vec) => vec.as_slice(),
        }
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, ()> {
        if slice.len() <= MSG_PAYLOAD_INLINE {
            let mut bytes = [0u8; MSG_PAYLOAD_INLINE];
            bytes[..slice.len()].copy_from_slice(slice);
            Ok(Self::Inline {
                len: slice.len() as u8,
                bytes,
            })
        } else {
            let mut vec = alloc::vec::Vec::new();
            vec.try_reserve_exact(slice.len()).map_err(|_| ())?;
            vec.extend_from_slice(slice);
            Ok(Self::Heap(vec))
        }
    }
}

impl core::ops::Deref for MsgPayload {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

/// Non-owning peek at the next delivery event. Payload bytes are copied into
/// kernel memory; the original record is NOT removed. Call `commit_resume`
/// after a successful copy-out to remove exactly the same event.
pub(super) enum ResumeSnapshot {
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
        wire_header: Option<super::ipc_wire::IpcWireHeader>,
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
pub(super) fn snapshot_resume(task: &super::Task, mask: usize) -> Result<ResumeSnapshot, ()> {
    // Owner-death events take priority (same order as take_resume_delivery).
    if super::Task::owner_death_matches_receive_mask(mask) {
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

/// Bind the dispatch boundary while the receive snapshot owns the scheduler
/// lock, before an independent caller can cancel and remove the queued record.
fn snapshot_resume_dispatched(
    sched: &mut super::scheduler::Scheduler,
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
        super::async_ipc::dispatch(sched, caller_id, *header);
    }
    Ok(snap)
}

/// Commit the exact event that was peeked by `snapshot_resume`.
/// Must be called under the scheduler lock with the same `task`.
pub(super) fn commit_resume(task: &mut super::Task, snap: &ResumeSnapshot) {
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
fn take_queued_death(
    caller_id: usize,
    mask: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> Result<Option<usize>, SyscallError> {
    let pending = {
        let mut guard = super::SCHEDULER.lock();
        guard.as_mut().and_then(|sched| {
            let t = sched.tasks.get_mut(&caller_id)?;
            let owner_death = super::Task::owner_death_matches_receive_mask(mask)
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

    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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

/// Legacy owned-removal delivery enum; used by selftests that consume the
/// event and immediately inspect it. Production paths use `snapshot_resume` +
/// `commit_resume` instead. Do not add new callers.
#[allow(dead_code)]
pub(super) enum ResumeDelivery {
    Death { sender_tid: usize, reason: usize },
    Message(super::tcb::PendingMsg),
    Wake,
}

/// Consume the next matching delivery event and return it. Selftest-only;
/// production callers use `snapshot_resume` + `commit_resume`.
pub(super) fn take_resume_delivery(task: &mut super::Task, mask: usize) -> ResumeDelivery {
    // Snapshot without allocation — Death and Wake carry no payload.
    // For Message we need to remove the record, so we take it directly.
    if super::Task::owner_death_matches_receive_mask(mask) {
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
            sender_tid: task.current_caller.unwrap_or(0),
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
    let sched_guard = super::SCHEDULER.lock();
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

/// The attested `CALLER_FLAG_VFS_MUTATE` bit: an explicit (non-permit-all)
/// allowlist declaring bit 63 AND the granted `vfs_mutate` right (annex
/// S2-T/S2-W §3). The declaration alone no longer attests anything, so a Cell
/// whose edge withholds W (the `/apps/`, `/data/bin/`, `/home/` sandbox) is
/// refused every classified VFS mutation whatever its ELF declares.
fn vfs_mutate_caller_flag(allowlist: u64, vfs_mutate: bool) -> u32 {
    if vfs_mutate && allowlist != u64::MAX && (allowlist & (1u64 << 63)) != 0 {
        api::caller_identity::CALLER_FLAG_VFS_MUTATE
    } else {
        0
    }
}

#[derive(Clone, Copy)]
pub(super) struct VfsGrantContext {
    grant_owner: usize,
    grant_owner_cell_id: u64,
    grant_owner_cell_generation: u64,
    request_generation: u64,
    pending_revoke: bool,
}

pub(super) enum VfsGrantLookup {
    NotVfs,
    MissingContext,
    Active(VfsGrantContext),
}

fn sender_cell_context_in_sched(
    sched: &super::scheduler::Scheduler,
    sender_tid: usize,
) -> (u64, u64) {
    super::sender_context(sched, sender_tid)
}

pub(super) fn current_vfs_grant_lookup(caller_id: usize) -> VfsGrantLookup {
    let sched_guard = super::SCHEDULER.lock();
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

fn finish_vfs_send_release(caller_id: usize, target: usize, context: Option<VfsGrantContext>) {
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
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            let _ = task
                .clear_current_caller_context_if(context.grant_owner, context.request_generation);
        }
    }
}

/// End a VFS request abandoned at its public receive boundary. Entering that
/// receive proves the holder has stopped using the prior GrantSlice pointer.
fn finish_vfs_context_drop(caller_id: usize, dropped: Option<(usize, u64)>) {
    let Some((grant_owner, request_generation)) = dropped else {
        return;
    };
    for range in crate::memory::pin::release_vfs_lease(caller_id, grant_owner, request_generation) {
        free_released_range(range);
    }
}

/// Install the exact VFS request lease while its scheduler context is live.
///
/// Production callers hold the matching grant-table lock. The complete order is
/// therefore `PAGE_GRANT_TABLE` or `REG_GRANT_TABLE` → `SCHEDULER` → pin
/// `REGISTRY` (a leaf). Task exit holds `SCHEDULER` only while marking
/// `REGISTRY`, and grant teardown never takes `SCHEDULER`, so no inverse edge
/// exists.
fn install_vfs_lease_if_context_live(
    caller_id: usize,
    context: VfsGrantContext,
    base: usize,
    size: usize,
    grant_id: usize,
) -> bool {
    let sched_guard = super::SCHEDULER.lock();
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
pub(super) fn test_install_vfs_lease_if_context_live(
    caller_id: usize,
    context: VfsGrantContext,
    base: usize,
    size: usize,
    grant_id: usize,
) -> bool {
    install_vfs_lease_if_context_live(caller_id, context, base, size, grant_id)
}

#[derive(Clone, Copy)]
struct GrantSliceRequest {
    caller_id: usize,
    grant_id: usize,
    size_out_ptr: usize,
    expected_owner_tid: usize,
    /// ABI a3 `LEASE`; the handler has already required a nonzero
    /// `expected_owner_tid`, which keys the lease.
    lease: bool,
    vfs_context: Option<VfsGrantContext>,
}

struct GrantSliceAccess {
    base: usize,
    size: usize,
    size_out: Option<*mut usize>,
}

/// GrantSlice ABI a2: a grantee (the compositor) cannot otherwise learn who
/// registered a grant shared to it, so a guessable grant id would let any
/// client present another client's buffer as its own. A nonzero
/// `expected_owner_tid` must name the row's owner; a mismatch is reported
/// exactly as an unknown id, so the probe reveals neither existence nor owner.
/// Zero keeps the pre-ABI behavior (no owner expectation).
fn grant_owner_expected(expected_owner_tid: usize, owner: usize) -> bool {
    expected_owner_tid == 0 || expected_owner_tid == owner
}

/// Publish the `LEASE` requested by a `GrantSlice` while the caller holds the
/// grant-table lock that resolved the row.
///
/// Lock order grant table → pin `REGISTRY` (leaf), the same edge the VFS lease
/// uses: the owner's `GrantFree`/`GrantUnregister` and the owner-death reaper
/// both remove the row under that table lock, so they either run first (the
/// row is gone and the slice answers as unknown) or observe this lease.
/// A refusal is reported as the unknown-id sentinel, so it is only logged.
fn lease_grant_slice_locked(
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

fn authorize_grant_slice_locked(
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
    let size_out =
        super::user_out::resolve_optional_usize_slot(request.caller_id, request.size_out_ptr)?;
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
fn resolve_and_lease_grant(
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

/// GrantShare for a private-root pair: publish the exact
/// `(target identity, generation, root, rights)` tuple into the receiver root,
/// then record `shared_to`.
///
/// A redundant share of the same tuple is a no-op; any change republishes after
/// revoking the old receiver PTE and shooting it down. Unsupported rights
/// (write-only) and a non-live or non-domain target are refused with the
/// established share failure.
// reason: the eight values are the share's exact inputs — row identity (owner,
// base, domain, shared_to) plus the receiver binding (caller, target tid/root,
// perm). Grouping them into a struct would only move the same list.
#[allow(clippy::too_many_arguments)]
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn domain_grant_share_row(
    owner: usize,
    base: usize,
    domain: Option<&Arc<super::domain_grant::DomainGrant>>,
    shared_to: &mut Option<(usize, GrantPerm)>,
    caller_id: usize,
    target_tid: usize,
    target_root: &Arc<crate::memory::address_space::AddressSpace>,
    perm: GrantPerm,
) -> Result<(), SyscallError> {
    let Some(record) = domain else {
        return Err(SyscallError::PermissionDenied);
    };
    if caller_id != owner || record.state() != super::domain_grant::DomainGrantState::Live {
        return Err(SyscallError::PermissionDenied);
    }
    let Some(rights) = super::domain_grant::rights_for(perm) else {
        log::warn!(
            "[grant] GrantShare {base:#x} refused: write-only has no domain page representation"
        );
        return Err(SyscallError::PermissionDenied);
    };
    let Some((cell, generation)) = live_task_binding(target_tid) else {
        return Err(SyscallError::PermissionDenied);
    };
    if !record.receiver_matches(cell, generation, target_root, rights) {
        record
            .publish(target_root, cell, generation, rights)
            .map_err(|error| {
                log::warn!("[grant] GrantShare {base:#x} receiver publish refused: {error:?}");
                SyscallError::PermissionDenied
            })?;
    }
    *shared_to = Some((target_tid, perm));
    Ok(())
}

/// Resolve one grant row for a capable domain receiver.
///
/// Returns the resolved base on success and the established `usize::MAX`
/// "not authorized" sentinel for every refusal: an unresolvable or non-domain
/// receiver, a SAS-owned record, an unresolvable or dying owner, a record that
/// is not `Live`, missing or unsupported rights, a partial receiver mapping, or
/// an unacknowledged rollback invalidation.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
#[allow(clippy::too_many_arguments)]
fn domain_grant_slice_row(
    owner: usize,
    base: usize,
    size: usize,
    shared_to: Option<(usize, GrantPerm)>,
    domain: Option<&Arc<super::domain_grant::DomainGrant>>,
    caller_id: usize,
    expected_owner_tid: usize,
    receiver: &Arc<crate::memory::address_space::AddressSpace>,
    size_out: Option<*mut usize>,
    lease_grant_id: Option<usize>,
) -> Result<usize, SyscallError> {
    // Same sentinel as an id absent from both tables (see `grant_owner_expected`).
    if !grant_owner_expected(expected_owner_tid, owner) {
        return Ok(usize::MAX);
    }
    let Some(record) = domain else {
        // A SAS-owned record's frames are not mapped in a private root, and a
        // mapping made here could not be revoked from the SAS side.
        return Ok(usize::MAX);
    };
    if !record.owner_is_live() || record.state() != super::domain_grant::DomainGrantState::Live {
        return Ok(usize::MAX);
    }
    // The owner resolves its own backing without a second mapping: the owner
    // domain page was published with the record.
    if caller_id == owner {
        if lease_grant_id.is_some_and(|grant_id| {
            !lease_grant_slice_locked(caller_id, grant_id, owner, base, size)
        }) {
            return Ok(usize::MAX);
        }
        super::user_out::write_resolved_optional_usize(size_out, size);
        return Ok(base);
    }
    let Some((shared_tid, perm)) = shared_to else {
        return Ok(usize::MAX);
    };
    if shared_tid != caller_id {
        return Ok(usize::MAX);
    }
    let Some(rights) = super::domain_grant::rights_for(perm) else {
        return Ok(usize::MAX);
    };
    let Some((cell, generation)) = live_task_binding(caller_id) else {
        return Ok(usize::MAX);
    };
    // The lease is published before the receiver mapping and taken back if the
    // publish fails, all under the resolving table lock, so neither a failed
    // slice nor a racing GrantFree can observe one without the other. The
    // lease keeps the frames out of the allocator; at owner death the domain
    // reaper still revokes this receiver mapping, so a leased domain receiver
    // faults rather than reads reused memory.
    if lease_grant_id
        .is_some_and(|grant_id| !lease_grant_slice_locked(caller_id, grant_id, owner, base, size))
    {
        return Ok(usize::MAX);
    }
    match record.publish(receiver, cell, generation, rights) {
        Ok(()) => {
            super::user_out::write_resolved_optional_usize(size_out, size);
            Ok(base)
        }
        Err(error) => {
            if let Some(grant_id) = lease_grant_id {
                crate::memory::pin::unlease_grant_hold(caller_id, owner, grant_id);
            }
            log::warn!(
                "[grant] GrantSlice {base:#x} refused for domain task {caller_id}: {error:?}"
            );
            Ok(usize::MAX)
        }
    }
}

/// GrantSlice entry point for a capable domain receiver: resolve the row in
/// either table and apply the lifecycle-aware resolver under the same lock that
/// linearizes teardown.
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", feature = "test-hooks")
    )
))]
fn resolve_domain_grant_slice(
    caller_id: usize,
    grant_id: usize,
    size_out_ptr: usize,
    expected_owner_tid: usize,
    lease: bool,
    receiver: &Arc<crate::memory::address_space::AddressSpace>,
) -> Result<usize, SyscallError> {
    let lease_grant_id = lease.then_some(grant_id);
    let size_out = super::user_out::resolve_optional_usize_slot(caller_id, size_out_ptr)?;
    {
        let tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) {
            return domain_grant_slice_row(
                grant.owner,
                grant.base,
                grant.size,
                grant.shared_to,
                grant.domain.as_ref(),
                caller_id,
                expected_owner_tid,
                receiver,
                size_out,
                lease_grant_id,
            );
        }
    }
    let tbl = reg_grant_table_lock().lock();
    let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) else {
        return Ok(usize::MAX);
    };
    domain_grant_slice_row(
        grant.owner,
        grant.base,
        grant.size,
        grant.shared_to,
        grant.domain.as_ref(),
        caller_id,
        expected_owner_tid,
        receiver,
        size_out,
        lease_grant_id,
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
fn write_caller_identity(caller_id: usize, buf_ptr: usize, buf_len: usize, sender_tid: usize) {
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

/// The oldest queued owner death a `TryRecv` with `mask` may take: only the
/// wildcard receive (`Task::owner_death_matches_receive_mask`), FIFO, and
/// `(token, dead_root_tid, exit_reason)` as queued. Nothing is removed here and
/// `pending_deaths` (NotifyOnExit) is never consulted: delivering those would
/// change every existing `TryRecv(0)` caller.
fn peek_owner_death_for_try_recv(task: &super::Task, mask: usize) -> Option<(u64, usize, usize)> {
    super::Task::owner_death_matches_receive_mask(mask)
        .then(|| task.pending_owner_deaths.first().copied())
        .flatten()
}

/// Drop the exact queued owner-death record `token` after its delivery copy
/// succeeded; a record already gone (or another one now at the head) is left alone.
fn remove_owner_death(task: &mut super::Task, token: u64) {
    if let Some(pos) = task
        .pending_owner_deaths
        .iter()
        .position(|(queued, _, _)| *queued == token)
    {
        task.pending_owner_deaths.remove(pos);
    }
}

/// May `caller_id` read the kernel's provenance record for `target_cell`?
///
/// Two callers have a reason to: the filesystem service, which must decide
/// whether to bind the set, and a cell asking about itself. Anything else
/// enumerating another cell's grant is reconnaissance with no legitimate use.
///
/// The filesystem service is identified by the service registry rather than by
/// name, because a cell's name comes from a spawner-chosen path hint and is
/// therefore forgeable; registering a well-known service id is SpawnCap-gated
/// and is not.
///
/// Lock order SCHEDULER → REGISTRY, matching `Scheduler::reap`; REGISTRY is a
/// leaf and is never held across a scheduler acquisition.
fn may_query_dir_handles(caller_id: usize, target_cell: u64) -> bool {
    let guard = super::SCHEDULER.lock();
    let Some(sched) = guard.as_ref() else {
        return false;
    };
    let Some(caller_cell) = sched.tasks.get(&caller_id).map(|t| t.cell_id.0) else {
        return false;
    };
    if caller_cell == 0 {
        return false;
    }
    if caller_cell == target_cell {
        return true;
    }
    crate::cell::service_registry::lookup(api::syscall::service::VFS)
        .and_then(|tid| sched.tasks.get(&tid))
        .is_some_and(|vfs| vfs.cell_id.0 == caller_cell)
}

pub(super) fn encode_task_state(state: &TaskState) -> u32 {
    match state {
        TaskState::Ready => 0,
        TaskState::Running => 1,
        TaskState::Terminated | TaskState::Retiring => 3,
        _ => 2,
    }
}

fn encode_task_name(name: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = name.as_bytes();
    let len = core::cmp::min(bytes.len(), out.len());
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

fn task_heap_bytes(task: &crate::task::tcb::Task) -> u64 {
    crate::memory::cell_quota::in_use(task.cell_id) as u64
}

fn task_owned_bytes(task: &crate::task::tcb::Task) -> u64 {
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

fn snapshot_process_info(task: &crate::task::tcb::Task) -> api::syscall::ProcessInfo {
    api::syscall::ProcessInfo {
        id: task.id,
        state: encode_task_state(&task.state) as usize,
        name: encode_task_name(&task.name),
    }
}

fn snapshot_process_info_v2(
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

fn collect_process_rows<T>(
    row_capacity: usize,
    mut build: impl FnMut(&crate::task::tcb::Task) -> T,
) -> Vec<T> {
    let mut rows = Vec::new();
    if row_capacity == 0 {
        return rows;
    }
    if let Some(sched) = super::SCHEDULER.lock().as_ref() {
        let cap = core::cmp::min(row_capacity, sched.tasks.len());
        rows.reserve(cap);
        for task in sched.tasks.values().take(cap) {
            rows.push(build(task));
        }
    }
    rows
}

/// The Fundamental Verbs of ViCell IPC (Hubris ABI + Lease System)
#[derive(Debug, Copy, Clone)]
pub enum Syscall {
    /// 0: Send (Blocking Message Send)
    Send {
        target: usize,
        msg_ptr: usize,
        msg_len: usize,
    },
    IpcSubmit {
        target: usize,
        ptr: usize,
        len: usize,
    },
    IpcTake {
        token: usize,
        ptr: usize,
        len: usize,
        status_ptr: usize,
    },
    IpcWait {
        ticks: u64,
    },
    IpcCancel {
        token: usize,
    },
    IpcCurrent,
    IpcReply {
        token: usize,
        ptr: usize,
        len: usize,
    },
    /// 4: TrySend (Non-blocking send — drops if target not in Recv)
    TrySend {
        target: usize,
        msg_ptr: usize,
        msg_len: usize,
    },
    /// 1: Recv (Blocking Message Receive)
    Recv {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        /// Receiver asked for a caller-identity trailer (a3 =
        /// `RECV_ATTEST_CALLER`). False for every pre-existing caller, which is
        /// why opting in cannot change any other receiver's buffer.
        attest_caller: bool,
    },
    /// 202: SendGather — send one IPC message from multiple non-contiguous buffers.
    SendGather {
        target: usize,
        iovec_ptr: usize,
        iovec_count: usize,
    },
    /// 203: RecvScatter — receive one IPC message into multiple non-contiguous buffers.
    RecvScatter {
        mask: usize,
        iovec_ptr: usize,
        iovec_count: usize,
    },
    /// 201: RecvTimeout — Recv with a monotonic-tick deadline (Phase 20).
    RecvTimeout {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        /// Deadline in kernel monotonic ticks from boot.  0 = non-blocking.
        deadline: u64,
    },
    /// 2: Reply (Unblocking Reply to Caller)
    Reply {
        caller: usize,
        result: usize,
    },
    /// 3: SetTimer (Wake up after ticks)
    SetTimer {
        deadline: usize,
    },
    /// 4: BorrowRead (Copy from Lease to Caller)
    BorrowRead {
        lease_id: usize,
        offset: usize,
        ptr: usize,
        len: usize,
    },
    /// 5: BorrowWrite (Copy from Caller to Lease)
    BorrowWrite {
        lease_id: usize,
        offset: usize,
        ptr: usize,
        len: usize,
    },
    /// 6: Lend (Create a Lease for Target Task)
    Lend {
        target: usize,
        ptr: usize,
        len: usize,
        flags: usize,
    },
    /// 7: TryRecv (Non-blocking Receive)
    TryRecv {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        attest_caller: bool,
    },
    /// 8: Spawn (Create new Task/Thread) - Returns Task ID
    Spawn {
        entry: usize,
        arg: usize,
    },
    /// ViSyscall 17: FutexWait — park until woken, the word changes, or the
    /// deadline elapses. `timeout_ticks == 0` blocks indefinitely.
    FutexWait {
        addr: usize,
        expected: u32,
        timeout_ticks: u64,
    },
    /// ViSyscall 18: FutexWake — wake up to `count` waiters on the caller's key.
    FutexWake {
        addr: usize,
        count: usize,
    },
    /// 11: Log (Debug Print)
    Log {
        msg_ptr: usize,
        msg_len: usize,
    },
    /// 12: Grant (Zero Copy)
    Grant {
        target: usize,
        ptr: usize,
        len: usize,
        flags: usize,
    },
    /// 13: Map (Zero Copy)
    Map {
        grant_id: usize,
    },
    /// 14: Exit (Terminate Process)
    Exit {
        code: usize,
    },
    /// 61: ForceExit — terminate another task by TID; non-blocking return to caller.
    ForceExit {
        tid: usize,
    },
    /// 204: NotifyOnExit — register the caller to be notified when `watched` dies.
    NotifyOnExit {
        watched: usize,
    },
    /// 205: RegisterService — register `tid` as the current provider of `service_id`.
    /// SpawnCap owns the namespace; fixed-ID self-registration uses narrower caps.
    RegisterService {
        service_id: u16,
        tid: usize,
    },
    /// 206: LookupService — resolve `service_id` to its live provider tid (open; 0 = none).
    LookupService {
        service_id: u16,
    },
    /// 429: LookupServiceBound — resolve `service_id` to its live provider binding
    /// `{ tid, cell_id, generation }` (open; 0 = none). The record is written whole
    /// or refused, never partially.
    LookupServiceBound {
        service_id: u16,
        out_ptr: usize,
        out_len: usize,
    },
    /// 207: Heartbeat — caller asserts liveness; (re)arm the hung-detection deadline
    /// `interval` ticks ahead (0 = disable).
    Heartbeat {
        interval: usize,
    },
    /// 6: Exec (Spawn from file)
    Exec {
        path_ptr: usize,
        path_len: usize,
    },
    /// 10: SpawnFromMem (Spawn from Memory buffer via Struct)
    SpawnFromMem {
        args_ptr: usize,
    },
    /// 12: SpawnFromPath (Spawn cell by filesystem path)
    /// ABI: path_ptr in a0, path_len in a1.
    SpawnFromPath {
        path_ptr: usize,
        path_len: usize,
    },
    /// 238: SpawnFromElf (Spawn cell from ELF bytes in a caller-owned Grant).
    /// ABI: a0=grant_id, a1=len, a2=path_hint_ptr, a3=path_hint_len.
    SpawnFromElf {
        grant_id: usize,
        len: usize,
        path_ptr: usize,
        path_len: usize,
    },
    /// 16: SpawnPinned — spawn cell pinned to a core (single-core: core_id must be 0).
    /// ABI: a0=path_ptr, a1=path_len, a2=priority: u8, a3=core_id: usize.
    SpawnPinned {
        path_ptr: usize,
        path_len: usize,
        priority: u8,
        core_id: usize,
    },
    /// 240: SpawnSetDirs — name the directory handles the caller's next spawn
    /// passes to its child. ABI: a0 = ptr to `ViSpawnDirHandles` (0 clears).
    SpawnSetDirs {
        carrier_ptr: usize,
    },
    /// 241: QueryDirHandles — read the kernel's record of a cell's inherited
    /// directory handles. ABI: a0 = cell_id, a1 = buf_ptr, a2 = buf_len.
    QueryDirHandles {
        cell_id: u64,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 244: attest the root task owning the current VFS receive principal.
    ResolveCellOwner {
        cell_id: u64,
        generation: u64,
        out_ptr: usize,
        out_len: usize,
    },
    /// 245: atomically attest and subscribe to the principal's root death.
    WatchCellOwner {
        cell_id: u64,
        generation: u64,
        out_ptr: usize,
        out_len: usize,
    },
    /// 246: idempotently cancel one VFS root-death subscription.
    CancelCellOwnerWatch {
        token: u64,
    },
    /// 247: RV32-safe owner attestation through a fixed request record.
    ResolveCellOwnerRecord {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 248: RV32-safe atomic owner watch through a fixed request record.
    WatchCellOwnerRecord {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 441: measured image identity of one live Cell generation (VFS only).
    ResolveCellImage {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 13: OpenCap — open a file and return a CapId.
    OpenCap {
        path_ptr: usize,
        path_len: usize,
    },
    /// 14: ReadCap — read bytes from a cap-backed file.
    ReadCap {
        cap_id: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 15: CloseCap — revoke a capability.
    CloseCap {
        cap_id: usize,
    },
    /// 228: SeekCap — seek a cap-backed file cursor.
    /// offset is transmitted as a raw i64 bit-pattern via usize (reinterpret_cast).
    SeekCap {
        cap_id: usize,
        offset: usize,
        whence: usize,
    },
    /// 229: WriteCap — write bytes into a cap-backed file at the current cursor.
    WriteCap {
        cap_id: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 230: StatCap — query file size via cap (cursor unchanged).
    StatCap {
        cap_id: usize,
    },
    /// 231: TruncateCap — truncate file to len bytes via cap.
    TruncateCap {
        cap_id: usize,
        len: usize,
    },
    /// 232: SyncCap — flush dirty pages to block device via cap.
    SyncCap {
        cap_id: usize,
    },
    /// 233: GrantDma — map PCIe DMA range into IOMMU for calling Cell.
    /// a0 = bdf: u32, a1 = phys: u64, a2 = size: usize.
    GrantDma {
        bdf: u32,
        phys: u64,
        size: usize,
    },
    /// 8: Wait (Wait for task)
    Wait {
        pid: usize,
    },
    /// 20: ShmAlloc
    ShmAlloc {
        size: usize,
    },
    /// 21: ShmMap
    ShmMap {
        handle: usize,
        target_pid: usize,
    },
    /// 30: GetProcs
    GetProcs {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 239: GetProcs2
    GetProcs2 {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 243: MemInfo
    MemInfo {
        out_ptr: usize,
        out_len: usize,
    },
    /// 430: TelemetrySnapshot (Phase 06 S6-1)
    TelemetrySnapshot {
        out_ptr: usize,
        out_len: usize,
        flags: usize,
        reserved: usize,
    },
    /// 431: QueryRetirement (Phase 09B K1)
    QueryRetirement {
        root_tid: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 432: LookupBlockRoute — copy route slot `index` as a `ViBlockRouteV1`.
    LookupBlockRoute {
        index: usize,
        out_ptr: usize,
        out_len: usize,
        reserved: usize,
    },
    /// 433: QueryMachineFacts — copy the current `ViMachineFactsV1`.
    QueryMachineFacts {
        out_ptr: usize,
        out_len: usize,
        a2: usize,
        a3: usize,
    },
    /// 434: QueryPcieFunction — copy the owned function's `ViPcieFunctionV1`.
    QueryPcieFunction {
        bdf: usize,
        out_ptr: usize,
        out_len: usize,
        reserved: usize,
    },
    /// 435: AllocPcieIrq — MSI-X/MSI vectors for the owned function.
    AllocPcieIrq {
        bdf: usize,
        want: usize,
        mode: usize,
        reserved: usize,
    },
    /// 436: SystemPower — reboot (a0 = 1) or poweroff (a0 = 2); SupervisorCap.
    SystemPower {
        action: usize,
        a1: usize,
        a2: usize,
        a3: usize,
    },
    /// 437: ExportDmaRange — lock caller-owned memory for a DMA export lease.
    ExportDmaRange {
        va: usize,
        len: usize,
        holder_service: usize,
        flags: usize,
    },
    /// 438: MapDmaExport — map a held export into the caller's VT-d domain.
    MapDmaExport {
        export_id: usize,
        bdf: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 439: RevokeDmaExport — owner or holder ends an export lease.
    RevokeDmaExport {
        export_id: usize,
        a1: usize,
        a2: usize,
        a3: usize,
    },
    CellSessionV1 {
        request_ptr: usize,
        request_len: usize,
        result_ptr: usize,
        result_capacity: usize,
    },
    /// 263: GetCpuCount — query number of online logical CPUs/harts.
    GetCpuCount,

    // --- Legacy / Compatibility Layer ---
    /// 100: Service Lookup (Find driver ID by name)
    ServiceLookup {
        name_ptr: usize,
        name_len: usize,
    },
    /// 101: Open (Path -> FD)
    Open {
        path_ptr: usize,
        path_len: usize,
    },
    /// 102: Read (FD, Buffer -> Bytes Read)
    Read {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 103: Close (FD)
    Close {
        fd: usize,
    },
    /// 105: ReadDir (Read Directory Entries)
    ReadDir {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 254: write one fixed-width caller-scoped descriptor metadata record.
    Fstat {
        fd: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 255: Rename (Old Path, New Path -> 0 on success)
    Rename {
        old_ptr: usize,
        old_len: usize,
        new_ptr: usize,
        new_len: usize,
    },
    /// 107: ChDir (Change Directory)
    ChDir {
        path_ptr: usize,
        path_len: usize,
    },
    /// 108: GetCwd (Get Current Directory)
    GetCwd {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 109: Write (FD, Buffer -> Bytes Written)
    Write {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 110: MkDir (Path)
    MkDir {
        path_ptr: usize,
        path_len: usize,
    },
    /// 111: Create (Path -> FD)
    /// 111: Create (Path -> FD)
    Create {
        path_ptr: usize,
        path_len: usize,
    },
    /// 104: Yield (Legacy)
    Yield,
    /// 106: Seek (FD, Offset, Whence)
    Seek {
        fd: usize,
        offset: isize,
        whence: usize,
    },
    /// 107: FileOp (Op, Arg1, Arg2)
    FileOp {
        op: usize,
        arg1: usize,
        arg2: usize,
    },
    /// 120: GetTime (Op)
    GetTime {
        op: usize,
    },
    /// 300: GpuFlush — copy cell pixel buffer to VirtIO GPU framebuffer.
    GpuFlush {
        data_ptr: usize,
        data_len: usize,
        xy: usize,
        wh: usize,
    },
    /// 218: AudioPlay — write raw PCM (S16LE/2ch/44100) to the VirtIO sound output.
    AudioPlay {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 219: CapRevoke — strip capabilities from a live cell at runtime.
    CapRevoke {
        target_tid: usize,
        cap_mask: u32,
    },
    /// 301: GpuCursor — set sprite (op=0) or move (op=1) the VirtIO GPU hardware cursor.
    GpuCursor {
        op: usize,
        data_ptr: usize,
        xy: usize,
        hot: usize,
    },
    /// 302: GpuGetResolution — return current GPU scanout (width, height) packed as (w<<32)|h.
    GpuGetResolution,
    /// 310: NetTx — transmit one Ethernet frame via the kernel VirtIO NIC.
    NetTx {
        frame_ptr: usize,
        frame_len: usize,
    },
    /// 311: NetRx — receive one pending Ethernet frame from the VirtIO NIC.
    NetRx {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 410: StateStash — save serialized cell state under `key` for hot-swap.
    StateStash {
        key: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 411: StateRestore — recover stashed state for `key` into the buffer.
    StateRestore {
        key: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 412: StateStashClear — delete the stash entry for `key`, freeing its slot.
    StateStashClear {
        key: usize,
    },
    /// 413: FreezeCell — freeze a running Cell. Requires SupervisorCap.
    FreezeCell {
        target_tid: usize,
    },
    /// 414: Resume a frozen Cell or atomically commit a hot-swap cutover.
    /// Plain resume: a0=target_tid, a1..a3=0. Cutover: a0=new_tid,
    /// a1=old_tid, a2=service_id, a3=0. Requires SupervisorCap.
    ResumeCell {
        target_tid: usize,
        source_tid: usize,
        service_id: usize,
        reserved: usize,
    },
    /// 415: KillCell — forcibly terminate a Cell. Requires SupervisorCap.
    KillCell {
        target_tid: usize,
        exit_code: u32,
    },
    /// 416: RegisterBlockDriver — bind the route of the block device the
    /// caller owns (`kind` 1 = PCI BDF, 2 = MMIO base; `reserved` must be 0).
    RegisterBlockDriver {
        kind: usize,
        id: usize,
        reserved: (usize, usize),
    },
    /// 417: RegisterNicDriver — announce caller as the active NIC driver.
    RegisterNicDriver,
    /// 423: RegisterUsbHidProducer — announce caller as the USB HID event producer.
    RegisterUsbHidProducer,
    /// 418: FindPcieDevice — locate a PCIe device by class/subclass/prog_if.
    FindPcieDevice {
        class: u8,
        subclass: u8,
        prog_if: u8,
        out_ptr: usize,
    },
    /// 424: FindPcieDeviceByVendor — locate a PCIe device by exact
    /// (vendor_id, device_id). The second half of the device-selection fix for
    /// the two Ethernet Driver Cells that share class triple `02:00:00`.
    FindPcieDeviceByVendor {
        vendor_id: u16,
        device_id: u16,
        out_ptr: usize,
    },
    /// 425: SerialPortInfo — report one declared serial port's identity and
    /// whether the kernel probed a 16550 register set there.
    SerialPortInfo {
        index: u32,
        out_ptr: usize,
    },
    /// 426: SerialWrite — write bytes to a usable serial port.
    SerialWrite {
        index: u32,
        ptr: usize,
        len: usize,
    },
    /// 427: SerialRead — poll bytes from a usable serial port.
    SerialRead {
        index: u32,
        ptr: usize,
        len: usize,
    },
    /// 428: SerialConfigure — set a usable serial port's baud rate (8N1 kept).
    SerialConfigure {
        index: u32,
        baud: u32,
    },
    /// 419: QueryHotswapReady — check whether `target_tid` has called sys_hotswap_ready().
    /// Returns 1 if ready, 0 if not yet, usize::MAX if tid is unknown.
    QueryHotswapReady {
        target_tid: usize,
    },
    /// 421: SpawnReplacement — supervisor-only spawn using the frozen task's recorded cap ceiling.
    SpawnReplacement {
        old_tid: usize,
        path_ptr: usize,
        path_len: usize,
    },
    /// 422: PauseService — hide the expected provider from service lookups while
    /// it remains runnable for snapshot IPC. Requires SupervisorCap.
    PauseService {
        service_id: u16,
        expected_tid: usize,
    },
    /// 401: HotSwapReady — signal that the new cell has finished deserializing
    /// state and is ready to receive IPC.  No arguments.
    HotSwapReady,
    /// 420: Snapshot — serialize all allocated physical frames to disk for warm boot.
    Snapshot,
    /// 500: BlkRead — read one 512-byte sector from the VirtIO block device.
    /// Not in `ViSyscall` enum to preserve `libs/api` stability (raw dispatch).
    BlkRead {
        sector: u64,
        buf_ptr: usize,
    },
    /// 501: BlkWrite — write one 512-byte sector to the VirtIO block device.
    BlkWrite {
        sector: u64,
        buf_ptr: usize,
    },
    /// 502: Shutdown — trigger SBI SRST system shutdown (S-mode → OpenSBI). No return.
    Shutdown,
    /// 503: BlkFlush — flush the VirtIO block device write cache to the backing image.
    BlkFlush,
    /// 504: BenchCounters — copy the `bench-counters` record to the caller.
    /// Raw opcode, compiled only with that feature (measurement builds).
    #[cfg(feature = "bench-counters")]
    BenchCounters {
        out_ptr: usize,
        out_len: usize,
    },
    /// 208: GrantAlloc — allocate n pages as a zero-copy Grant region.
    GrantAlloc {
        size: usize,
    },
    /// 209: GrantShare — share a Grant region with `target_cell` under `perm`.
    GrantShare {
        grant_id: usize,
        target_cell: usize,
        perm: usize,
    },
    /// 210: GrantSlice — return the user-space pointer for a Grant the caller owns/holds.
    /// A nonzero `expected_owner_tid` (ABI a2) must equal the Grant's owner;
    /// `flags` (ABI a3) bit 0 `LEASE` also publishes a holder lease on the Grant.
    GrantSlice {
        grant_id: usize,
        size_out_ptr: usize,
        expected_owner_tid: usize,
        flags: usize,
    },
    /// 211: GrantFree — unmap + deallocate a Grant region.
    GrantFree {
        grant_id: usize,
    },
    /// 212: BlkReadAsync — synchronous-but-zero-copy sector read into a Grant buffer.
    BlkReadAsync {
        sector: u64,
        grant_id: usize,
    },
    /// 213: RequestMmio — claim exclusive MMIO range for a peripheral Driver Cell.
    RequestMmio {
        base: usize,
        len: usize,
    },
    /// 442: ProbeVirtioMmio — read one VirtIO MMIO slot's identity unclaimed.
    ProbeVirtioMmio {
        base: usize,
        reserved: (usize, usize, usize),
    },
    /// 214: GetRandom — fill a caller buffer with VirtIO-RNG entropy bytes.
    GetRandom {
        buf_ptr: usize,
        len: usize,
    },
    /// 215: GrantRegister — allocate a persistent pre-pinned Grant buffer (lifetime = cell exit).
    GrantRegister {
        size: usize,
    },
    /// 216: GrantUnregister — explicitly release a registered buffer.
    GrantUnregister {
        reg_id: usize,
    },
    /// 217: WaitForEvent — block until `mask` bits fire or `deadline` ticks pass.
    /// `deadline = None` means block indefinitely.
    WaitForEvent {
        mask: u32,
        deadline: Option<u64>,
    },
    /// 249: synchronize an owned Grant subrange before device submission.
    GrantCacheSyncBegin {
        grant_id: usize,
        offset: usize,
        len: usize,
    },
    /// 250: synchronize device output and release one exact Grant pin.
    GrantCacheSyncComplete {
        token: usize,
    },
    /// 251: validate and expose a firmware-owned display framebuffer.
    RegisterDisplayFramebuffer {
        base: usize,
        size: usize,
        packed_dimensions: usize,
        pitch: usize,
    },
    /// 242: WaitCompletion — reserve a slot on the caller's completion queue for
    /// the source named by `mask`, wait for it to be filled, and write the
    /// result to `out_ptr`. `deadline = None` waits indefinitely.
    ///
    /// Carries a deadline for the same reason `WaitForEvent` does: its caller
    /// has maintenance work that must run on a timer even when no frame arrives.
    WaitCompletion {
        mask: u32,
        deadline: Option<u64>,
        out_ptr: usize,
    },

    // === Hypervisor (220-225) — HypervisorCap ZST-gated ===
    /// 220: CreateVm — allocate guest RAM + Stage-2 table → vm_id.
    CreateVm {
        guest_pages: usize,
    },
    /// 221: CreateVcpu — create a vCPU with `entry_pc` in `vm_id` → vcpu_id.
    CreateVcpu {
        vm_id: usize,
        entry_pc: u64,
    },
    /// 222: MapGuestMemory — map guest IPA range in `vm_id` Stage-2 table.
    MapGuestMemory {
        vm_id: usize,
        ipa: u64,
        size: usize,
        writable: bool,
    },
    /// 223: RunVcpu — world-switch into vCPU; write `ViVmExit` to `out_ptr`.
    RunVcpu {
        vm_id: usize,
        vcpu_id: usize,
        budget_ns: u64,
        out_ptr: usize,
    },
    /// 224: VcpuRegs — mode 0 reads and 1 writes 32 GP words; Pi mode 2
    /// reads CNTV_CTL_EL0/CNTV_CVAL_EL0 in words 0/1 and zeroes words 2..31.
    VcpuRegs {
        vm_id: usize,
        vcpu_id: usize,
        buf_ptr: usize,
        mode: usize,
    },
    /// 225: InjectIrq — inject GICv2 virtual interrupt (0 ≤ intid ≤ 1019).
    InjectIrq {
        vm_id: usize,
        vcpu_id: usize,
        intid: u32,
    },
    /// 226: WriteGuestMemory — copy `len` bytes from `src_ptr` to guest GPA.
    WriteGuestMemory {
        vm_id: usize,
        gpa: u64,
        src_ptr: usize,
        len: usize,
    },
    /// 227: ReadGuestMemory — copy `len` bytes from guest GPA into `dst_ptr`.
    ReadGuestMemory {
        vm_id: usize,
        gpa: u64,
        dst_ptr: usize,
        len: usize,
    },
    /// 234: WaitIrq — block until hardware IRQ `irq` fires (Driver Cell).
    /// ISR sets IRQ_PENDING[irq] (atomic, no lock); scheduler sweep wakes the task.
    /// `mmio_base`: VirtIO MMIO slot base for InterruptACK write, or 0 for non-VirtIO.
    WaitIrq {
        irq: u8,
        mmio_base: usize,
    },
    /// 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
    /// Populates the kernel BAR allowlist used by `sys_request_mmio`.
    /// Requires singleton PlatformCap.
    RegisterPcieBar {
        bdf: u32,
        base: usize,
        len: usize,
    },
    /// 236: RegisterPciDevice — Platform Cell announces a PCI device with class/BAR info.
    /// Populates kernel PCI_DEVICES so find_class() works without a kernel ECAM scan.
    /// Requires singleton PlatformCap (allowlist bit 53).
    RegisterPciDevice {
        bdf: u32,
        cls: u32,
        bar0_base: usize,
        bar0_size: usize,
    },
    /// 237: ReadLog — drain up to `max` bytes from the kernel user-log ring.
    /// ABI: a0 = buf_ptr, a1 = max → bytes_copied.
    /// Gated by allowlist bit 54 (ReadLog).
    ReadLog {
        buf_ptr: usize,
        max: usize,
        source: usize,
    },
    /// ViSyscall 9: SetTlsBase — set the caller's own user thread pointer and
    /// return the previous value. Self-only (no target parameter) and always
    /// permitted: it carries no authority.
    SetTlsBase {
        base: usize,
    },
    /// ViSyscall 19: PipeCreate — create a pipe and write both endpoint handles
    /// into the caller's 2-word buffer.
    PipeCreate {
        capacity: usize,
        out_ptr: usize,
    },
    /// ViSyscall 22: PipeRead — read up to `len` bytes, blocking (with an optional
    /// deadline) while the ring is empty and a writer end is still open.
    PipeRead {
        handle: usize,
        buf_ptr: usize,
        len: usize,
        timeout_ticks: u64,
    },
    /// ViSyscall 23: PipeWrite — write up to `len` bytes, blocking while the ring
    /// is full. A zero-length write closes the writer end.
    PipeWrite {
        handle: usize,
        buf_ptr: usize,
        len: usize,
        timeout_ticks: u64,
    },
    /// ViSyscall 24: PipeClose — close one endpoint this task owns.
    PipeClose {
        handle: usize,
    },
    /// ViSyscall 25: PipeShare — duplicate an endpoint into another task's table.
    /// Owner-only: a task cannot share an endpoint it does not hold.
    PipeShare {
        handle: usize,
        target_tid: usize,
    },
    /// ViSyscall 440: AnonMemory — manage Tier-2 large anonymous memory.
    AnonMemory {
        op: usize,
        addr: usize,
        len: usize,
        flags: usize,
    },
}

/// Return the syscall allowlist only for an active caller.
///
/// Task ID zero is the explicit kernel-context sentinel. Every nonzero caller
/// must still have a dispatch-visible, non-terminal task record; a missing or
/// terminal user task is denied rather than inheriting kernel authority.
fn syscall_allowlist_for(caller_id: usize) -> Option<u64> {
    if caller_id == 0 {
        return Some(u64::MAX);
    }

    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&caller_id))
        .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
        .map(|task| task.syscall_allowlist)
}

/// Map a kernel-internal `Syscall` variant to its `ViSyscall` representation
/// for allowlist bit lookup.
///
/// Returns `None` for:
/// - Raw block-I/O ops (500-503): ZST-gated via `BlockIoCap`, not filtered here.
/// - Legacy/internal variants (FutexWait, BorrowRead, Lend, …): no bit assigned.
/// - Always-permitted syscalls (Yield, Exit, …): `allowlist_bit()` returns `None`.
fn syscall_to_vi(syscall: &Syscall) -> Option<api::syscall::ViSyscall> {
    use api::syscall::ViSyscall as V;
    Some(match syscall {
        Syscall::IpcSubmit { .. } => V::IpcSubmit,
        Syscall::IpcTake { .. } => V::IpcTake,
        Syscall::IpcWait { .. } => V::IpcWait,
        Syscall::IpcCancel { .. } => V::IpcCancel,
        Syscall::IpcCurrent => V::IpcCurrent,
        Syscall::IpcReply { .. } => V::IpcReply,
        Syscall::Send { .. } => V::Send,
        Syscall::TrySend { .. } => V::TrySend,
        Syscall::Recv { .. } => V::Recv,
        Syscall::TryRecv { .. } => V::TryRecv,
        Syscall::RecvTimeout { .. } => V::RecvTimeout,
        Syscall::SendGather { .. } => V::SendGather,
        Syscall::RecvScatter { .. } => V::RecvScatter,
        Syscall::Reply { .. } => V::Reply,
        Syscall::Spawn { .. } => V::Spawn,
        Syscall::SpawnFromMem { .. } => V::SpawnFromMem,
        Syscall::SpawnFromPath { .. } => V::SpawnFromPath,
        Syscall::SpawnFromElf { .. } => V::SpawnFromElf,
        Syscall::SpawnPinned { .. } => V::SpawnPinned,
        Syscall::Wait { .. } => V::Wait,
        Syscall::SetTlsBase { .. } => V::SetTlsBase,
        Syscall::FutexWait { .. } => V::FutexWait,
        Syscall::FutexWake { .. } => V::FutexWake,
        Syscall::PipeCreate { .. } => V::PipeCreate,
        Syscall::PipeRead { .. } => V::PipeRead,
        Syscall::PipeWrite { .. } => V::PipeWrite,
        Syscall::PipeClose { .. } => V::PipeClose,
        Syscall::PipeShare { .. } => V::PipeShare,
        Syscall::CellSessionV1 { .. } => return None,
        Syscall::GetCpuCount => V::GetCpuCount,
        Syscall::Log { .. } => V::Log,
        Syscall::SetTimer { .. } => V::SetTimer,
        Syscall::ShmAlloc { .. } => V::ShmAlloc,
        Syscall::ShmMap { .. } => V::ShmMap,
        Syscall::GetProcs { .. } => V::GetProcs,
        Syscall::GetProcs2 { .. } => V::GetProcs2,
        Syscall::MemInfo { .. } => V::MemInfo,
        Syscall::TelemetrySnapshot { .. } => V::TelemetrySnapshot,
        Syscall::OpenCap { .. } => V::OpenCap,
        Syscall::ReadCap { .. } => V::ReadCap,
        Syscall::CloseCap { .. } => V::CloseCap,
        Syscall::SeekCap { .. } => V::SeekCap,
        Syscall::WriteCap { .. } => V::WriteCap,
        Syscall::StatCap { .. } => V::StatCap,
        Syscall::TruncateCap { .. } => V::TruncateCap,
        Syscall::SyncCap { .. } => V::SyncCap,
        Syscall::GrantDma { .. } => V::GrantDma,
        Syscall::Open { .. } => V::Open,
        Syscall::Read { .. } => V::Read,
        Syscall::Write { .. } => V::Write,
        Syscall::Close { .. } => V::Close,
        Syscall::ReadDir { .. } => V::ReadDir,
        Syscall::Fstat { .. } => V::Fstat,
        Syscall::Rename { .. } => V::Rename,
        Syscall::Seek { .. } => V::Seek,
        Syscall::FileOp { .. } => V::FileOp,
        Syscall::ChDir { .. } => V::Chdir,
        Syscall::GetCwd { .. } => V::Getcwd,
        Syscall::GetTime { .. } => V::GetTime,
        Syscall::GpuFlush { .. } => V::GpuFlush,
        Syscall::AudioPlay { .. } => V::AudioPlay,
        Syscall::GpuCursor { .. } => V::GpuCursor,
        Syscall::GpuGetResolution => V::GpuGetResolution,
        Syscall::NetTx { .. } => V::NetTx,
        Syscall::NetRx { .. } => V::NetRx,
        Syscall::HotSwapReady => V::HotSwapReady,
        Syscall::Snapshot => V::Snapshot,
        Syscall::StateStash { .. } => V::StateStash,
        Syscall::StateRestore { .. } => V::StateRestore,
        Syscall::StateStashClear { .. } => V::StateStashClear,
        Syscall::FreezeCell { .. } => V::FreezeCell,
        Syscall::ResumeCell { .. } => V::ResumeCell,
        Syscall::KillCell { .. } => V::KillCell,
        Syscall::QueryHotswapReady { .. } => V::QueryHotswapReady,
        Syscall::SpawnReplacement { .. } => V::SpawnReplacement,
        Syscall::PauseService { .. } => V::PauseService,
        Syscall::RegisterBlockDriver { .. } => V::RegisterBlockDriver,
        Syscall::RegisterNicDriver => V::RegisterNicDriver,
        Syscall::RegisterUsbHidProducer => V::RegisterUsbHidProducer,
        Syscall::FindPcieDevice { .. } => V::FindPcieDevice,
        Syscall::FindPcieDeviceByVendor { .. } => V::FindPcieDeviceByVendor,
        Syscall::QueryPcieFunction { .. } => V::QueryPcieFunction,
        Syscall::AllocPcieIrq { .. } => V::AllocPcieIrq,
        Syscall::SystemPower { .. } => V::SystemPower,
        Syscall::MapDmaExport { .. } => V::MapDmaExport,
        Syscall::SerialPortInfo { .. } => V::SerialPortInfo,
        Syscall::SerialWrite { .. } => V::SerialWrite,
        Syscall::SerialRead { .. } => V::SerialRead,
        Syscall::SerialConfigure { .. } => V::SerialConfigure,
        Syscall::Exec { .. } => V::Exec,
        Syscall::LookupService { .. } => V::LookupService,
        Syscall::LookupServiceBound { .. } => V::LookupServiceBound,
        Syscall::Heartbeat { .. } => V::Heartbeat,
        Syscall::GrantAlloc { .. } => V::GrantAlloc,
        Syscall::GrantShare { .. } => V::GrantShare,
        Syscall::GrantSlice { .. } => V::GrantSlice,
        Syscall::GrantFree { .. } => V::GrantFree,
        Syscall::BlkReadAsync { .. } => V::BlkReadAsync,
        Syscall::RequestMmio { .. } => V::RequestMmio,
        Syscall::ProbeVirtioMmio { .. } => V::ProbeVirtioMmio,
        Syscall::GetRandom { .. } => V::GetRandom,
        Syscall::GrantRegister { .. } => V::GrantRegister,
        Syscall::GrantUnregister { .. } => V::GrantUnregister,
        Syscall::WaitForEvent { .. } => V::WaitForEvent,
        Syscall::GrantCacheSyncBegin { .. } => V::GrantCacheSyncBegin,
        Syscall::GrantCacheSyncComplete { .. } => V::GrantCacheSyncComplete,
        Syscall::RegisterDisplayFramebuffer { .. } => V::RegisterDisplayFramebuffer,
        Syscall::WaitCompletion { .. } => V::WaitCompletion,
        Syscall::CreateVm { .. } => V::CreateVm,
        Syscall::CreateVcpu { .. } => V::CreateVcpu,
        Syscall::MapGuestMemory { .. } => V::MapGuestMemory,
        Syscall::RunVcpu { .. } => V::RunVcpu,
        Syscall::VcpuRegs { .. } => V::VcpuRegs,
        Syscall::InjectIrq { .. } => V::InjectIrq,
        Syscall::WriteGuestMemory { .. } => V::WriteGuestMemory,
        Syscall::ReadGuestMemory { .. } => V::ReadGuestMemory,
        Syscall::WaitIrq { .. } => V::WaitIrq,
        Syscall::RegisterPcieBar { .. } => V::RegisterPcieBar,
        Syscall::RegisterPciDevice { .. } => V::RegisterPciDevice,
        Syscall::ReadLog { .. } => V::ReadLog,
        // Always-permitted; allowlist_bit() returns None → filter is a no-op.
        Syscall::Yield
        | Syscall::Exit { .. }
        | Syscall::ForceExit { .. }
        | Syscall::NotifyOnExit { .. }
        | Syscall::QueryRetirement { .. }
        | Syscall::LookupBlockRoute { .. }
        | Syscall::QueryMachineFacts { .. }
        | Syscall::RegisterService { .. }
        | Syscall::CapRevoke { .. }
        // SpawnCap / VFS-provider gated at dispatch; see ViSyscall::allowlist_bit.
        | Syscall::SpawnSetDirs { .. }
        | Syscall::QueryDirHandles { .. }
        | Syscall::ResolveCellOwner { .. }
        | Syscall::WatchCellOwner { .. }
        | Syscall::CancelCellOwnerWatch { .. }
        | Syscall::ResolveCellOwnerRecord { .. }
        | Syscall::WatchCellOwnerRecord { .. }
        | Syscall::ResolveCellImage { .. }
        | Syscall::AnonMemory { .. }
        // 437/439: own memory / lease party is the dispatch gate (CP1 D-G9).
        | Syscall::ExportDmaRange { .. }
        | Syscall::RevokeDmaExport { .. } => return None,
        // Raw block-I/O (500-503): ZST BlockIoCap gated at dispatch.
        Syscall::BlkRead { .. }
        | Syscall::BlkWrite { .. }
        | Syscall::BlkFlush
        | Syscall::Shutdown => return None,
        // Legacy / internal variants without allowlist bits.
        _ => return None,
    })
}

/// Every task of `tid`'s Cell incarnation (root and threads), in a fixed
/// buffer: no allocation, bounded by the per-Cell thread cap.
fn incarnation_members(sched: &super::scheduler::Scheduler, tid: usize) -> incarnation_tids::Tids {
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

mod incarnation_tids {
    /// Root plus the per-Cell thread cap.
    const CAP: usize = super::super::scheduler::MAX_THREADS_PER_CELL + 1;

    pub(super) struct Tids {
        ids: [usize; CAP],
        len: usize,
    }

    impl Default for Tids {
        fn default() -> Self {
            Self {
                ids: [0; CAP],
                len: 0,
            }
        }
    }

    impl Tids {
        pub(super) fn push(&mut self, id: usize) {
            // The thread cap bounds an incarnation; a surplus id is a broken
            // invariant, and dropping it would skip a teardown.
            assert!(self.len < CAP, "Cell incarnation exceeds its thread cap");
            self.ids[self.len] = id;
            self.len += 1;
        }

        pub(super) fn iter(&self) -> core::slice::Iter<'_, usize> {
            self.ids[..self.len].iter()
        }
    }
}

/// One `CapRevoke` at a time: the pending mask on a root is a single word, and
/// two overlapping revokes must not clear each other's bits. A concurrent
/// caller gets `TryAgain`.
static REVOKE_IN_PROGRESS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

struct RevokeSerial;

impl RevokeSerial {
    fn acquire() -> Option<Self> {
        use core::sync::atomic::Ordering;
        REVOKE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for RevokeSerial {
    fn drop(&mut self) {
        REVOKE_IN_PROGRESS.store(false, core::sync::atomic::Ordering::Release);
    }
}

/// The revoke's mask published as pending on the target incarnation's root.
/// Dropping it (every early return, including a failed teardown) withdraws the
/// pending bits again; a committed revoke has already folded them into
/// `Task::rights`, so the clear is then a no-op.
struct PendingRevoke {
    root_tid: usize,
    incarnation: (types::CellId, u64),
    mask: u32,
}

impl Drop for PendingRevoke {
    fn drop(&mut self) {
        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
            if let Some(root) = sched.tasks.get_mut(&self.root_tid) {
                if (root.cell_id, root.cell_generation) == self.incarnation {
                    root.revoke_pending &= !self.mask;
                }
            }
        }
    }
}

/// Withdraw `cap_mask` from the target's Cell incarnation after its Class-2
/// teardown has finished: re-run the gate, then narrow the incarnation record
/// (`root_tid`'s `Task::rights`, captured before the teardown) that every
/// thread of the Cell is gated on. The record is narrowed even if the target
/// thread exited meanwhile; only a retired incarnation (root gone) has nothing
/// left to narrow. Returns whether the target itself is still present. Takes
/// `SCHEDULER` only.
fn commit_cap_revoke(
    caller_id: usize,
    target_tid: usize,
    root_tid: usize,
    incarnation: (types::CellId, u64),
    cap_mask: u32,
) -> Result<bool, SyscallError> {
    let mut sched_opt = super::SCHEDULER.lock();
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
fn cap_revoke_gate(
    sched: &mut super::scheduler::Scheduler,
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
fn revoke_mmio_classes(
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

/// The audit subject of `tid` (its Cell incarnation). Takes `SCHEDULER`;
/// callers that already hold it use `audit::Subject::of` directly.
fn audit_subject(tid: usize) -> crate::audit::Subject {
    super::SCHEDULER.lock().as_ref().map_or(
        crate::audit::Subject {
            tid: tid as u64,
            ..crate::audit::Subject::KERNEL
        },
        |sched| crate::audit::Subject::of(sched, tid),
    )
}

/// Audit a refused syscall against the caller's Cell incarnation.
fn audit_syscall_denied(caller_id: usize, detail: u32) {
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
    super::ipc_post_nonblock(0, target_tid, &msg).is_ok()
}

/// Dispatches a system call to the appropriate handler.
///
/// `caller_id` is the ID of the task invoking the syscall.
pub fn handle_syscall(caller_id: usize, syscall: Syscall) -> SyscallResult {
    #[cfg(cellos_p01_trace)]
    let mut trace = {
        let route = match &syscall {
            Syscall::SpawnFromElf { .. } => 1,
            Syscall::SpawnFromPath { .. } => 2,
            Syscall::SpawnPinned { .. } => 3,
            _ => 0,
        };
        if route == 0 {
            None
        } else {
            let parent = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|task| crate::p01_measurement::Owner::of(task));
            parent.and_then(|owner| crate::p01_measurement::ingress(owner, route))
        }
    };
    let result = handle_syscall_inner(
        caller_id,
        syscall,
        #[cfg(cellos_p01_trace)]
        &mut trace,
    );
    #[cfg(cellos_p01_trace)]
    {
        crate::p01_measurement::outcome(trace, result);
        crate::p01_measurement::safe_point();
    }
    result
}

fn handle_syscall_inner(
    caller_id: usize,
    syscall: Syscall,
    #[cfg(cellos_p01_trace)] trace: &mut Option<crate::p01_measurement::Attempt>,
) -> SyscallResult {
    let Some(allowed) = syscall_allowlist_for(caller_id) else {
        log::warn!("[kernel] syscall denied for non-live tid {}", caller_id);
        return Err(SyscallError::PermissionDenied);
    };

    let _staged_spawn_argv_cleanup = staged_spawn_argv_cleanup(&syscall, caller_id);
    #[cfg(cellos_p01_trace)]
    {
        // These non-fixture routes do not have historical per-object provenance
        // in this observer. Preserve the uncertainty before any source can lose it.
        let class = match &syscall {
            Syscall::GrantAlloc { .. }
            | Syscall::GrantShare { .. }
            | Syscall::GrantSlice { .. }
            | Syscall::GrantFree { .. }
            | Syscall::GrantRegister { .. }
            | Syscall::GrantUnregister { .. }
            | Syscall::GrantCacheSyncBegin { .. }
            | Syscall::GrantCacheSyncComplete { .. }
            | Syscall::ShmAlloc { .. }
            | Syscall::ShmMap { .. }
            | Syscall::Grant { .. }
            | Syscall::Map { .. } => Some(3),
            Syscall::GrantDma { .. } => Some(5),
            Syscall::CreateVm { .. }
            | Syscall::CreateVcpu { .. }
            | Syscall::MapGuestMemory { .. } => Some(8),
            _ => None,
        };
        if let Some(class) = class {
            if let Some(owner) = crate::p01_resources::owner_for_tid(caller_id) {
                crate::p01_resources::unknown(owner, class);
            }
        }
    }

    // Syscall allowlist enforcement: reject if this syscall's bit is not set in
    // the per-Cell bitset loaded from ELF section `__ViCell_syscalls`.
    // Cells without that section retain their TCB's `u64::MAX` allowlist.
    if let Some(vi) = syscall_to_vi(&syscall) {
        if let Some(bit) = vi.allowlist_bit() {
            if (allowed >> bit) & 1 == 0 {
                log::warn!(
                    "[kernel] syscall {:?} denied for tid {} (allowlist={:#018x})",
                    vi,
                    caller_id,
                    allowed
                );
                audit_syscall_denied(caller_id, bit as u32);
                return Err(SyscallError::PermissionDenied);
            }
        }
    }

    // Every public receive form ends the prior VFS request. Masked dependency
    // receives preserve the outer grant authority and bounded reply token.
    if let Syscall::Recv { mask, .. }
    | Syscall::RecvTimeout { mask, .. }
    | Syscall::TryRecv { mask, .. }
    | Syscall::RecvScatter { mask, .. } = &syscall
    {
        let dropped = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .and_then(|task| task.begin_receive_context(*mask))
        };
        finish_vfs_context_drop(caller_id, dropped);
        // Every receive form, masked or not, ends the caller's `GrantSlice`
        // leases: the holder has finished with the pointers it resolved.
        if crate::memory::pin::grant_leases_live() {
            free_released_grant_lease_frames(
                caller_id,
                crate::memory::pin::release_grant_holder_leases(caller_id),
            );
        }
    }
    end_grant_leases_at_send(caller_id, &syscall);

    match syscall {
        Syscall::CellSessionV1 {
            request_ptr,
            request_len,
            result_ptr,
            result_capacity,
        } => {
            const MAX: usize = api::cell_session::MAX_REQUEST_LEN;
            if !(api::cell_session::HEADER_LEN..=MAX).contains(&request_len)
                || result_capacity < api::cell_session::HEADER_LEN
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
            let _reply_pin =
                SessionOutputPin::reserve(caller_id, result_ptr, result_capacity, &view)?;
            view.read_into(request_ptr, &mut request[..request_len])
                .map_err(|_| SyscallError::InvalidInput)?;
            let header = api::cell_session::RequestHeaderV1::decode(&request[..request_len]);
            let mut result = [0u8; 1536];
            if let Ok(header) = header {
                let required = super::cell_session::required_result(
                    header.selector,
                    &request[api::cell_session::HEADER_LEN..request_len],
                );
                if result_capacity < required {
                    api::cell_session::ResultHeaderV1 {
                        selector: header.selector,
                        status: api::cell_session::Status::BufferTooSmall,
                        total_len: required as u32,
                        request_id: header.request_id,
                    }
                    .encode(&mut result)
                    .map_err(|_| SyscallError::InvalidInput)?;
                    view.write_bytes(result_ptr, &result[..api::cell_session::HEADER_LEN])
                        .map_err(|_| SyscallError::InvalidInput)?;
                    return Ok(0);
                }
            }
            let len =
                super::cell_session::dispatch(caller_id, &request[..request_len], &mut result);
            view.write_bytes(result_ptr, &result[..len])
                .map_err(|_| SyscallError::InvalidInput)?;
            // Only a reply that reached the caller may be acknowledged.
            super::cell_session::delivered(caller_id, &request[..request_len]);
            Ok(0)
        }
        // --- Hubris ABI Implementation ---
        Syscall::IpcSubmit { target, ptr, len } => {
            super::async_ipc::submit(caller_id, target, ptr, len).map_err(ipc_error)
        }
        Syscall::IpcTake {
            token,
            ptr,
            len,
            status_ptr,
        } => super::async_ipc::take(caller_id, token, ptr, len, status_ptr)
            .map(usize::from)
            .map_err(ipc_error),
        Syscall::IpcWait { ticks } => super::async_ipc::wait(caller_id, ticks)
            .map(usize::from)
            .map_err(ipc_error),
        Syscall::IpcCancel { token } => super::async_ipc::cancel(caller_id, token)
            .map(|()| 0)
            .map_err(ipc_error),
        Syscall::IpcCurrent => Ok(super::async_ipc::current(caller_id)),
        Syscall::IpcReply { token, ptr, len } => {
            // An explicit reply is the same holder-quiescence boundary as Send.
            // A stale/cancelled token may already be gone from the client, but
            // must still release only this exact current VFS request's lease.
            let vfs_release = if token != 0 && super::async_ipc::current(caller_id) == token {
                match current_vfs_grant_lookup(caller_id) {
                    VfsGrantLookup::Active(context) => Some(context),
                    VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
                }
            } else {
                None
            };
            let out = super::async_ipc::reply(caller_id, token, ptr, len)
                .map(|()| 0)
                .map_err(ipc_error);
            if let Some(context) = vfs_release {
                finish_vfs_send_release(caller_id, context.grant_owner, Some(context));
            }
            out
        }
        Syscall::Send {
            target,
            msg_ptr,
            msg_len,
        } => {
            crate::audit::log_event(
                crate::audit::AuditEvent::IpcSend,
                &crate::audit::encode_u32x2(caller_id as u32, target as u32),
            );
            let vfs_release = match current_vfs_grant_lookup(caller_id) {
                VfsGrantLookup::Active(context) => Some(context),
                VfsGrantLookup::NotVfs | VfsGrantLookup::MissingContext => None,
            };
            if let Some(outcome) =
                super::async_ipc::reply_current(caller_id, target, msg_ptr, msg_len)
            {
                let out = outcome.map(|()| 0).map_err(ipc_error);
                finish_vfs_send_release(caller_id, target, vfs_release);
                return out;
            }
            let res = super::ipc_send(caller_id, target, msg_ptr, msg_len);
            let out = match res {
                Ok(0) => Ok(0),
                Ok(1) => {
                    super::yield_cpu(); // Blocked
                    if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                        Ok(sched
                            .tasks
                            .get(&caller_id)
                            .and_then(|t| t.reply_value)
                            .unwrap_or(0))
                    } else {
                        Ok(0)
                    }
                }
                Err(super::IpcSendError::Backpressure) => Err(SyscallError::TryAgain),
                Err(super::IpcSendError::TargetGone) => Err(SyscallError::InvalidCommand),
                _ => Ok(0),
            };
            finish_vfs_send_release(caller_id, target, vfs_release);
            out
        }
        Syscall::TrySend {
            target,
            msg_ptr,
            msg_len,
        } => {
            // Non-blocking: deliver if target in Recv, else drop. Never blocks.
            // Target 0 is resolved to caller_id for self-wake without an extra tid lookup.
            let target_id = if target == 0 { caller_id } else { target };
            match super::ipc_try_send(caller_id, target_id, msg_ptr, msg_len) {
                Ok(()) => Ok(0), // delivered (or queued for input service / same-cell wake)
                Err(()) => Ok(usize::MAX), // dropped (target not ready or gone)
            }
        }
        Syscall::Recv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => {
            // Identity trailer, when requested, is written at each delivery point
            // AFTER the payload copy and AFTER the scheduler lock is dropped —
            // `attested_identity_of` takes that lock itself.
            let attest = |sender_tid: usize| {
                if attest_caller {
                    write_caller_identity(caller_id, buf_ptr, buf_len, sender_tid);
                }
            };

            let death_info = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().and_then(|sched| {
                    let t = sched.tasks.get_mut(&caller_id)?;
                    let owner_death = super::Task::owner_death_matches_receive_mask(mask)
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
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                let mut guard = super::SCHEDULER.lock();
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
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some(info))
                })
            };
            let (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            ) = match pending_msg_info {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(None) => {
                    // A receive masked to a peer that is already gone, with no
                    // message of its left to deliver, would park forever: the
                    // exit-time wake (scheduler `exit_task`) fired before this
                    // receive existed. Complete it as that death, reason 0 like
                    // NotifyOnExit's already-dead target.
                    if mask != 0 {
                        let peer_gone = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
                            sched.tasks.get(&mask).is_none_or(|peer| {
                                matches!(
                                    peer.state,
                                    super::tcb::TaskState::Retiring
                                        | super::tcb::TaskState::Terminated
                                )
                            })
                        });
                        if peer_gone {
                            if buf_len >= core::mem::size_of::<u64>() {
                                validate_user_buf(
                                    buf_ptr,
                                    core::mem::size_of::<u64>(),
                                    MAX_USER_BUF,
                                )?;
                                let view = caller_copy_view(caller_id)?;
                                view.write_bytes(buf_ptr, &0u64.to_ne_bytes())
                                    .map_err(|_| SyscallError::InvalidInput)?;
                            }
                            attest(mask);
                            return Ok(mask);
                        }
                    }
                    let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                    match res {
                        Ok(0) => {
                            super::yield_cpu();
                            // Peek the next event without removing it,
                            // copy payload, then commit the exact record.
                            let snap = {
                                let mut guard = super::SCHEDULER.lock();
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
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                                    super::wake_sender_token(sched, sender_tid, caller_id, header);
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
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            attest(sender_tid);
            Ok(sender_tid)
        }
        // ── Scatter/gather IPC ────────────────────────────────────────────────
        Syscall::SendGather {
            target,
            iovec_ptr,
            iovec_count,
        } => {
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
            super::ipc_post_nonblock(caller_id, target, &gathered)
                .map_err(|_| SyscallError::TryAgain)?;
            Ok(0)
        }
        Syscall::RecvScatter {
            mask,
            iovec_ptr,
            iovec_count,
        } => {
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
                let mut guard = super::SCHEDULER.lock();
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
                        super::async_ipc::dispatch(sched, caller_id, header);
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
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            Ok(sender_tid)
        }
        Syscall::RecvTimeout {
            mask,
            buf_ptr,
            buf_len,
            deadline,
        } => {
            // Queued death records first, exactly as the plain `Recv` arm delivers
            // them: a supervised loop that polls with a deadline (a supervisor with
            // backoff timers) must still see its children exit.
            if let Some(dead_tid) = take_queued_death(caller_id, mask, buf_ptr, buf_len)? {
                return Ok(dead_tid);
            }
            let mut vfs_context_drop = None;
            let pending_msg_info: Result<Option<_>, ()> = {
                let mut guard = super::SCHEDULER.lock();
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
                        super::async_ipc::dispatch(sched, caller_id, header);
                    }
                    Ok(Some(info))
                })
            };
            finish_vfs_context_drop(caller_id, vfs_context_drop);

            let (
                pos,
                sender_tid,
                sender_cell_id,
                sender_generation,
                wire_header,
                caller_view,
                data,
            ) = match pending_msg_info {
                Err(()) => return Err(SyscallError::OutOfMemory),
                Ok(None) => {
                    // Fast path: check for a pending message immediately.
                    let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                    match res {
                        Ok(0) => {
                            // Blocked with deadline:None — install the absolute deadline.
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                                    if let super::tcb::TaskState::Recv {
                                        deadline: ref mut d,
                                        ..
                                    } = task.state
                                    {
                                        *d = Some(deadline);
                                    }
                                }
                            }
                            // Yield so the scheduler runs other tasks and can fire the timeout.
                            super::yield_cpu();
                            let snap = {
                                let mut guard = super::SCHEDULER.lock();
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
                            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                                    super::wake_sender_token(sched, sender_tid, caller_id, header);
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
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                    super::wake_sender_token(sched, sender_tid, caller_id, header);
                }
            }
            Ok(sender_tid)
        }
        Syscall::TryRecv {
            mask,
            buf_ptr,
            buf_len,
            attest_caller,
        } => {
            // A queued owner death is delivered first, exactly as `Recv` does
            // for a wildcard receive: reason at the start of the buffer, zero
            // trailer for the dead (unattributable) root, dead root TID returned.
            // `NotifyOnExit` deaths stay out of `TryRecv` on purpose.
            let owner_death = {
                let mut guard = super::SCHEDULER.lock();
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
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                let mut guard = super::SCHEDULER.lock();
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
                        super::async_ipc::dispatch(sched, caller_id, header);
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
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
                        super::wake_sender_token(sched, sender_tid, caller_id, header);
                    }
                }
                return Ok(sender_tid);
            }

            // Non-blocking Recv (scan Sending tasks)
            let res = super::ipc_try_recv(caller_id, mask, buf_ptr, buf_len);
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
        Syscall::Spawn { entry, arg } => {
            // Build and fully inherit the child under one scheduler lock before
            // publication. A runnable thread must never observe Task defaults
            // (SAS, permissive allowlist, or zero PKU state) between construction
            // and its creator's authority transfer.
            match super::spawn_with_arg(caller_id, "thread", entry, arg) {
                Ok(tid) => Ok(tid),
                Err(ViError::OutOfMemory) => Err(SyscallError::TryAgain),
                Err(_) => Err(SyscallError::Unknown),
            }
        }
        Syscall::SetTlsBase { base } => {
            // Self-only by construction: the ABI has no target task parameter, so
            // this can only ever change the caller's own thread pointer, and the
            // base carries no authority — the kernel never dereferences it. A bad
            // value is contained by the cell's own tier (LBI on Tier 1, an MMU
            // fault on Tier 2).
            //
            // The previous value is returned so a cell can read its base back
            // without touching the register, which the kernel owns (a direct user
            // write is overwritten on the next resume).
            if let Some(task) = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
            {
                let previous = task.tls_base;
                super::tls::set_base(task, base);
                Ok(previous)
            } else {
                Err(SyscallError::PermissionDenied)
            }
        }
        Syscall::PipeCreate { capacity, out_ptr } => {
            // The ring is charged to the creating cell, so a pipe is not a way to
            // hold memory outside the per-cell quota.
            let owner_cell = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&caller_id))
                .map(|task| task.cell_id.0 as usize)
                .ok_or(SyscallError::PermissionDenied)?;
            let (read_handle, write_handle) = super::pipe::create(capacity, owner_cell)?;
            let mut out = [0u8; 16];
            let mut offset = 0usize;
            for word in [read_handle.to_raw(), write_handle.to_raw()] {
                let bytes = word.to_ne_bytes();
                out[offset..offset + bytes.len()].copy_from_slice(&bytes);
                offset += bytes.len();
            }
            if let Err(error) = write_user_slice(caller_id, out_ptr, &out[..offset], 64) {
                super::pipe::close_end(read_handle, caller_id);
                super::pipe::close_end(write_handle, caller_id);
                return Err(error);
            }
            let installed = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| {
                    #[cfg(cellos_p01_trace)]
                    let _semantic = crate::memory::heap::p01_owner::creation(
                        Some(crate::p01_resources::owner(task)),
                        crate::memory::heap::p01_owner::TASK_FIELDS,
                    );
                    task.pipe_handles.insert(read_handle);
                    task.pipe_handles.insert(write_handle);
                })
                .is_some();
            if !installed {
                super::pipe::close_end(read_handle, caller_id);
                super::pipe::close_end(write_handle, caller_id);
                return Err(SyscallError::PermissionDenied);
            }
            Ok(0)
        }
        Syscall::PipeRead {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => {
            const CHUNK: usize = 512;
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            if handle.end != super::pipe::PipeEnd::Read || !caller_owns_pipe(caller_id, handle) {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            if len == 0 {
                return Ok(0);
            }
            let deadline = if timeout_ticks == 0 {
                None
            } else {
                Some(super::system_ticks() as u64 + timeout_ticks)
            };
            let count = len.min(CHUNK);
            let mut scratch = [0u8; CHUNK];
            loop {
                match super::pipe::try_read(handle, &mut scratch[..count], caller_id) {
                    Some(super::pipe::ReadOutcome::Read(read)) => {
                        write_user_slice(caller_id, buf_ptr, &scratch[..read], MAX_USER_BUF)?;
                        return Ok(read);
                    }
                    // EOF: the ring is empty and no writer end remains.
                    Some(super::pipe::ReadOutcome::Eof) => return Ok(0),
                    Some(super::pipe::ReadOutcome::Empty) => {
                        if deadline.is_some_and(|d| super::system_ticks() as u64 >= d) {
                            return Ok(0);
                        }
                        // Enqueue and park under one lock: a writer that drains the
                        // queue takes `SCHEDULER` to wake us, so either it sees our
                        // entry and wakes a parked task, or we see its bytes on the
                        // next `try_read`. Lock order stays SCHEDULER -> PIPES.
                        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                            match sched.tasks.get_mut(&caller_id) {
                                Some(task) => {
                                    super::pipe::park_reader(handle, caller_id);
                                    task.state =
                                        super::tcb::TaskState::PipeRead { handle, deadline };
                                }
                                None => return Err(SyscallError::PermissionDenied),
                            }
                        }
                        super::yield_cpu();
                    }
                    None => return Err(SyscallError::InvalidInput),
                }
            }
        }
        Syscall::PipeWrite {
            handle,
            buf_ptr,
            len,
            timeout_ticks,
        } => {
            const CHUNK: usize = 512;
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            if handle.end != super::pipe::PipeEnd::Write || !caller_owns_pipe(caller_id, handle) {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            if len == 0 {
                // A zero-length write closes the writer end: readers then observe
                // EOF once they have drained the ring.
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(task) = sched.tasks.get_mut(&caller_id) {
                        task.pipe_handles.remove(&handle);
                    }
                }
                super::pipe::close_end(handle, caller_id);
                return Ok(0);
            }
            let deadline = if timeout_ticks == 0 {
                None
            } else {
                Some(super::system_ticks() as u64 + timeout_ticks)
            };
            let count = len.min(CHUNK);
            let mut scratch = [0u8; CHUNK];
            {
                let view = caller_copy_view_for(caller_id)?;
                view.read_into(buf_ptr, &mut scratch[..count])
                    .map_err(|_| SyscallError::InvalidInput)?;
            }
            loop {
                match super::pipe::try_write(handle, &scratch[..count], caller_id) {
                    Some(super::pipe::WriteOutcome::Wrote(written)) => return Ok(written),
                    // No reader end remains: the stream is broken, not full.
                    Some(super::pipe::WriteOutcome::NoReader) => {
                        return Err(SyscallError::BrokenPipe);
                    }
                    Some(super::pipe::WriteOutcome::Full) => {
                        if deadline.is_some_and(|d| super::system_ticks() as u64 >= d) {
                            return Ok(0);
                        }
                        if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                            match sched.tasks.get_mut(&caller_id) {
                                Some(task) => {
                                    super::pipe::park_writer(handle, caller_id);
                                    task.state =
                                        super::tcb::TaskState::PipeWrite { handle, deadline };
                                }
                                None => return Err(SyscallError::PermissionDenied),
                            }
                        }
                        super::yield_cpu();
                    }
                    None => return Err(SyscallError::InvalidInput),
                }
            }
        }
        Syscall::PipeClose { handle } => {
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            let owned = super::SCHEDULER
                .lock()
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| task.pipe_handles.remove(&handle))
                .unwrap_or(false);
            if !owned {
                audit_syscall_denied(caller_id, handle.to_raw() as u32);
                return Err(SyscallError::PermissionDenied);
            }
            super::pipe::close_end(handle, caller_id);
            Ok(0)
        }
        Syscall::PipeShare { handle, target_tid } => {
            let Some(handle) = super::pipe::PipeHandle::from_raw(handle) else {
                return Err(SyscallError::InvalidInput);
            };
            // Owner-only: a task cannot hand out an endpoint it does not hold. That
            // ownership rule is the whole authorization model for pipes. The
            // endpoint count and target table change together while SCHEDULER is
            // held, preserving the documented SCHEDULER -> PIPES lock order.
            let mut sched = super::SCHEDULER.lock();
            let sched = sched.as_mut().ok_or(SyscallError::Unknown)?;
            let owned = sched
                .tasks
                .get(&caller_id)
                .is_some_and(|task| task.pipe_handles.contains(&handle));
            if !owned {
                crate::audit::log_event(
                    crate::audit::AuditEvent::SyscallDenied,
                    &crate::audit::encode_subject(
                        crate::audit::Subject::of(sched, caller_id),
                        handle.to_raw() as u32,
                        0,
                    ),
                );
                return Err(SyscallError::PermissionDenied);
            }
            let already_granted = sched
                .tasks
                .get(&target_tid)
                .ok_or(SyscallError::InvalidInput)?
                .pipe_handles
                .contains(&handle);
            if already_granted {
                return Ok(0);
            }
            if !super::pipe::duplicate_end(handle) {
                return Err(SyscallError::InvalidInput);
            }
            let target = sched
                .tasks
                .get_mut(&target_tid)
                .expect("target task remained live while holding SCHEDULER");
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                Some(crate::p01_resources::owner(target)),
                crate::memory::heap::p01_owner::TASK_FIELDS,
            );
            target.pipe_handles.insert(handle);
            Ok(0)
        }
        Syscall::Wait { pid } => {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
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
            super::yield_cpu();
            // Resume with exit code published by the quiescent zombie reaper.
            if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                return Ok(sched
                    .tasks
                    .get(&caller_id)
                    .and_then(|task| task.reply_value)
                    .unwrap_or(0));
            }
            Ok(0)
        }
        Syscall::ShmAlloc { size } => {
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
        Syscall::ShmMap { handle, target_pid } => {
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
            let flags = Flags::VALID
                | Flags::READ
                | Flags::WRITE
                | Flags::USER
                | Flags::ACCESSED
                | Flags::DIRTY;

            let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
            if let Some(allocator) = frame_guard.as_mut() {
                if crate::memory::paging::map_page(allocator, vaddr, frame, Flags::from_bits(flags))
                    .is_ok()
                {
                    return Ok(vaddr);
                }
            }
            Err(SyscallError::Unknown)
        }
        Syscall::FutexWait {
            addr,
            expected,
            timeout_ticks,
        } => {
            // The wait parks the caller inside `futex::wait`; the outcome
            // (woken / value mismatch / timed out) is the syscall's return value,
            // so it needs no error encoding. An invalid address is a recoverable
            // error, never a kernel fault.
            super::futex::wait(caller_id, addr, expected, timeout_ticks)
        }
        Syscall::FutexWake { addr, count } => {
            if let Ok(n) = super::futex::wake(caller_id, addr, count) {
                Ok(n)
            } else {
                Err(SyscallError::Unknown) // Should not fail typically
            }
        }
        Syscall::Log { msg_ptr, msg_len } => {
            if let Ok(msg) = read_user_string(caller_id, msg_ptr, msg_len, MAX_LOG_MSG) {
                #[cfg(cellos_p01_trace)]
                {
                    let owner = super::SCHEDULER
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
        Syscall::Grant {
            target,
            ptr,
            len,
            flags,
        } => super::ipc_grant(caller_id, target, ptr, len, flags as u32)
            .map_err(|_| SyscallError::PermissionDenied),
        Syscall::Map { grant_id } => {
            super::ipc_map(caller_id, grant_id).map_err(|_| SyscallError::PermissionDenied)
        }
        Syscall::Exit { code } => {
            // TID zero is the explicit kernel-context sentinel. It has no Cell
            // generation to retire; a kernel scheduling point remains explicit
            // rather than fabricating a user-exit record.
            if caller_id == 0 {
                super::yield_cpu();
                return Ok(0);
            }

            #[cfg(feature = "bench-counters")]
            crate::bench_counters::note_exit(caller_id);

            // Do not inspect the task table, log, audit, or call
            // `Scheduler::exit_task` while the exiting Cell is still charged.
            // A root exit can retire every member and grow scheduler vectors;
            // the fixed record is therefore the entire victim-attributed path.
            let exit = super::hart_local::DeferredExit {
                tid: caller_id,
                cell_id: super::hart_local::current_cell_id(),
                generation: super::hart_local::current_cell_generation(),
                code,
            };
            super::hart_local::defer_exit(exit);
            super::hart_local::set_current_cell_id(0);

            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            super::retirement_selftest::observe_exit_deferred_record_commit(exit);

            // `yield_cpu` consumes the fixed record under normal scheduler
            // locking and routes roots through quiescent retirement. A worker
            // still reaches the task-local branch of `exit_task`.
            super::yield_cpu();
            Ok(0)
        }

        Syscall::ForceExit { tid } => {
            // Self-kill rejected before touching the scheduler (cheap early check).
            if tid == caller_id {
                return Err(SyscallError::InvalidCommand);
            }

            // Single SCHEDULER lock: SpawnCap gate + all cleanup in one scope.
            // Two separate acquisitions would create a TOCTOU window where the target
            // self-exits between them, causing a spurious InvalidCommand return.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                // Gate 1: only SpawnCap holders (init/shell) may force-terminate tasks.
                let has_spawn =
                    sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
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

        Syscall::CapRevoke {
            target_tid,
            cap_mask,
        } => {
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
                let mut sched_opt = super::SCHEDULER.lock();
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
                &crate::audit::encode_subject(
                    audit_subject(caller_id),
                    target_tid as u32,
                    cap_mask,
                ),
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
                log::warn!(
                    "[kernel] CapRevoke: task {target_tid} not notified (exited or mailbox full)"
                );
            }
            if root_tid != target_tid && !send_cap_revoked_event(root_tid, cap_mask) {
                log::warn!(
                    "[kernel] CapRevoke: Cell root {root_tid} not notified (exited or mailbox full)"
                );
            }

            Ok(0)
        }

        Syscall::NotifyOnExit { watched } => {
            // Privileged: only SpawnCap holders (supervisors like init) may watch
            // arbitrary tasks — same authority gate as ForceExit. The watcher's
            // next Recv returns `watched` when it dies (see exit_task delivery).
            //
            // Race: the watched task may have already exited before this call.
            // Subscribe while SCHEDULER still proves the target is live; exit_task
            // takes DEATH_SUBSCRIBERS under the same outer lock, so publication and
            // delivery are atomic with respect to task death.
            {
                let mut sched_opt = super::SCHEDULER.lock();
                let sched = match sched_opt.as_mut() {
                    Some(s) => s,
                    None => return Ok(0),
                };
                let has_spawn =
                    sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).spawn;
                if !has_spawn {
                    return Err(SyscallError::PermissionDenied);
                }
                if sched.tasks.get(&watched).is_some_and(|task| {
                    !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
                }) {
                    super::scheduler::subscribe_death(watched, caller_id);
                } else {
                    // Task already dead — queue synthetic death so watcher never stalls.
                    if let Some(wt) = sched.tasks.get_mut(&caller_id) {
                        super::scheduler::queue_pending_death(wt, watched, 0);
                    }
                }
            }
            Ok(0)
        }

        Syscall::QueryRetirement {
            root_tid,
            out_ptr,
            out_len,
        } => {
            // Admitted past the allowlist; the verdict table is the gate: a
            // death subscriber of that root at its retirement, or a supervisor.
            if out_len != api::retirement::RETIREMENT_RECORD_LEN {
                return Err(SyscallError::InvalidInput);
            }
            let supervisor = caller_has_supervisor(caller_id);
            let record = crate::cell::retirement_verdict::query(caller_id, supervisor, root_tid)
                .map_err(|error| match error {
                    crate::cell::retirement_verdict::QueryError::NotFound => {
                        SyscallError::FileNotFound
                    }
                    crate::cell::retirement_verdict::QueryError::Denied => {
                        SyscallError::PermissionDenied
                    }
                })?;
            write_user_slice(
                caller_id,
                out_ptr,
                &record.encode(),
                api::retirement::RETIREMENT_RECORD_LEN,
            )?;
            Ok(api::retirement::RETIREMENT_RECORD_LEN)
        }

        Syscall::LookupBlockRoute {
            index,
            out_ptr,
            out_len,
            reserved,
        } => {
            // Admitted past the allowlist; the VFS (BlockIoCap) and init
            // (SpawnCap) are the route readers.
            if !(caller_has_block_io(caller_id) || caller_has_spawn(caller_id)) {
                return Err(SyscallError::PermissionDenied);
            }
            let len = api::block_route::BLOCK_ROUTE_RECORD_LEN;
            if out_len != len || reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = crate::task::drivers::driver_cell::block_route_record(index)
                .ok_or(SyscallError::FileNotFound)?;
            write_user_slice(caller_id, out_ptr, &record.encode(), len)?;
            Ok(len)
        }

        Syscall::QueryMachineFacts {
            out_ptr,
            out_len,
            a2,
            a3,
        } => {
            // Admitted past the allowlist; init (SpawnCap) is the reader.
            if !caller_has_spawn(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if out_len != api::machine_facts::MACHINE_FACTS_CAPACITY || a2 != 0 || a3 != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = crate::machine_facts::query().ok_or(SyscallError::FileNotFound)?;
            write_user_slice(caller_id, out_ptr, &record, record.len())?;
            Ok(record.len())
        }

        Syscall::RegisterService { service_id, tid } => {
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
            if service_id == api::syscall::service::HOTSWAP_DEMO
                && tid == 0
                && caller_has_spawn(caller_id)
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
                crate::task::drivers::driver_cell::publish_role_or_rollback(
                    caller_id,
                    crate::task::drivers::driver_cell::register_gpu_driver,
                    || {
                        crate::cell::service_registry::register(
                            service_id, caller_id, cell_id, generation,
                        )
                    },
                    crate::task::drivers::driver_cell::deregister_gpu_driver,
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
            let registrar = if caller_is_trusted_init(caller_id) || caller_has_supervisor(caller_id)
            {
                crate::cell::service_registry::Registrar::Namespace
            } else {
                crate::cell::service_registry::Registrar::Spawner(caller_id)
            };
            // The liveness check and the identity capture share one scheduler lock,
            // so a provider cannot be recorded with a binding that was already stale
            // when it was written.
            let provider = super::SCHEDULER.lock().as_ref().and_then(|scheduler| {
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
                Err(crate::cell::service_registry::Refusal::Rejected) => {
                    Err(SyscallError::InvalidInput)
                }
            }
        }
        Syscall::LookupService { service_id } => {
            // Open to all cells: resolve the live provider tid (0 = none registered),
            // so a client reconnects transparently after the supervisor respawns a
            // service. The dynamic replacement for the boot-order `ServiceLookup` hardcode.
            Ok(crate::cell::service_registry::lookup(service_id).unwrap_or(0))
        }
        Syscall::LookupServiceBound {
            service_id,
            out_ptr,
            out_len,
        } => {
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
            let Some((tid, cell_id, generation)) =
                crate::cell::service_registry::lookup_bound(service_id)
            else {
                return Ok(0);
            };
            let binding =
                api::service_binding::ServiceBinding::new(tid as u64, cell_id, generation);
            write_user_slice(caller_id, out_ptr, &binding.to_bytes(), MAX_USER_BUF)?;
            Ok(api::service_binding::SERVICE_BINDING_LEN)
        }
        Syscall::Heartbeat { interval } => {
            // Open: a cell asserts its own liveness. Arms a deadline `interval` ticks
            // ahead; `pick_next` terminates the cell as HUNG if it lapses. interval=0
            // disables. Self-targeted only — a cell can only (re)arm its OWN deadline.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    t.heartbeat_deadline = if interval == 0 {
                        None
                    } else {
                        Some(super::system_ticks() as u64 + interval as u64)
                    };
                }
            }
            Ok(0)
        }

        Syscall::Reply { caller: _, result } => {
            super::ipc_reply(caller_id, result).map_err(|_| SyscallError::InvalidCommand)
        }

        Syscall::Lend {
            target,
            ptr,
            len,
            flags,
        } => super::ipc_lend(caller_id, target, ptr, len, flags as u32)
            .map_err(|_| SyscallError::PermissionDenied),

        Syscall::BorrowRead {
            lease_id,
            offset,
            ptr,
            len,
        } => super::ipc_borrow_read(caller_id, lease_id, offset, ptr, len)
            .map_err(|_| SyscallError::PermissionDenied),
        Syscall::BorrowWrite {
            lease_id,
            offset,
            ptr,
            len,
        } => super::ipc_borrow_write(caller_id, lease_id, offset, ptr, len)
            .map_err(|_| SyscallError::PermissionDenied),

        // --- Legacy Implementation ---
        Syscall::Yield => {
            super::yield_cpu();
            Ok(0)
        }
        Syscall::ServiceLookup { name_ptr, name_len } => {
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
        Syscall::Open { path_ptr, path_len } => {
            let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            if let Ok(fd) = super::file_open(caller_id, &path) {
                return Ok(fd);
            }
            Err(SyscallError::FileNotFound)
        }
        Syscall::Read {
            fd,
            buf_ptr,
            buf_len,
        } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_bytes = super::file_read(fd, &mut kbuf);
            // A committed child's endpoint failure is explicit, never a short count.
            if read_bytes == usize::MAX {
                return Err(SyscallError::PermissionDenied);
            }
            if read_bytes > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
            }
            Ok(read_bytes)
        }
        Syscall::Close { fd } => {
            super::file_close(fd);
            Ok(0)
        }
        Syscall::ReadDir {
            fd,
            buf_ptr,
            buf_len,
        } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_bytes =
                super::file_readdir(fd, &mut kbuf).map_err(|_| SyscallError::Unknown)?;
            if read_bytes > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
            }
            Ok(read_bytes)
        }
        Syscall::Fstat {
            fd,
            out_ptr,
            out_len,
        } => {
            let len = api::syscall::VI_FSTAT_V1_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
            preflight_user_output(caller_id, out_ptr, len)?;

            let metadata = super::file_fstat(caller_id, fd).map_err(|_| SyscallError::Unknown)?;
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &metadata as *const api::syscall::ViFstatV1 as *const u8,
                    len,
                )
            };
            write_user_slice(caller_id, out_ptr, bytes, MAX_USER_BUF)?;
            Ok(len)
        }
        Syscall::Rename {
            old_ptr,
            old_len,
            new_ptr,
            new_len,
        } => {
            let old_path = read_user_string(caller_id, old_ptr, old_len, MAX_LOG_MSG)?;
            let new_path = read_user_string(caller_id, new_ptr, new_len, MAX_LOG_MSG)?;
            let vfs_tid = crate::cell::service_registry::lookup(api::syscall::service::VFS)
                .ok_or(SyscallError::InvalidCommand)?;
            let request = api::ipc::VfsRequest::Rename {
                old: &old_path,
                new: &new_path,
            };
            let mut request_buf = [0u8; api::ipc::IPC_BUF_SIZE];
            let encoded = api::ipc::encode(&request, &mut request_buf)
                .map_err(|_| SyscallError::InvalidCommand)?;
            match super::ipc_send_kernel(caller_id, vfs_tid, encoded) {
                Ok(0) => {}
                Ok(1) => super::yield_cpu(),
                Err(super::IpcSendError::Backpressure) => return Err(SyscallError::TryAgain),
                Ok(_) => return Err(SyscallError::InvalidCommand),
                Err(super::IpcSendError::TargetGone) => return Err(SyscallError::InvalidCommand),
            }

            let mut response_buf = [0u8; api::ipc::IPC_BUF_SIZE];
            loop {
                let response = super::ipc_recv_kernel(caller_id, vfs_tid, &mut response_buf)
                    .map_err(|_| SyscallError::InvalidCommand)?;
                match response {
                    Some((_, len)) => {
                        return match api::ipc::decode::<api::ipc::VfsResponse>(&response_buf[..len])
                        {
                            Ok(api::ipc::VfsResponse::Ok) => Ok(0),
                            _ => Err(SyscallError::PermissionDenied),
                        };
                    }
                    None => super::yield_cpu(),
                }
            }
        }
        // Syscall::Remove removed
        Syscall::ChDir { path_ptr, path_len } => {
            let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            if super::file_chdir(caller_id, &path).is_ok() {
                return Ok(0);
            }
            Err(SyscallError::FileNotFound)
        }
        Syscall::GetCwd { buf_ptr, buf_len } => {
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            if let Ok(len) = super::file_getcwd(caller_id, &mut kbuf) {
                write_user_slice(caller_id, buf_ptr, &kbuf[..len], MAX_USER_BUF)?;
                return Ok(len);
            }
            Err(SyscallError::BufferTooSmall)
        }
        Syscall::AnonMemory {
            op,
            addr,
            len,
            flags,
        } => {
            #[cfg(target_arch = "x86_64")]
            {
                let is_domain = {
                    let guard = super::SCHEDULER.lock();
                    guard
                        .as_ref()
                        .and_then(|sched| sched.tasks.get(&caller_id))
                        .map(|t| matches!(t.address_space, super::tcb::TaskAddressSpace::Domain(_)))
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
                let _ = (op, addr, len, flags);
                Ok(api::anon_memory::ERR_NOT_SUPPORTED)
            }
        }
        Syscall::Write {
            fd,
            buf_ptr,
            buf_len,
        } => {
            let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
            let written = super::file_write(fd, &bytes);
            if written == usize::MAX {
                return Err(SyscallError::PermissionDenied);
            }
            Ok(written)
        }
        Syscall::MkDir { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::PermissionDenied)
        }
        Syscall::Exec { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::NotSupported)
        }
        Syscall::SpawnFromPath { path_ptr, path_len } => {
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
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
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            #[cfg(cellos_p01_trace)]
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

        Syscall::SpawnSetDirs { carrier_ptr } => {
            if !caller_has_spawn(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if carrier_ptr == 0 {
                crate::task::dir_inherit::clear_staged(caller_id);
                return Ok(0);
            }
            let carrier_size = core::mem::size_of::<api::dir_handles::ViSpawnDirHandles>();
            let bytes = read_user_slice(caller_id, carrier_ptr, carrier_size, MAX_USER_BUF)?;
            let carrier = unsafe {
                core::ptr::read_unaligned(
                    bytes.as_ptr() as *const api::dir_handles::ViSpawnDirHandles
                )
            };
            let set = api::dir_handles::DirHandleSet::from_carrier(&carrier).map_err(|e| {
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

        Syscall::QueryDirHandles {
            cell_id,
            buf_ptr,
            buf_len,
        } => {
            if !may_query_dir_handles(caller_id, cell_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let len = api::dir_attestation::DIR_ATTESTATION_LEN;
            if buf_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let record = crate::task::dir_inherit::attestation_for(cell_id)
                .ok_or(SyscallError::InvalidInput)?
                .to_bytes();
            write_user_slice(caller_id, buf_ptr, &record, MAX_USER_BUF)?;
            Ok(len)
        }

        Syscall::ResolveCellOwner {
            cell_id,
            generation,
            out_ptr,
            out_len,
        } => {
            let len = api::cell_owner::CELL_OWNER_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let owner = {
                let guard = super::SCHEDULER.lock();
                let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
                let owner = sched
                    .resolve_live_cell_owner(types::CellId(cell_id), generation)
                    .ok_or(SyscallError::PermissionDenied)?;
                let vfs_principal = sched.tasks.get(&caller_id).is_some_and(|task| {
                    crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize)
                        && task.current_caller_cell_id == cell_id
                        && task.current_caller_cell_generation == generation
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

        Syscall::WatchCellOwner {
            cell_id,
            generation,
            out_ptr,
            out_len,
        } => {
            let len = api::cell_owner::CELL_OWNER_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let (owner, token) = super::SCHEDULER
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

        Syscall::ResolveCellOwnerRecord {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => {
            let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
            let len = api::cell_owner::CELL_OWNER_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let owner = {
                let guard = super::SCHEDULER.lock();
                let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
                let allowed = sched.tasks.get(&caller_id).is_some_and(|task| {
                    crate::fast_ipc::is_registered_vfs_cell(task.cell_id.0 as usize)
                        && task.current_caller_cell_id == request.cell_id
                        && task.current_caller_cell_generation == request.generation
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

        Syscall::ResolveCellImage {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => {
            let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
            let len = api::cell_image::CELL_IMAGE_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let image = {
                let guard = super::SCHEDULER.lock();
                let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
                super::cell_image::resolve_for_caller(
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

        Syscall::WatchCellOwnerRecord {
            request_ptr,
            request_len,
            out_ptr,
            out_len,
        } => {
            let request = read_cell_owner_request(caller_id, request_ptr, request_len)?;
            let len = api::cell_owner::CELL_OWNER_LEN;
            if out_len < len {
                return Err(SyscallError::BufferTooSmall);
            }
            let (owner, token) = super::SCHEDULER
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

        Syscall::CancelCellOwnerWatch { token } => {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                sched.cancel_cell_owner_watch(caller_id, token);
            }
            Ok(0)
        }

        Syscall::SpawnFromElf {
            grant_id,
            len,
            path_ptr,
            path_len,
        } => {
            let elf = snapshot_grant_elf(caller_id, grant_id, len)?;
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
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
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_gated(elf.bytes(), &path_str, request);
            #[cfg(cellos_p01_trace)]
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

        Syscall::SpawnPinned {
            path_ptr,
            path_len,
            priority,
            core_id,
        } => {
            if core_id != 0 {
                return Err(SyscallError::NotSupported);
            }
            let path_str = read_user_string(
                caller_id,
                path_ptr,
                path_len,
                crate::loader::disk_layout::MAX_CELL_PATH,
            )?;
            #[cfg(cellos_p01_trace)]
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
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            #[cfg(cellos_p01_trace)]
            let request = {
                let mut request = request;
                request.trace = *trace;
                request
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            #[cfg(cellos_p01_trace)]
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
                        crate::task::scheduler::current_cell_id() as u64,
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

        Syscall::OpenCap { path_ptr, path_len } => {
            let path_str = read_user_string(caller_id, path_ptr, path_len, 256)?;
            use crate::fs::VIFS1;
            let file = {
                let mut guard = VIFS1.lock();
                guard
                    .as_mut()
                    .ok_or(SyscallError::FileNotFound)?
                    .open(&path_str, api::fs::OpenMode::Read)
                    .map_err(|_| SyscallError::FileNotFound)?
            };
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
            let auth = auths
                .get_mut(&(cell_id.0, cell_gen))
                .ok_or(SyscallError::PermissionDenied)?;
            let ticket = auth
                .allocate_file_slot(file, api::cap::CapPerms::FILE_READ.0)
                .map_err(|_| SyscallError::OutOfMemory)?;
            Ok(ticket as usize)
        }

        Syscall::ReadCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => {
            if buf_len == 0 {
                return Ok(0);
            }
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_result = boxed_file.read(&mut kbuf);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match read_result {
                Ok(n) => {
                    if n > 0 {
                        write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
                    }
                    Ok(n)
                }
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::SeekCap {
            cap_id,
            offset,
            whence,
        } => {
            let pos = match whence {
                0 => api::fs::SeekFrom::Start(offset as u64),
                1 => api::fs::SeekFrom::Current(offset as isize as i64),
                2 => api::fs::SeekFrom::End(offset as isize as i64),
                _ => return Err(SyscallError::InvalidInput),
            };
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let seek_result = boxed_file.seek(pos);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match seek_result {
                Ok(new_pos) => Ok(new_pos as usize),
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::WriteCap {
            cap_id,
            buf_ptr,
            buf_len,
        } => {
            if buf_len == 0 {
                return Ok(0);
            }
            let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let write_result = boxed_file.write(&bytes);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            match write_result {
                Ok(n) => Ok(n),
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::StatCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.size();
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result
                .map(|s| s as usize)
                .map_err(|_| SyscallError::Unknown)
        }

        Syscall::TruncateCap { cap_id, len } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.truncate(len as u64);
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::SyncCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut boxed_file = {
                let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
                let auth = auths
                    .get_mut(&(cell_id.0, cell_gen))
                    .ok_or(SyscallError::PermissionDenied)?;
                auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
                    .map_err(|_| SyscallError::PermissionDenied)?
            };
            let result = boxed_file.sync();
            if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
                .lock()
                .get_mut(&(cell_id.0, cell_gen))
            {
                auth.unpark_file(cap_id as u64, boxed_file);
            }
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::GrantDma { bdf, phys, size } => {
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
            let publication = match reserve_caller_owned_dma_range(caller_id, bdf, phys_base, size)
            {
                Ok(publication) => publication,
                Err(DmaGrantError::NotOwned) => {
                    log::warn!(
                        "[iommu] Cell {caller_id} DMA grant denied: {phys:#x}+{size} is not \
                             contained in a live caller-owned Grant"
                    );
                    return Err(SyscallError::PermissionDenied);
                }
                Err(DmaGrantError::BdfNotOwned) => {
                    log::warn!(
                        "[iommu] Cell {caller_id} DMA grant denied: BDF {bdf:08x} is not owned"
                    );
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
                    log::warn!(
                        "[iommu] Cell {caller_id} DMA grant denied: publication already in flight"
                    );
                    return Err(SyscallError::Unknown);
                }
            };
            let iova =
                match super::drivers::iommu::map_dma_for_cell(caller_id as u64, bdf, phys, size) {
                    super::drivers::iommu::DmaMapResult::Mapped(iova) => iova,
                    super::drivers::iommu::DmaMapResult::Rejected => {
                        let rolled_back =
                            crate::memory::pin::rollback_pin(phys_base, size, caller_id);
                        debug_assert!(rolled_back, "rejected DMA map must release its exact pin");
                        drop(publication);
                        log::warn!(
                            "[iommu] Cell {caller_id} DMA grant denied: no active mapping backend"
                        );
                        return Err(SyscallError::Unknown);
                    }
                    super::drivers::iommu::DmaMapResult::PublishedUnconfirmed => {
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
            super::drivers::pcie_ecam::enable_bus_master(bdf);
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

        Syscall::CloseCap { cap_id } => {
            let (cell_id, cell_gen) = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| (t.cell_id, t.cell_generation))
                    .unwrap_or((types::CellId(0), 0))
            } else {
                (types::CellId(0), 0)
            };
            let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
            let auth = auths
                .get_mut(&(cell_id.0, cell_gen))
                .ok_or(SyscallError::PermissionDenied)?;
            auth.close_ticket(cap_id as u64)
                .map_err(|_| SyscallError::PermissionDenied)?;
            Ok(0)
        }

        Syscall::SpawnFromMem { args_ptr } => {
            if args_ptr == 0 {
                return Err(SyscallError::InvalidInput);
            }
            let view = caller_copy_view(caller_id)?;
            let mut args_bytes = [0u8; core::mem::size_of::<ViSpawnArgs>()];
            view.read_into(args_ptr, &mut args_bytes)
                .map_err(|_| SyscallError::InvalidInput)?;
            let args =
                unsafe { core::ptr::read_unaligned(args_bytes.as_ptr() as *const ViSpawnArgs) };
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
                api::TaskPriority::Normal as u8,
                #[cfg(cellos_p01_trace)]
                *trace,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            let spawned =
                crate::loader::mem_spawn_gate::spawn_from_mem_gated(&elf_bytes, name, request);
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

        Syscall::Create { path_ptr, path_len } => {
            let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
            Err(SyscallError::PermissionDenied)
        }
        Syscall::SetTimer { deadline } => {
            // Check if deadline passed
            let now = super::system_ticks();
            let wake_at = now + deadline;

            // Sleep!
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.current_task_mut() {
                    task.state = TaskState::Sleeping { until: wake_at };
                }
            }
            // Yield CPU safely
            super::yield_cpu();
            Ok(0)
        }

        Syscall::GetProcs { buf_ptr, buf_len } => {
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

        Syscall::GetProcs2 { buf_ptr, buf_len } => {
            let bytes_len = buf_len
                .checked_mul(core::mem::size_of::<api::syscall::ProcessInfoV2>())
                .ok_or(SyscallError::InvalidInput)?;
            validate_user_buf(buf_ptr, bytes_len, MAX_USER_BUF)?;

            let sample_ticks = super::system_ticks() as u64;
            let rows =
                collect_process_rows(buf_len, |task| snapshot_process_info_v2(task, sample_ticks));
            let raw_bytes = unsafe {
                core::slice::from_raw_parts(
                    rows.as_ptr() as *const u8,
                    rows.len() * core::mem::size_of::<api::syscall::ProcessInfoV2>(),
                )
            };
            write_user_slice(caller_id, buf_ptr, raw_bytes, MAX_USER_BUF)?;
            Ok(rows.len())
        }

        Syscall::MemInfo { out_ptr, out_len } => {
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
                core::slice::from_raw_parts(
                    &info as *const api::syscall::ViMemInfoV1 as *const u8,
                    len,
                )
            };
            write_user_slice(caller_id, out_ptr, raw_bytes, MAX_USER_BUF)?;
            Ok(len)
        }

        #[cfg(feature = "bench-counters")]
        Syscall::BenchCounters { out_ptr, out_len } => {
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

        Syscall::TelemetrySnapshot {
            out_ptr,
            out_len,
            flags,
            reserved,
        } => {
            use api::telemetry::request;
            // The dispatch gate already required `observe_tasks` (X). Arguments
            // are refused before anything is sampled.
            if !super::telemetry::request_is_valid(out_ptr, out_len, flags, reserved) {
                return Err(SyscallError::InvalidInput);
            }
            validate_user_buf(out_ptr, out_len, MAX_USER_BUF)?;
            let memory_allowed = flags & request::SYSTEM_MEMORY != 0
                && caller_has_cap(caller_id, |rights| rights.observe_memory);
            let epoch = super::telemetry::provider_epoch();
            // Identity, membership and CPU in one SCHEDULER hold. The transient
            // rows are allocated there, charged to the caller like GetProcs2.
            let capture = {
                let mut guard = super::SCHEDULER.lock();
                let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
                super::telemetry::capture(
                    sched,
                    epoch,
                    super::telemetry::monotonic_ns(),
                    out_len,
                    flags & request::TASKS != 0,
                )
                .map_err(|_| SyscallError::OutOfMemory)?
            };
            // Frames are read under FRAME_ALLOCATOR only after SCHEDULER is released.
            let memory = if flags & request::SYSTEM_MEMORY == 0 {
                super::telemetry::SystemMemory::NotRequested
            } else if !memory_allowed {
                super::telemetry::SystemMemory::Denied
            } else {
                let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
                match guard.as_ref() {
                    Some(allocator) => {
                        super::telemetry::SystemMemory::Present(api::syscall::ViMemInfoV1 {
                            total_frames: allocator.total_frames() as u64,
                            used_frames: allocator.used_frames() as u64,
                            free_frames: allocator.free_frames() as u64,
                            page_size: allocator.page_size() as u64,
                        })
                    }
                    None => super::telemetry::SystemMemory::Unavailable,
                }
            };
            let bytes = capture.finish(memory, super::telemetry::monotonic_ns());
            write_user_slice(caller_id, out_ptr, &bytes, MAX_USER_BUF)?;
            Ok(bytes.len())
        }

        Syscall::Seek { fd, offset, whence } => {
            super::file_seek(fd, offset, whence).map_err(|_| SyscallError::Unknown)
        }

        Syscall::FileOp { op, arg1, arg2 } => {
            match op {
                0 => {
                    let path = read_user_string(caller_id, arg1, arg2, MAX_LOG_MSG)?;
                    super::file_remove(caller_id, &path).map_err(|_| SyscallError::PermissionDenied)
                }
                1 => {
                    // Rename - Stub
                    Err(SyscallError::NotSupported)
                }
                _ => Err(SyscallError::InvalidCommand),
            }
        }

        Syscall::GetTime { op } => {
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
                4 => Ok(super::system_ticks()),
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
        Syscall::GetCpuCount => Ok(crate::task::smp::online_hart_count()),
        Syscall::AudioPlay { .. } => {
            // No Sound Cell registered; virtio_sound.rs removed (kernel boundary law).
            // Syscall 218 is reserved — do not reuse. A VirtIO Sound Driver Cell
            // will handle audio in G2 via direct MMIO + IPC, no kernel driver needed.
            log::warn!("[audio] AudioPlay: no Sound Cell registered (G2 TODO)");
            Err(SyscallError::Unknown)
        }
        // Compositor-only; the source is copied through the caller's copy view
        // into a GPU-Cell-owned Grant before the hand-off, so the GPU Cell never
        // reads compositor memory; the framebuffer fallback stages it the same
        // way (H-26).
        Syscall::GpuFlush {
            data_ptr,
            data_len,
            xy,
            wh,
        } => super::gpu_scanout::flush(caller_id, data_ptr, data_len, xy, wh),
        Syscall::GpuCursor {
            op,
            data_ptr,
            xy,
            hot,
        } => super::gpu_scanout::cursor(caller_id, op, data_ptr, xy, hot),
        Syscall::GpuGetResolution => {
            // Firmware scanout registration is authoritative when present;
            // generic VirtIO retains the legacy fallback before its driver
            // publishes a query IPC path.
            // Packed as (width << 32) | height, which only fits a 64-bit
            // register; RV32 Nano has no GPU cell so this path never fires there.
            #[cfg(not(target_arch = "riscv32"))]
            {
                if let Some((width, height)) =
                    crate::resource_registry::display_framebuffer_resolution()
                {
                    return Ok(((width as usize) << 32) | height as usize);
                }
                Ok(((1280usize) << 32) | 800usize)
            }
            #[cfg(target_arch = "riscv32")]
            {
                Ok(0)
            }
        }
        Syscall::NetTx {
            frame_ptr,
            frame_len,
        } => {
            if !caller_has_network(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::audit::log_event(
                crate::audit::AuditEvent::NetTx,
                &crate::audit::encode_u32x2(caller_id as u32, frame_len as u32),
            );
            let frame = read_user_slice(caller_id, frame_ptr, frame_len, MAX_USER_BUF)?;
            let ok = crate::task::drivers::nic::send_frame(&frame);
            Ok(if ok { 1 } else { 0 })
        }
        Syscall::NetRx { buf_ptr, buf_len } => {
            if !caller_has_network(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let n = crate::task::drivers::nic::recv_frame(&mut kbuf);
            if n > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
            }
            Ok(n)
        }
        Syscall::StateStash {
            key,
            buf_ptr,
            buf_len,
        } => {
            let raw_key = key as u64;
            let is_shell = caller_launch_state(caller_id)
                .is_some_and(|(name, _, _)| name.as_str() == "cellbox");
            if is_shell
                && (raw_key != SPAWN_ARGV_KEY
                    || buf_len > crate::cell::state_stash::MAX_SPAWN_ARGV_LEN)
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
        Syscall::StateRestore {
            key,
            buf_ptr,
            buf_len,
        } => {
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
        // 412: StateStashClear — owner (open), sealed destination (consume) or a
        // supervisor (abort). Absent keys succeed; a foreign entry is refused.
        Syscall::StateStashClear { key } => {
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

        // ── Supervisor Primitives (P03) ───────────────────────────────────────

        // 413: FreezeCell — stop a running Cell from being scheduled.
        // The frozen cell still exists; its queued IPC is preserved.
        Syscall::FreezeCell { target_tid } => {
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

        // 422: PauseService — atomically hide the expected provider from new
        // lookups without stopping it, so it can process Snapshot IPC.
        Syscall::PauseService {
            service_id,
            expected_tid,
        } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if expected_tid == 0 {
                return Err(SyscallError::InvalidInput);
            }
            if !crate::cell::service_registry::pause(service_id, expected_tid) {
                return Err(SyscallError::TryAgain);
            }
            if !super::inbound_ipc_drained(expected_tid) {
                return Err(SyscallError::TryAgain);
            }
            Ok(0)
        }

        // 414: plain resume or atomic old-provider -> replacement cutover.
        Syscall::ResumeCell {
            target_tid,
            source_tid,
            service_id,
            reserved,
        } => {
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

        // 415: KillCell — terminate a Cell and reclaim its resources.
        Syscall::KillCell {
            target_tid,
            exit_code,
        } => {
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
                let guard = super::SCHEDULER.lock();
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

        // 419: QueryHotswapReady — non-blocking check of a cell's hotswap_ready flag.
        // Returns 1 if the cell has called sys_hotswap_ready(), 0 if not yet,
        // usize::MAX if the tid is not found.
        Syscall::QueryHotswapReady { target_tid } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let ready = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|s| s.tasks.get(&target_tid).map(|t| t.hotswap_ready));
            match ready {
                Some(true) => Ok(1),
                Some(false) => Ok(0),
                None => Err(SyscallError::FileNotFound), // tid does not exist
            }
        }

        Syscall::SpawnReplacement {
            old_tid,
            path_ptr,
            path_len,
        } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if old_tid == 0 || path_len == 0 || path_len > crate::loader::disk_layout::MAX_CELL_PATH
            {
                return Err(SyscallError::InvalidInput);
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
            let request =
                match governed_replacement_request(caller_id, profile.child_ceiling, replacement) {
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

        // ── Driver Cell Registration (P00) ───────────────────────────────────

        // 416: RegisterBlockDriver — bind the caller's owned block device route.
        Syscall::RegisterBlockDriver { kind, id, reserved } => {
            register_block_device_route(caller_id, kind, id, reserved)
        }

        // 417: RegisterNicDriver — announce caller as the active NIC driver.
        Syscall::RegisterNicDriver => register_driver_service(
            caller_id,
            api::syscall::service::NIC_DRIVER,
            crate::task::drivers::driver_cell::register_nic_driver,
            crate::task::drivers::driver_cell::deregister_nic_driver,
        ),

        // 423: RegisterUsbHidProducer — announce caller as the USB HID event
        // producer, publishing service::USB_HID_PRODUCER for the input service's
        // producer gate. Requires UsbDriverCap and never touches the singleton
        // NIC role, so an xHCI cell on a PC with e1000/virtio-net cannot steal
        // the network route.
        Syscall::RegisterUsbHidProducer => register_usb_hid_producer_role(caller_id),

        // 418: FindPcieDevice — query ECAM table for a device by class triple.
        // Writes a `PcieDeviceInfo` record to `out_ptr` and returns 1 if found.
        // Requires PcieDriverCap and a class row for the caller's measured
        // program in `catalog::PCI_DRIVERS`; also records BDF ownership in
        // resource_registry. Every Ethernet driver claims through 424.
        Syscall::FindPcieDevice {
            class,
            subclass,
            prog_if,
            out_ptr,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !caller_pci_query_allowed(caller_id, |program| {
                crate::catalog::pci_class_allowed(program, class, subclass, prog_if)
            }) {
                return Err(SyscallError::PermissionDenied);
            }
            let usable = |bdf| pcie_usable_by(caller_id, bdf);
            match crate::task::drivers::pcie_ecam::find_class_where(
                class, subclass, prog_if, usable,
            ) {
                None => Ok(0), // device not present → VirtIO fallback
                Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
            }
        }

        // 424: FindPcieDeviceByVendor — query ECAM table for an exact
        // (vendor_id, device_id). Same hand-out semantics as 418 (PcieDriverCap,
        // BDF ownership, 48-byte `PcieDeviceInfo`), but the match names the
        // device, so two Driver Cells that share a class triple each claim their
        // own controller without a sibling-exit race.
        Syscall::FindPcieDeviceByVendor {
            vendor_id,
            device_id,
            out_ptr,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !caller_pci_query_allowed(caller_id, |program| {
                crate::catalog::pci_id_allowed(program, vendor_id, device_id)
            }) {
                return Err(SyscallError::PermissionDenied);
            }
            let usable = |bdf| pcie_usable_by(caller_id, bdf);
            match crate::task::drivers::pcie_ecam::find_vendor_device(vendor_id, device_id, usable)
            {
                None => Ok(0),
                Some(dev) => offer_pcie_device(caller_id, out_ptr, dev),
            }
        }

        // 434/435 (ADR-0031 CP1 §3–§4): the owned function's record, and its
        // MSI-X/MSI vectors. PcieDriverCap plus BDF ownership (418/424) gate both.
        Syscall::QueryPcieFunction {
            bdf,
            out_ptr,
            out_len,
            reserved,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if out_len != api::pcie_function::PCIE_FUNCTION_RECORD_LEN || reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            let record = query_pcie_function(caller_id, bdf)?;
            write_user_slice(caller_id, out_ptr, &record, record.len())
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(0)
        }
        Syscall::AllocPcieIrq {
            bdf,
            want,
            mode,
            reserved,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if reserved != 0 {
                return Err(SyscallError::InvalidInput);
            }
            alloc_pcie_irq(caller_id, bdf, want, mode)
        }
        Syscall::SystemPower { action, a1, a2, a3 } => {
            if !caller_has_supervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if a1 != 0 || a2 != 0 || a3 != 0 {
                return Err(SyscallError::InvalidInput);
            }
            system_power(caller_id, action)
        }

        // 437-439 (ADR-0033 CP1 §5): client-memory DMA export leases. 437/439
        // gate on own memory / lease party; 438 also needs bit 48,
        // PcieDriverCap and BDF ownership (checked in `dma_export::map`).
        Syscall::ExportDmaRange {
            va,
            len,
            holder_service,
            flags,
        } => dma_export::export(caller_id, va, len, holder_service, flags),
        Syscall::MapDmaExport {
            export_id,
            bdf,
            out_ptr,
            out_len,
        } => {
            if out_len != api::dma_export::DMA_EXPORT_RECORD_LEN {
                return Err(SyscallError::InvalidInput);
            }
            caller_copy_view(caller_id)?
                .validate_writable(out_ptr, out_len)
                .map_err(|_| SyscallError::InvalidInput)?;
            // Same retirement barrier as GrantDma: the holder's root cannot
            // retire (and tear its domain down) mid-publication.
            let barrier =
                crate::memory::cell_quota::try_reserve_dma(0, 0).ok_or(SyscallError::Unknown)?;
            let publication =
                reserve_dma_publication(caller_id, barrier).ok_or(SyscallError::TryAgain)?;
            let record = dma_export::map(caller_id, export_id, bdf);
            drop(publication);
            let mut bytes = [0u8; api::dma_export::DMA_EXPORT_RECORD_LEN];
            record?.encode(&mut bytes);
            write_user_slice(caller_id, out_ptr, &bytes, bytes.len())
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(0)
        }
        Syscall::RevokeDmaExport {
            export_id,
            a1,
            a2,
            a3,
        } => {
            if a1 != 0 || a2 != 0 || a3 != 0 {
                return Err(SyscallError::InvalidInput);
            }
            dma_export::revoke(caller_id, export_id)
        }

        // 425-428: the serial-port surface. The kernel owns the 16550 mechanism
        // (it probed and programmed every port at boot); a cell holding the
        // `serial_port` capability drives only the ports that probe accepted, so
        // there is no way to reach port space the machine did not prove it has.
        // Index 0 is the console; the extras are polled, not interrupt-driven.
        Syscall::SerialPortInfo { index, out_ptr } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let Some((base, irq)) = serial_port::declared(index) else {
                return Ok(0);
            };
            let present = serial_port::present(index);
            let info = [base.to_le_bytes(), [irq, present as u8]].concat();
            write_user_slice(caller_id, out_ptr, &info, info.len())?;
            Ok(present as usize)
        }
        Syscall::SerialWrite { index, ptr, len } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(usize::MAX);
            }
            // Bound one call: a cell cannot hold the console path for an
            // unbounded burst through a blocking port.
            const SERIAL_BURST_MAX: usize = 4096;
            let len = len.min(SERIAL_BURST_MAX);
            if len == 0 {
                return Ok(0);
            }
            let view = caller_copy_view(caller_id)?;
            let mut buf = [0u8; SERIAL_BURST_MAX];
            let buf = &mut buf[..len];
            view.read_into(ptr, buf)
                .map_err(|_| SyscallError::InvalidInput)?;
            let mut written = 0usize;
            for byte in buf.iter() {
                if !serial_port::write_byte(index, *byte) {
                    break;
                }
                written += 1;
            }
            Ok(written)
        }
        Syscall::SerialRead { index, ptr, len } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(usize::MAX);
            }
            const SERIAL_BURST_MAX: usize = 4096;
            let len = len.min(SERIAL_BURST_MAX);
            if len == 0 {
                return Ok(0);
            }
            let mut buf = [0u8; SERIAL_BURST_MAX];
            let mut read = 0usize;
            while read < len {
                let Some(byte) = serial_port::read_byte(index) else {
                    break;
                };
                buf[read] = byte;
                read += 1;
            }
            write_user_slice(caller_id, ptr, &buf[..read], len)?;
            Ok(read)
        }
        Syscall::SerialConfigure { index, baud } => {
            if !caller_has_serial_port(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !serial_port::present(index) {
                return Ok(2);
            }
            if serial_port::configure_baud(index, baud) {
                Ok(0)
            } else {
                Ok(3)
            }
        }

        // 234: WaitIrq — block until hardware IRQ fires (Driver Cell).
        // ISR calls irq_wait::signal_irq (atomic only; no lock, no scheduler access).
        // Scheduler sweep (pick_next) does the actual Ready transition.
        Syscall::WaitIrq { irq, mmio_base } => {
            if !caller_has_pcie_driver(caller_id)
                && !caller_has_platform(caller_id)
                && !caller_has_usb_driver(caller_id)
            {
                return Err(SyscallError::PermissionDenied);
            }
            #[cfg(target_arch = "aarch64")]
            {
                use hal::aarch64::backend::{ArmIrqController, Board};
                if let Some(usb_irq) = <Board as ArmIrqController>::USB_ONE_SHOT_IRQ {
                    if caller_has_usb_driver(caller_id) && irq == usb_irq as u8 {
                        // One-shot unmask: re-enable the USB legacy IRQ after the
                        // driver has cleared GINTSTS.
                        <Board as ArmIrqController>::unmask_usb_irq();
                    }
                }
            }
            // The ISR dereferences a stored nonzero base (VirtIO InterruptACK),
            // so only an MMIO window the caller was granted is accepted.
            if !wait_irq_base_permitted(caller_id, mmio_base) {
                return Err(SyscallError::PermissionDenied);
            }
            // x86 IRQ numbers are MSI vectors from 435; a cell waits only on its
            // own, so it can neither steal nor drain another driver's interrupts.
            #[cfg(target_arch = "x86_64")]
            if !crate::task::drivers::pcie_msi::irq_owned_by(irq, caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // Lost-wakeup guard: if IRQ already fired before this call, return immediately.
            if crate::task::drivers::irq_wait::take_pending(irq) {
                return Ok(0);
            }
            // Single-waiter policy: second caller on same IRQ gets TryAgain.
            if !crate::task::drivers::irq_wait::register_waiter(irq, caller_id, mmio_base) {
                return Err(SyscallError::TryAgain);
            }
            // Park the task — scheduler sweep will wake it when IRQ_PENDING[irq] is set.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                    task.state = super::tcb::TaskState::WaitIrq { irq };
                }
            }
            super::yield_cpu();
            Ok(0)
        }

        // 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
        // Populates the MMIO allowlist only. BDF ownership belongs to the Driver
        // Cell that claims the device through FindPcieDevice.
        Syscall::RegisterPcieBar { bdf: _, base, len } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::resource_registry::register_pcie_bar(base, len);
            // Any RegisterPcieBar call from the Platform Cell marks it as active;
            // Phase 08 uses this to gate out the kernel ECAM scan.
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            Ok(0)
        }

        // 236: RegisterPciDevice — Platform Cell announces a device with class/BAR info.
        // Populates PCI_DEVICES so sys_find_pcie_device queries work without kernel ECAM scan.
        // After each registration, attempt deferred IOMMU init (no-op until IOMMU device appears).
        // Every function that answers is recorded, as the bus-0 scan records it, so
        // the machine fingerprint (433) does not depend on the driver table. `1`
        // means recorded but an Ethernet ID no Driver Cell drives: the handout
        // queries never offer it, and the Platform Cell names its ID. A BDF with no
        // function inside the admitted window is `FileNotFound` and not recorded.
        Syscall::RegisterPciDevice {
            bdf,
            cls,
            bar0_base,
            bar0_size,
        } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            use crate::task::drivers::pcie_ecam::{register_device, Registration};
            let registration = register_device(bdf, cls, bar0_base, bar0_size);
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            crate::task::drivers::iommu::try_deferred_init();
            match registration {
                Registration::Recorded => Ok(0),
                Registration::RecordedWithoutDriver => Ok(1),
                Registration::Absent => Err(SyscallError::FileNotFound),
            }
        }

        // 237: ReadLog — drain the user-log (source 0) or audit (source 1) ring
        // into a cell-provided buffer. The caller must have allowlist bit 54
        // (ReadLog) set in its manifest; the audit ring also needs SupervisorCap.
        // Returns the number of bytes actually copied (0 when nothing available).
        Syscall::ReadLog {
            buf_ptr,
            max,
            source,
        } => {
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

        Syscall::BlkFlush => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkFlush denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }

            match crate::task::drivers::block::flush() {
                Ok(()) => Ok(1),
                Err(_) => Ok(0),
            }
        }
        Syscall::Shutdown => {
            authorize_raw_shutdown(caller_id)?;
            #[cfg(all(feature = "test-hooks", target_arch = "aarch64"))]
            crate::qemu_exit(true);

            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            crate::qemu_exit(true);

            #[cfg(all(not(feature = "test-hooks"), target_arch = "riscv64"))]
            unsafe {
                // SAFETY: ecall traps to OpenSBI which powers off QEMU; no return.
                core::arch::asm!(
                    "li a7, 0x53525354", // SBI_EXT_SRST
                    "li a6, 0",          // fid = SYSTEM_RESET
                    "li a0, 0",          // reset_type = Shutdown
                    "li a1, 0",          // reset_reason = NoReason
                    "ecall",
                    options(noreturn)
                );
            }
            #[cfg(all(not(feature = "test-hooks"), target_arch = "aarch64"))]
            unsafe {
                // PSCI SYSTEM_OFF via HVC (QEMU virt machine default)
                core::arch::asm!(
                    "mov x0, #0x0008",
                    "movk x0, #0x8400, lsl #16",
                    "hvc #0",
                    options(noreturn)
                );
            }
            #[cfg(target_arch = "x86_64")]
            loop {
                unsafe {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            }
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            loop {
                unsafe {
                    core::arch::asm!("wfi", options(nomem, nostack));
                }
            }
        }
        Syscall::BlkRead { sector, buf_ptr } => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkRead denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }
            // Per-cell partition range gate — a runaway FAT offset must never
            // reach kernel-owned LBAs (P2 cell table, P3 snapshot). Returns 0 = failure.
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            validate_user_buf(buf_ptr, 512, MAX_USER_BUF)?;

            // Bounce buffer: VirtioHal::share() treats the buffer's virtual address
            // as its physical address (identity-map assumption). Stack frames ARE
            // identity-mapped; ELF BSS/heap pages are NOT — DMA would land at the
            // wrong physical address without the bounce. Read into an on-stack buffer
            // (always identity-mapped), then copy to the user buffer under SUM=1.
            let mut bounce = [0u8; 512];
            match crate::task::drivers::block::read_sector(sector, &mut bounce) {
                Ok(()) => {
                    write_user_slice(caller_id, buf_ptr, &bounce, MAX_USER_BUF)?;
                    Ok(1)
                }
                Err(_) => Ok(0),
            }
        }
        Syscall::BlkWrite { sector, buf_ptr } => {
            if !caller_has_block_io(caller_id) {
                log::warn!(
                    "BlkWrite denied: task {} lacks block-I/O capability",
                    caller_id
                );
                return Err(SyscallError::PermissionDenied);
            }
            // Per-cell partition range gate — prevents a cell from corrupting
            // the loader's table or the snapshot region. Returns 0 = failure.
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            let user = read_user_slice(caller_id, buf_ptr, 512, MAX_USER_BUF)?;
            let mut bounce = [0u8; 512];
            bounce.copy_from_slice(&user);
            match crate::task::drivers::block::write_sector(sector, &bounce) {
                Ok(()) => Ok(1),
                Err(error) => {
                    log::warn!("[blk] write LBA={} failed: {:?}", sector, error);
                    Ok(0)
                }
            }
        }
        Syscall::HotSwapReady => {
            // The new cell signals that it has finished deserializing state.
            // Set the per-task flag; the supervisor cutover path polls this.
            // No SpawnCap required — only the new cell itself calls this
            // after its restore flow finishes.
            crate::cell::hotswap::set_task_hotswap_ready(caller_id);
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::milestone("hotswap-ready", caller_id);
            Ok(0)
        }

        Syscall::Snapshot => {
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

        // ── Zero-Copy Grant Syscalls (Phase 01, Storage 2.0) ─────────────────
        Syscall::GrantAlloc { size } => {
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
                    log::warn!(
                        "[grant] GrantAlloc owner mapping failed for task {caller_id}: {error:?}"
                    );
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
            #[cfg(cellos_p01_trace)]
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
        Syscall::GrantShare {
            grant_id,
            target_cell,
            perm,
        } => {
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

        Syscall::GrantSlice {
            grant_id,
            size_out_ptr,
            expected_owner_tid,
            flags,
        } => {
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
            super::user_out::write_resolved_optional_usize(access.size_out, access.size);
            Ok(access.base)
        }

        Syscall::GrantFree { grant_id } => {
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

        Syscall::GrantCacheSyncBegin {
            grant_id,
            offset,
            len,
        } => {
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

        Syscall::GrantCacheSyncComplete { token } => {
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

        Syscall::RegisterDisplayFramebuffer {
            base,
            size,
            packed_dimensions,
            pitch,
        } => {
            #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
            {
                const MAX_DIMENSION: usize = 8192;
                const PAGE_SIZE: usize = 4096;
                // AArch64: only a board with a firmware display path registers a
                // framebuffer; any other board refuses before every other check.
                #[cfg(target_arch = "aarch64")]
                let Some(display) =
                    <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::DISPLAY
                else {
                    let _ = (base, size, packed_dimensions, pitch);
                    return Err(SyscallError::Unknown);
                };
                if !caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY) {
                    return Err(SyscallError::PermissionDenied);
                }
                #[cfg(target_arch = "aarch64")]
                if !crate::resource_registry::owns_exact_mmio(
                    caller_id,
                    display.mailbox.base,
                    display.mailbox.size,
                ) {
                    return Err(SyscallError::PermissionDenied);
                }
                if base == 0
                    || size == 0
                    || base & (PAGE_SIZE - 1) != 0
                    || packed_dimensions > u32::MAX as usize
                {
                    return Err(SyscallError::InvalidInput);
                }
                let width = packed_dimensions >> 16;
                let height = packed_dimensions & 0xffff;
                if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                    return Err(SyscallError::InvalidInput);
                }
                let minimum_pitch = width.checked_mul(4).ok_or(SyscallError::InvalidInput)?;
                if pitch < minimum_pitch || pitch & 3 != 0 {
                    return Err(SyscallError::InvalidInput);
                }
                let covered = pitch
                    .checked_mul(height)
                    .ok_or(SyscallError::InvalidInput)?;
                let end = base.checked_add(size).ok_or(SyscallError::InvalidInput)?;
                #[cfg(target_arch = "aarch64")]
                if covered > size || end > display.framebuffer_limit {
                    return Err(SyscallError::InvalidInput);
                }
                #[cfg(not(target_arch = "aarch64"))]
                {
                    if covered > size {
                        return Err(SyscallError::InvalidInput);
                    }
                }
                // The framebuffer is MMIO: it must not overlap *any* managed frame,
                // not merely sit above one end of the allocator's memory.
                //
                // `match` rather than `?`: the early return has to happen outside the
                // `guard` borrow, or the RPi3 build refuses the guard's lifetime
                // (E0597) with the borrowed allocator still in scope.
                let overlaps_managed = {
                    let guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
                    match guard.as_ref() {
                        Some(allocator) => allocator.managed_ranges().any(|range| {
                            let range_end = range.start + range.frames * PAGE_SIZE;
                            base < range_end && range.start < end
                        }),
                        None => return Err(SyscallError::Unknown),
                    }
                };
                if overlaps_managed {
                    return Err(SyscallError::InvalidInput);
                }
                let rounded = size
                    .checked_add(PAGE_SIZE - 1)
                    .ok_or(SyscallError::InvalidInput)?
                    & !(PAGE_SIZE - 1);
                let pages = rounded / PAGE_SIZE;
                let owner = types::CellId(caller_id as u64);
                match crate::resource_registry::reserve_display_framebuffer(
                    owner,
                    base,
                    size,
                    width as u16,
                    height as u16,
                    pitch,
                ) {
                    Ok(crate::resource_registry::DisplayFramebufferReservation::ActiveReplay) => {
                        return Ok(0);
                    }
                    Err(crate::resource_registry::DisplayFramebufferReservationError::Conflict) => {
                        return Err(SyscallError::PermissionDenied);
                    }
                    Ok(crate::resource_registry::DisplayFramebufferReservation::Reserved) => {}
                }
                if crate::memory::paging::map_display_framebuffer_user(base, pages).is_err() {
                    crate::resource_registry::cancel_display_framebuffer(
                        owner,
                        base,
                        size,
                        width as u16,
                        height as u16,
                        pitch,
                    );
                    return Err(SyscallError::Unknown);
                }
                if crate::resource_registry::activate_display_framebuffer(
                    owner,
                    base,
                    size,
                    width as u16,
                    height as u16,
                    pitch,
                ) {
                    #[cfg(target_arch = "x86_64")]
                    crate::task::drivers::fb_console::handover();
                    Ok(0)
                } else {
                    // This cannot be recovered safely: pages are firmware-owned
                    // and remain mapped until reboot, so display stays fail-stop.
                    Err(SyscallError::Unknown)
                }
            }
            #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
            {
                let _ = (base, size, packed_dimensions, pitch);
                Err(SyscallError::Unknown)
            }
        }

        Syscall::GrantRegister { size } => {
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
            #[cfg(cellos_p01_trace)]
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
        Syscall::GrantUnregister { reg_id } => {
            unregister_registered_grant(caller_id, reg_id)?;
            Ok(0)
        }

        Syscall::WaitForEvent { mask, deadline } => {
            // Lost-wakeup guard: check pending events BEFORE parking.
            let already = super::waker::consume_pending(mask);
            if already != 0 {
                return Ok(already as usize);
            }
            // Park: set WaitEvent state so the timer sweep can wake this task.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.tasks.get_mut(&caller_id) {
                    task.state = super::tcb::TaskState::WaitEvent { mask, deadline };
                }
            }
            // Yield; wake happens in pick_next's global sweep (hart 0).
            super::yield_cpu();
            // After re-schedule: timer sweep wrote the fired mask into trap_frame.regs[10].
            // Return it so ViCell_syscall_dispatch writes the correct value back.
            let fired = super::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|s| s.tasks.get(&caller_id))
                .map(|t| t.trap_frame.regs[10])
                .unwrap_or(0);
            #[cfg(target_arch = "riscv32")]
            let fired = fired as usize;
            Ok(fired)
        }

        Syscall::WaitCompletion {
            mask,
            deadline,
            out_ptr,
        } => super::completion_wait::wait_completion(caller_id, mask, deadline, out_ptr),

        Syscall::RequestMmio { base, len } => {
            // PlatformCap bypass: the Platform Cell may claim only the kernel-admitted
            // ECAM config-space window (not in PCIE_BARS), never managed RAM — the
            // claim is user-mapped. Overlap check still applies.
            // x86 SAS: the boot identity map covers MMIO kernel-only; a granted
            // window must gain PTE_USER (+PCD, NX) before the ring-3 cell touches
            // it. BAR windows may not be mapped at all yet — this also creates
            // the identity mapping.
            #[cfg(target_arch = "x86_64")]
            fn user_map(base: usize, len: usize) {
                crate::memory::paging::map_mmio_user_x86(base, len);
            }
            // RISC-V/AArch64 make an allowlisted window reachable from EL0 with the
            // *boot* identity map, so a window whose access was revoked earlier —
            // by a capability revoke, or by the self-test that exercises one — stays
            // kernel-only unless the grant re-arms it. This is the non-x86 half of
            // the same rule: a granted window is a reachable window.
            #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
            fn user_map(base: usize, len: usize) {
                let _ = crate::memory::paging::grant_mmio_user(base, len);
            }
            #[cfg(not(any(
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "x86_64"
            )))]
            fn user_map(_base: usize, _len: usize) {}
            // x86: a PCIe BAR is user-mapped without its MSI-X table/PBA pages
            // (ADR-0031 CP1 §4.3), so a cell cannot retarget its device's
            // interrupts by rewriting table entries.
            #[cfg(target_arch = "x86_64")]
            fn user_map_pcie_bar(base: usize, len: usize) {
                for (start, size) in crate::task::drivers::pcie_msi::user_ranges(base, len) {
                    crate::memory::paging::map_mmio_user_x86(start, size);
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            fn user_map_pcie_bar(base: usize, len: usize) {
                user_map(base, len);
            }

            if caller_has_platform(caller_id) {
                return match crate::resource_registry::request_ecam_mmio(caller_id, base, len) {
                    Ok(()) => {
                        user_map(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // PCIe BAR path: cells with PcieDriverCap may claim any BAR
            // discovered during ECAM scan (registered in resource_registry::PCIE_BARS).
            if caller_has_pcie_driver(caller_id) && crate::resource_registry::is_pcie_bar(base, len)
            {
                return match crate::resource_registry::request_mmio(
                    caller_id,
                    base,
                    len,
                    crate::resource_registry::DEV_PCIE,
                ) {
                    Ok(()) => {
                        user_map_pcie_bar(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // USB Host path: cells with UsbDriverCap may claim the DWC2 MMIO aperture.
            if caller_has_usb_driver(caller_id) && crate::resource_registry::is_dwc2_mmio(base, len)
            {
                return match crate::resource_registry::request_dwc2_mmio(caller_id, base, len) {
                    Ok(()) => {
                        user_map(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::PermissionDenied) => Ok(1),
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // GPIO/UART path: gate on manifest-declared device classes.
            let allowed_devices = {
                let sched = super::SCHEDULER.lock();
                sched
                    .as_ref()
                    .map_or(0, |s| s.cell_rights(caller_id).mmio_devices)
            };
            if allowed_devices == 0 {
                return Err(SyscallError::PermissionDenied);
            }
            match crate::resource_registry::request_mmio(caller_id, base, len, allowed_devices) {
                Ok(()) => {
                    user_map(base, len);
                    Ok(0)
                }
                Err(types::ViError::PermissionDenied) => {
                    log::warn!(
                        "[mmio] DENY caller={} base={:#x} len={:#x} allowed_devices={:#04x}",
                        caller_id,
                        base,
                        len,
                        allowed_devices
                    );
                    Ok(1)
                }
                Err(types::ViError::AlreadyExists) => Ok(2),
                Err(_) => Ok(3),
            }
        }

        // 442: ProbeVirtioMmio — the kernel reads a slot's identity registers so
        // a Driver Cell can pick its slot before claiming it; a claim is
        // exclusive and never released, so probing by claiming would lock
        // sibling drivers out of their slots.
        Syscall::ProbeVirtioMmio { base, reserved } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if reserved != (0, 0, 0) {
                return Err(SyscallError::InvalidInput);
            }
            #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
            {
                use crate::task::drivers::virtio_common::{probe_slot, SlotProbe};
                match probe_slot(base) {
                    SlotProbe::Identity(identity) => Ok(identity),
                    SlotProbe::NoTransport => Err(SyscallError::FileNotFound),
                    SlotProbe::NotASlot => Err(SyscallError::InvalidInput),
                }
            }
            // x86_64 reaches VirtIO over PCI: there is no MMIO slot to probe.
            #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
            {
                let _ = base;
                Err(SyscallError::NotSupported)
            }
        }

        Syscall::GetRandom { buf_ptr, len } => {
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
            let written = crate::task::drivers::virtio_rng::get_random(&mut kbuf[..capped]);
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
                        super::system_ticks() as u32 ^ (caller_id as u32).wrapping_mul(0x9e37_79b9);
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

        Syscall::BlkReadAsync { sector, grant_id } => {
            if !caller_has_block_io(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            if !check_block_access(caller_id, sector, 1) {
                return Ok(0);
            }
            // Validate ownership and minimum size (must hold ≥ 512 bytes).
            let buf_paddr = {
                let tbl = grant_table_lock().lock();
                tbl.as_ref()
                    .and_then(|m| m.get(&grant_id))
                    .filter(|g| g.owner == caller_id && g.size >= 512)
                    .map(|g| g.base)
            };
            let buf_paddr = match buf_paddr {
                Some(p) => p,
                None => return Ok(0),
            };
            // Grant pages are identity-mapped (vaddr == paddr), so DMA addresses are correct.
            // SAFETY: buf_paddr is a physically contiguous, identity-mapped grant page; valid
            // for 512 bytes of VirtIO DMA read.

            let buf = unsafe { core::slice::from_raw_parts_mut(buf_paddr as *mut u8, 512) };
            match crate::task::drivers::block::read_sector(sector, buf) {
                Ok(()) => Ok(1), // async_id = 1 means immediately complete (Phase 04 for real async)
                Err(_) => Ok(0),
            }
        }

        // ── Hypervisor syscalls 220-225 (HypervisorCap ZST-gated) ────────────────
        Syscall::CreateVm { guest_pages } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // An empty carve disables the nested table's SAS check, and the
            // window's byte size must be representable for every later bound.
            if guest_pages == 0
                || guest_pages
                    .checked_mul(crate::memory::paging::PAGE_SIZE)
                    .is_none()
            {
                return Err(SyscallError::InvalidInput);
            }
            crate::hypervisor::registry::create_vm(caller_id, guest_pages)
                .map_err(|_| SyscallError::NotSupported)
        }

        Syscall::CreateVcpu { vm_id, entry_pc } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::hypervisor::registry::create_vcpu(caller_id, vm_id, entry_pc)
                .map_err(|_| SyscallError::NotSupported)
        }

        Syscall::MapGuestMemory {
            vm_id,
            ipa,
            size,
            writable,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // C3: overflow guard on IPA + size.
            ipa.checked_add(size as u64)
                .ok_or(SyscallError::InvalidInput)?;
            // C-2: the range is backed from the start of the guest carve, so
            // it may never be larger than the carve (the table re-checks).
            let (_, guest_bytes) = crate::hypervisor::registry::guest_window(caller_id, vm_id)
                .map_err(|_| SyscallError::NotSupported)?;
            if size == 0 || size > guest_bytes {
                return Err(SyscallError::InvalidInput);
            }
            crate::hypervisor::registry::map_guest_memory(caller_id, vm_id, ipa, size, writable)
                .map(|_| 0usize)
                .map_err(|_| SyscallError::NotSupported)
        }

        Syscall::RunVcpu {
            vm_id,
            vcpu_id,
            budget_ns,
            out_ptr,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            use crate::hypervisor::registry::vmm_wire;
            validate_user_buf(out_ptr, vmm_wire::EXIT_LEN, MAX_USER_BUF)?;
            let view = caller_copy_view(caller_id)?;
            // Prove the record is writable before the guest runs: an exit
            // that cannot be delivered would be lost with the guest advanced.
            view.write_bytes(out_ptr, &[0u8; vmm_wire::EXIT_LEN])
                .map_err(|_| SyscallError::InvalidInput)?;
            let exit = crate::hypervisor::registry::run_vcpu(caller_id, vm_id, vcpu_id, budget_ns)
                .map_err(|_| SyscallError::NotSupported)?;
            view.write_bytes(out_ptr, &vmm_wire::encode_exit(&exit))
                .map_err(|_| SyscallError::InvalidInput)?;
            Ok(0)
        }

        Syscall::VcpuRegs {
            vm_id,
            vcpu_id,
            buf_ptr,
            mode,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // All modes access the full 32-word (256-byte) user buffer.
            // Mode 2 is Pi-only, read-only, and clears unused words.
            #[cfg(target_arch = "aarch64")]
            let max_mode =
                <hal::aarch64::vcpu::VcpuExt as hal::aarch64::backend::ArmVcpuExt>::REGS_MAX_MODE;
            #[cfg(not(target_arch = "aarch64"))]
            let max_mode = 1;
            if mode > max_mode {
                return Err(SyscallError::InvalidInput);
            }
            use crate::hypervisor::registry::vmm_wire;
            validate_user_buf(buf_ptr, vmm_wire::REGS_LEN, MAX_USER_BUF)?;
            let view = caller_copy_view(caller_id)?;
            // Read the buffer in every mode: mode 1 consumes it, and the read
            // modes leave words the backend does not model as the cell had them.
            let mut bytes = [0u8; vmm_wire::REGS_LEN];
            view.read_into(buf_ptr, &mut bytes)
                .map_err(|_| SyscallError::InvalidInput)?;
            let mut regs = vmm_wire::regs_from_bytes(&bytes);
            let result =
                crate::hypervisor::registry::vcpu_regs(caller_id, vm_id, vcpu_id, &mut regs, mode)
                    .map_err(|_| SyscallError::NotSupported)?;
            if mode != 1 {
                view.write_bytes(buf_ptr, &vmm_wire::regs_to_bytes(&regs))
                    .map_err(|_| SyscallError::InvalidInput)?;
            }
            Ok(result)
        }

        Syscall::InjectIrq {
            vm_id,
            vcpu_id,
            intid,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // m3: GICv2 has SPIs/PPIs/SGIs up to intid 1019.
            if intid > 1019 {
                return Err(SyscallError::InvalidInput);
            }
            crate::hypervisor::registry::inject_irq(caller_id, vm_id, vcpu_id, intid)
                .map_err(|_| SyscallError::NotSupported)
        }

        Syscall::WriteGuestMemory {
            vm_id,
            gpa,
            src_ptr,
            len,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            validate_user_buf(src_ptr, len, MAX_USER_BUF)?;
            check_guest_range(caller_id, vm_id, gpa, len)?;
            let view = caller_copy_view(caller_id)?;
            let mut staging = guest_copy_staging(len)?;
            let mut done = 0;
            while done < len {
                let chunk = &mut staging[..(len - done).min(GUEST_COPY_CHUNK)];
                view.read_into(src_ptr + done, chunk)
                    .map_err(|_| SyscallError::InvalidInput)?;
                crate::hypervisor::registry::write_guest_memory(
                    caller_id,
                    vm_id,
                    gpa + done as u64,
                    chunk,
                )
                .map_err(|_| SyscallError::InvalidInput)?;
                done += chunk.len();
            }
            Ok(len)
        }

        Syscall::ReadGuestMemory {
            vm_id,
            gpa,
            dst_ptr,
            len,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            validate_user_buf(dst_ptr, len, MAX_USER_BUF)?;
            check_guest_range(caller_id, vm_id, gpa, len)?;
            let view = caller_copy_view(caller_id)?;
            let mut staging = guest_copy_staging(len)?;
            let mut done = 0;
            while done < len {
                let chunk = &mut staging[..(len - done).min(GUEST_COPY_CHUNK)];
                crate::hypervisor::registry::read_guest_memory(
                    caller_id,
                    vm_id,
                    gpa + done as u64,
                    chunk,
                )
                .map_err(|_| SyscallError::InvalidInput)?;
                view.write_bytes(dst_ptr + done, chunk)
                    .map_err(|_| SyscallError::InvalidInput)?;
                done += chunk.len();
            }
            Ok(len)
        }
    }
}

/// Staging granule for guest-memory copies. Cell bytes cross the copy view
/// into a kernel buffer of at most this size; the registry only ever sees that
/// buffer, never a cell address.
const GUEST_COPY_CHUNK: usize = 256 * 1024;

/// Refuse a guest-memory request unless `[gpa, gpa + len)` lies wholly inside
/// the VM's guest-RAM window, so no chunk is copied for a request that would
/// fail part-way on bounds.
fn check_guest_range(
    caller_id: usize,
    vm_id: usize,
    gpa: u64,
    len: usize,
) -> Result<(), SyscallError> {
    let (base, bytes) = crate::hypervisor::registry::guest_window(caller_id, vm_id)
        .map_err(|_| SyscallError::InvalidInput)?;
    let end = gpa
        .checked_sub(base)
        .and_then(|offset| offset.checked_add(len as u64))
        .ok_or(SyscallError::InvalidInput)?;
    if end > bytes as u64 {
        return Err(SyscallError::InvalidInput);
    }
    Ok(())
}

/// A zeroed kernel staging buffer for a `len`-byte guest-memory copy.
fn guest_copy_staging(len: usize) -> Result<Vec<u8>, SyscallError> {
    let size = len.min(GUEST_COPY_CHUNK);
    let mut staging = Vec::new();
    staging
        .try_reserve_exact(size)
        .map_err(|_| SyscallError::OutOfMemory)?;
    staging.resize(size, 0);
    Ok(staging)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_registered_grant_owner(reg_id: usize) -> Option<usize> {
    let tbl = reg_grant_table_lock().lock();
    tbl.as_ref()
        .and_then(|map| map.get(&reg_id))
        .map(|grant| grant.owner)
}

#[cfg(not(target_arch = "riscv32"))]
use crate::hal::arch::ViTrapFrame;
use api::syscall::ViSyscall;

/// Map a syscall ID + promoted register args to the internal [`Syscall`] enum.
///
/// All register values must already be promoted to `usize` by the caller.
/// Returns `None` for unknown/unhandled opcodes; the caller writes the
/// arch-appropriate sentinel (usize::MAX or u32::MAX) to the return register.
fn map_syscall(syscall_id: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> Option<Syscall> {
    let sc = match ViSyscall::from(syscall_id) {
        ViSyscall::IpcSubmit => Syscall::IpcSubmit {
            target: a0,
            ptr: a1,
            len: a2,
        },
        ViSyscall::IpcTake => Syscall::IpcTake {
            token: a0,
            ptr: a1,
            len: a2,
            status_ptr: a3,
        },
        ViSyscall::IpcWait => Syscall::IpcWait {
            ticks: (a0 as u64) | ((a1 as u64) << 32),
        },
        ViSyscall::IpcCancel => Syscall::IpcCancel { token: a0 },
        ViSyscall::IpcCurrent => Syscall::IpcCurrent,
        ViSyscall::IpcReply => Syscall::IpcReply {
            token: a0,
            ptr: a1,
            len: a2,
        },
        ViSyscall::Send => Syscall::Send {
            target: a0,
            msg_ptr: a1,
            msg_len: a2,
        },
        ViSyscall::TrySend => Syscall::TrySend {
            target: a0,
            msg_ptr: a1,
            msg_len: a2,
        },
        ViSyscall::Recv => Syscall::Recv {
            mask: a0,
            buf_ptr: a1,
            buf_len: a2,
            attest_caller: a3 & api::caller_identity::RECV_ATTEST_CALLER != 0,
        },
        ViSyscall::TryRecv => Syscall::TryRecv {
            mask: a0,
            buf_ptr: a1,
            buf_len: a2,
            attest_caller: a3 & api::caller_identity::RECV_ATTEST_CALLER != 0,
        },
        ViSyscall::SendGather => Syscall::SendGather {
            target: a0,
            iovec_ptr: a1,
            iovec_count: a2,
        },
        ViSyscall::RecvScatter => Syscall::RecvScatter {
            mask: a0,
            iovec_ptr: a1,
            iovec_count: a2,
        },
        ViSyscall::RecvTimeout => Syscall::RecvTimeout {
            mask: a0,
            buf_ptr: a1,
            buf_len: a2,
            deadline: (super::system_ticks() as u64).wrapping_add(a3 as u64),
        },
        ViSyscall::CellSessionV1 => Syscall::CellSessionV1 {
            request_ptr: a0,
            request_len: a1,
            result_ptr: a2,
            result_capacity: a3,
        },
        ViSyscall::GetCpuCount => Syscall::GetCpuCount,
        ViSyscall::Reply => Syscall::Reply {
            caller: a0,
            result: a1,
        },
        ViSyscall::Call => Syscall::ServiceLookup {
            name_ptr: a0,
            name_len: a1,
        },
        ViSyscall::Spawn => Syscall::Spawn { entry: a0, arg: a1 },
        ViSyscall::Exec => Syscall::Exec {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::SpawnFromMem => Syscall::SpawnFromMem { args_ptr: a0 },
        ViSyscall::SpawnFromPath => Syscall::SpawnFromPath {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::SpawnFromElf => Syscall::SpawnFromElf {
            grant_id: a0,
            len: a1,
            path_ptr: a2,
            path_len: a3,
        },
        ViSyscall::SpawnPinned => Syscall::SpawnPinned {
            path_ptr: a0,
            path_len: a1,
            priority: a2 as u8,
            core_id: a3,
        },
        ViSyscall::ResolveCellOwnerRecord => Syscall::ResolveCellOwnerRecord {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::WatchCellOwnerRecord => Syscall::WatchCellOwnerRecord {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::ResolveCellImage => Syscall::ResolveCellImage {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::SpawnSetDirs => Syscall::SpawnSetDirs { carrier_ptr: a0 },
        ViSyscall::QueryDirHandles => Syscall::QueryDirHandles {
            cell_id: a0 as u64,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::ResolveCellOwner => Syscall::ResolveCellOwner {
            cell_id: a0 as u64,
            generation: a1 as u64,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::WatchCellOwner => Syscall::WatchCellOwner {
            cell_id: a0 as u64,
            generation: a1 as u64,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::CancelCellOwnerWatch => Syscall::CancelCellOwnerWatch { token: a0 as u64 },
        ViSyscall::OpenCap => Syscall::OpenCap {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::ReadCap => Syscall::ReadCap {
            cap_id: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::CloseCap => Syscall::CloseCap { cap_id: a0 },
        ViSyscall::SeekCap => Syscall::SeekCap {
            cap_id: a0,
            offset: a1,
            whence: a2,
        },
        ViSyscall::WriteCap => Syscall::WriteCap {
            cap_id: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StatCap => Syscall::StatCap { cap_id: a0 },
        ViSyscall::TruncateCap => Syscall::TruncateCap {
            cap_id: a0,
            len: a1,
        },
        ViSyscall::SyncCap => Syscall::SyncCap { cap_id: a0 },
        ViSyscall::GrantDma => Syscall::GrantDma {
            bdf: a0 as u32,
            phys: a1 as u64,
            size: a2,
        },
        ViSyscall::Wait => Syscall::Wait { pid: a0 },
        ViSyscall::SetTlsBase => Syscall::SetTlsBase { base: a0 },
        ViSyscall::FutexWait => Syscall::FutexWait {
            addr: a0,
            expected: a1 as u32,
            timeout_ticks: a2 as u64,
        },
        ViSyscall::FutexWake => Syscall::FutexWake {
            addr: a0,
            count: a1,
        },
        ViSyscall::PipeCreate => Syscall::PipeCreate {
            capacity: a0,
            out_ptr: a1,
        },
        ViSyscall::PipeRead => Syscall::PipeRead {
            handle: a0,
            buf_ptr: a1,
            len: a2,
            timeout_ticks: a3 as u64,
        },
        ViSyscall::PipeWrite => Syscall::PipeWrite {
            handle: a0,
            buf_ptr: a1,
            len: a2,
            timeout_ticks: a3 as u64,
        },
        ViSyscall::PipeClose => Syscall::PipeClose { handle: a0 },
        ViSyscall::PipeShare => Syscall::PipeShare {
            handle: a0,
            target_tid: a1,
        },
        ViSyscall::ShmAlloc => Syscall::ShmAlloc { size: a0 },
        ViSyscall::ShmMap => Syscall::ShmMap {
            handle: a0,
            target_pid: a1,
        },
        ViSyscall::Exit => Syscall::Exit { code: a0 },
        ViSyscall::ForceExit => Syscall::ForceExit { tid: a0 },
        ViSyscall::NotifyOnExit => Syscall::NotifyOnExit { watched: a0 },
        ViSyscall::RegisterService => Syscall::RegisterService {
            service_id: a0 as u16,
            tid: a1,
        },
        ViSyscall::LookupService => Syscall::LookupService {
            service_id: a0 as u16,
        },
        ViSyscall::LookupServiceBound => Syscall::LookupServiceBound {
            service_id: a0 as u16,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::Heartbeat => Syscall::Heartbeat { interval: a0 },
        ViSyscall::Yield => Syscall::Yield,
        ViSyscall::SetTimer => Syscall::SetTimer { deadline: a0 },
        ViSyscall::Log => Syscall::Log {
            msg_ptr: a0,
            msg_len: a1,
        },
        ViSyscall::GetProcs => Syscall::GetProcs {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::GetProcs2 => Syscall::GetProcs2 {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::MemInfo => Syscall::MemInfo {
            out_ptr: a0,
            out_len: a1,
        },
        ViSyscall::TelemetrySnapshot => Syscall::TelemetrySnapshot {
            out_ptr: a0,
            out_len: a1,
            flags: a2,
            reserved: a3,
        },
        ViSyscall::QueryRetirement => Syscall::QueryRetirement {
            root_tid: a0,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::LookupBlockRoute => Syscall::LookupBlockRoute {
            index: a0,
            out_ptr: a1,
            out_len: a2,
            reserved: a3,
        },
        ViSyscall::QueryMachineFacts => Syscall::QueryMachineFacts {
            out_ptr: a0,
            out_len: a1,
            a2,
            a3,
        },
        ViSyscall::QueryPcieFunction => Syscall::QueryPcieFunction {
            bdf: a0,
            out_ptr: a1,
            out_len: a2,
            reserved: a3,
        },
        ViSyscall::AllocPcieIrq => Syscall::AllocPcieIrq {
            bdf: a0,
            want: a1,
            mode: a2,
            reserved: a3,
        },
        ViSyscall::SystemPower => Syscall::SystemPower {
            action: a0,
            a1,
            a2,
            a3,
        },
        ViSyscall::ExportDmaRange => Syscall::ExportDmaRange {
            va: a0,
            len: a1,
            holder_service: a2,
            flags: a3,
        },
        ViSyscall::MapDmaExport => Syscall::MapDmaExport {
            export_id: a0,
            bdf: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::RevokeDmaExport => Syscall::RevokeDmaExport {
            export_id: a0,
            a1,
            a2,
            a3,
        },
        ViSyscall::Open => Syscall::Open {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::Read => Syscall::Read {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Close => Syscall::Close { fd: a0 },
        ViSyscall::ReadDir => Syscall::ReadDir {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Write => Syscall::Write {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Seek => Syscall::Seek {
            fd: a0,
            offset: a1 as isize,
            whence: a2,
        },
        ViSyscall::Fstat => Syscall::Fstat {
            fd: a0,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::Rename => Syscall::Rename {
            old_ptr: a0,
            old_len: a1,
            new_ptr: a2,
            new_len: a3,
        },
        ViSyscall::FileOp => Syscall::FileOp {
            op: a0,
            arg1: a1,
            arg2: a2,
        },
        ViSyscall::GetTime => Syscall::GetTime { op: a0 },
        ViSyscall::GpuFlush => Syscall::GpuFlush {
            data_ptr: a0,
            data_len: a1,
            xy: a2,
            wh: a3,
        },
        ViSyscall::AudioPlay => Syscall::AudioPlay {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::CapRevoke => Syscall::CapRevoke {
            target_tid: a0,
            cap_mask: a1 as u32,
        },
        ViSyscall::GpuCursor => Syscall::GpuCursor {
            op: a0,
            data_ptr: a1,
            xy: a2,
            hot: a3,
        },
        ViSyscall::GpuGetResolution => Syscall::GpuGetResolution,
        ViSyscall::NetTx => Syscall::NetTx {
            frame_ptr: a0,
            frame_len: a1,
        },
        ViSyscall::NetRx => Syscall::NetRx {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::StateStash => Syscall::StateStash {
            key: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StateRestore => Syscall::StateRestore {
            key: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StateStashClear => Syscall::StateStashClear { key: a0 },
        ViSyscall::FreezeCell => Syscall::FreezeCell { target_tid: a0 },
        ViSyscall::ResumeCell => Syscall::ResumeCell {
            target_tid: a0,
            source_tid: a1,
            service_id: a2,
            reserved: a3,
        },
        ViSyscall::KillCell => Syscall::KillCell {
            target_tid: a0,
            exit_code: a1 as u32,
        },
        ViSyscall::RegisterBlockDriver => Syscall::RegisterBlockDriver {
            kind: a0,
            id: a1,
            reserved: (a2, a3),
        },
        ViSyscall::RegisterNicDriver => Syscall::RegisterNicDriver,
        ViSyscall::RegisterUsbHidProducer => Syscall::RegisterUsbHidProducer,
        ViSyscall::FindPcieDevice => Syscall::FindPcieDevice {
            class: a0 as u8,
            subclass: a1 as u8,
            prog_if: a2 as u8,
            out_ptr: a3,
        },
        ViSyscall::FindPcieDeviceByVendor => Syscall::FindPcieDeviceByVendor {
            vendor_id: a0 as u16,
            device_id: a1 as u16,
            out_ptr: a2,
        },
        ViSyscall::SerialPortInfo => Syscall::SerialPortInfo {
            index: a0 as u32,
            out_ptr: a1,
        },
        ViSyscall::SerialWrite => Syscall::SerialWrite {
            index: a0 as u32,
            ptr: a1,
            len: a2,
        },
        ViSyscall::SerialRead => Syscall::SerialRead {
            index: a0 as u32,
            ptr: a1,
            len: a2,
        },
        ViSyscall::SerialConfigure => Syscall::SerialConfigure {
            index: a0 as u32,
            baud: a1 as u32,
        },
        ViSyscall::QueryHotswapReady => Syscall::QueryHotswapReady { target_tid: a0 },
        ViSyscall::SpawnReplacement => Syscall::SpawnReplacement {
            old_tid: a0,
            path_ptr: a1,
            path_len: a2,
        },
        ViSyscall::PauseService => Syscall::PauseService {
            service_id: a0 as u16,
            expected_tid: a1,
        },
        ViSyscall::HotSwapReady => Syscall::HotSwapReady,
        ViSyscall::Snapshot => Syscall::Snapshot,
        ViSyscall::GrantAlloc => Syscall::GrantAlloc { size: a0 },
        ViSyscall::GrantShare => Syscall::GrantShare {
            grant_id: a0,
            target_cell: a1,
            perm: a2,
        },
        ViSyscall::GrantSlice => Syscall::GrantSlice {
            grant_id: a0,
            size_out_ptr: a1,
            expected_owner_tid: a2,
            flags: a3,
        },
        ViSyscall::GrantFree => Syscall::GrantFree { grant_id: a0 },
        ViSyscall::GrantCacheSyncBegin => Syscall::GrantCacheSyncBegin {
            grant_id: a0,
            offset: a1,
            len: a2,
        },
        ViSyscall::GrantCacheSyncComplete => Syscall::GrantCacheSyncComplete { token: a0 },
        ViSyscall::RegisterDisplayFramebuffer => Syscall::RegisterDisplayFramebuffer {
            base: a0,
            size: a1,
            packed_dimensions: a2,
            pitch: a3,
        },
        ViSyscall::BlkReadAsync => Syscall::BlkReadAsync {
            sector: a0 as u64,
            grant_id: a1,
        },
        ViSyscall::RequestMmio => Syscall::RequestMmio { base: a0, len: a1 },
        ViSyscall::ProbeVirtioMmio => Syscall::ProbeVirtioMmio {
            base: a0,
            reserved: (a1, a2, a3),
        },
        ViSyscall::GetRandom => Syscall::GetRandom {
            buf_ptr: a0,
            len: a1,
        },
        ViSyscall::GrantRegister => Syscall::GrantRegister { size: a0 },
        ViSyscall::GrantUnregister => Syscall::GrantUnregister { reg_id: a0 },
        ViSyscall::WaitForEvent => {
            // ABI: a0 = mask (u32), a1 = timeout_ticks_lo, a2 = timeout_ticks_hi.
            let mask = a0 as u32;
            let timeout = (a1 as u64) | ((a2 as u64) << 32);
            let deadline = if timeout == 0 {
                None
            } else {
                Some((super::system_ticks() as u64).wrapping_add(timeout))
            };
            Syscall::WaitForEvent { mask, deadline }
        }
        ViSyscall::WaitCompletion => {
            // ABI: a0 = source mask (u32), a1 = timeout_ticks_lo,
            // a2 = timeout_ticks_hi, a3 = pointer to the result record.
            // Bits 32..63 of a0 are reserved and must be zero: a mask that
            // does not fit becomes `u32::MAX`, which the source rule refuses
            // with InvalidInput instead of silently dropping the high bits.
            let mask = u32::try_from(a0).unwrap_or(u32::MAX);
            let timeout = (a1 as u64) | ((a2 as u64) << 32);
            let deadline = if timeout == 0 {
                None
            } else {
                Some((super::system_ticks() as u64).wrapping_add(timeout))
            };
            Syscall::WaitCompletion {
                mask,
                deadline,
                out_ptr: a3,
            }
        }
        // Hypervisor syscalls 220-225.
        ViSyscall::CreateVm => Syscall::CreateVm { guest_pages: a0 },
        ViSyscall::CreateVcpu => Syscall::CreateVcpu {
            vm_id: a0,
            entry_pc: a1 as u64,
        },
        ViSyscall::MapGuestMemory => Syscall::MapGuestMemory {
            vm_id: a0,
            ipa: a1 as u64,
            size: a2,
            writable: a3 != 0,
        },
        ViSyscall::RunVcpu => Syscall::RunVcpu {
            vm_id: a0,
            vcpu_id: a1,
            budget_ns: a2 as u64,
            out_ptr: a3,
        },
        ViSyscall::VcpuRegs => Syscall::VcpuRegs {
            vm_id: a0,
            vcpu_id: a1,
            buf_ptr: a2,
            mode: a3,
        },
        ViSyscall::InjectIrq => Syscall::InjectIrq {
            vm_id: a0,
            vcpu_id: a1,
            intid: a2 as u32,
        },
        ViSyscall::WriteGuestMemory => Syscall::WriteGuestMemory {
            vm_id: a0,
            gpa: a1 as u64,
            src_ptr: a2,
            len: a3,
        },
        ViSyscall::ReadGuestMemory => Syscall::ReadGuestMemory {
            vm_id: a0,
            gpa: a1 as u64,
            dst_ptr: a2,
            len: a3,
        },
        ViSyscall::WaitIrq => Syscall::WaitIrq {
            irq: a0 as u8,
            mmio_base: a1,
        },
        ViSyscall::RegisterPcieBar => Syscall::RegisterPcieBar {
            bdf: a0 as u32,
            base: a1,
            len: a2,
        },
        ViSyscall::RegisterPciDevice => Syscall::RegisterPciDevice {
            bdf: a0 as u32,
            cls: a1 as u32,
            bar0_base: a2,
            bar0_size: a3,
        },
        ViSyscall::ReadLog => Syscall::ReadLog {
            buf_ptr: a0,
            max: a1,
            source: a2,
        },
        ViSyscall::Chdir => Syscall::ChDir {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::Getcwd => Syscall::GetCwd {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::AnonMemory => Syscall::AnonMemory {
            op: a0,
            addr: a1,
            len: a2,
            flags: a3,
        },
        _ => match syscall_id {
            3 => Syscall::SetTimer { deadline: a0 },
            100 => Syscall::ServiceLookup {
                name_ptr: a0,
                name_len: a1,
            },
            110 => Syscall::MkDir {
                path_ptr: a0,
                path_len: a1,
            },
            111 => Syscall::Create {
                path_ptr: a0,
                path_len: a1,
            },
            // Block I/O — intentionally absent from ViSyscall/libs/api (avoids Law 1).
            255 => Syscall::Rename {
                old_ptr: a0,
                old_len: a1,
                new_ptr: a2,
                new_len: a3,
            },
            500 => Syscall::BlkRead {
                sector: a0 as u64,
                buf_ptr: a1,
            },
            501 => Syscall::BlkWrite {
                sector: a0 as u64,
                buf_ptr: a1,
            },
            502 => Syscall::Shutdown,
            503 => Syscall::BlkFlush,
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::OPCODE => Syscall::BenchCounters {
                out_ptr: a0,
                out_len: a1,
            },
            _ => return None,
        },
    };
    Some(sc)
}
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
fn with_test_sum<T>(operation: impl FnOnce() -> T) -> T {
    let saved_sstatus: usize;
    unsafe {
        // SAFETY: test-only dispatch must reproduce the real trap entry's SUM state.
        core::arch::asm!(
            "csrr {saved_sstatus}, sstatus",
            "csrs sstatus, {sum}",
            saved_sstatus = out(reg) saved_sstatus,
            sum = in(reg) 0x40000_usize,
            options(nostack)
        );
    }
    let result = operation();
    if saved_sstatus & 0x40000 == 0 {
        unsafe {
            // SAFETY: restore exactly the caller hart's prior SUM bit.
            core::arch::asm!(
                "csrc sstatus, {sum}",
                sum = in(reg) 0x40000_usize,
                options(nostack)
            );
        }
    }
    result
}

/// Exercise the production raw-opcode decoder and handler without a trap frame.
///
/// Test hooks use this only for in-kernel QEMU fixtures whose calls would
/// otherwise need a hand-written U-mode trampoline. RV64's real syscall entry
/// runs with `sstatus.SUM` set; mirror that per-hart state around the handler.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn dispatch_raw_for_test(
    caller_id: usize,
    syscall_id: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> SyscallResult {
    check_allowlist(syscall_id, caller_id)?;
    let syscall = map_syscall(syscall_id, a0, a1, a2, a3).ok_or(SyscallError::InvalidCommand)?;
    with_test_sum(|| handle_syscall(caller_id, syscall))
}

/// Per-cell syscall allowlist gate.
///
/// Reads the caller's `syscall_allowlist` bitmask and returns
/// `Err(SyscallError::PermissionDenied)` if the opcode's bit is not set, or if
/// the opcode is one of the S2-T telemetry calls and the caller's CapSet lacks
/// the matching right (`observe_right_held`).
/// The SCHEDULER lock is acquired and released here — callers must NOT hold it.
fn check_allowlist(syscall_id: usize, caller_id: usize) -> Result<(), SyscallError> {
    let sc = ViSyscall::from(syscall_id);
    // `bench-counters` raw 504 borrows MemInfo's gate exactly: allowlist bit 56
    // and the `observe_memory` right. Without the feature 504 stays unknown.
    #[cfg(feature = "bench-counters")]
    let gate_sc = if syscall_id == crate::bench_counters::OPCODE {
        ViSyscall::MemInfo
    } else {
        sc
    };
    #[cfg(not(feature = "bench-counters"))]
    let gate_sc = sc;
    let bit = gate_sc.allowlist_bit();
    // Bit 36 gates raw block-I/O opcodes (500/501/503) and BlkReadAsync (212).
    let blk_io_bit: Option<u8> = if matches!(syscall_id, 500 | 501 | 503 | 212) {
        Some(36)
    } else {
        None
    };
    let rename_bit: Option<u8> = if syscall_id == 255 { Some(62) } else { None };

    // Raw opcodes with a dedicated `map_syscall` fallback mapping. These are
    // intentionally absent from `ViSyscall` (Law 1: keeps experimental ids out
    // of the stable ABI) so they decode as `Unknown` — but they are NOT unknown:
    // 500/501/503 are gated by bit 36 below + the ZST BlockIoCap at the handler;
    // 502 and the legacy raw ops (3/100/110/111) predate the bitmap and stay
    // always-permitted, matching their pre-Phase-31b behavior.
    let known_raw = matches!(syscall_id, 3 | 100 | 110 | 111 | 500 | 501 | 502 | 503)
        || (cfg!(feature = "bench-counters") && syscall_id == 504);

    let Some(allowlist) = syscall_allowlist_for(caller_id) else {
        log::warn!(
            "[kernel] syscall opcode {} denied for non-live tid {}",
            syscall_id,
            caller_id
        );
        return Err(SyscallError::PermissionDenied);
    };

    // Deny truly-unknown opcodes that land in the legacy inner-match fallback —
    // their allowlist_bit() returns None, so without this guard they bypass the
    // check. Exit (60) and Yield (104) are always permitted unconditionally.
    // Known-raw ids are exempt: blocking them here made every allowlist-declaring
    // cell lose raw block I/O (broke the VFS FAT32 mount silently since Phase 31b).
    // Every deny below logs: a silent dispatch-level denial cost a full day of
    // triage when the shell's missing `Read` bit bricked serial input with no
    // kernel output at all.
    if sc == ViSyscall::Unknown
        && !known_raw
        && !matches!(syscall_id, 60 | 104)
        && allowlist != u64::MAX
    {
        log::warn!(
            "[kernel] unknown opcode {} denied for tid {} (allowlist={:#018x})",
            syscall_id,
            caller_id,
            allowlist
        );
        return Err(SyscallError::PermissionDenied);
    }
    if let Some(b) = bit.or(rename_bit) {
        if allowlist & (1u64 << b) == 0 {
            log::warn!(
                "[kernel] syscall {:?} (bit {}) denied for tid {} (allowlist={:#018x})",
                sc,
                b,
                caller_id,
                allowlist
            );
            return Err(SyscallError::PermissionDenied);
        }
    }
    if let Some(b) = blk_io_bit {
        if allowlist & (1u64 << b) == 0 {
            log::warn!(
                "[kernel] raw block opcode {} denied for tid {} (no bit 36)",
                syscall_id,
                caller_id
            );
            return Err(SyscallError::PermissionDenied);
        }
    }
    if !observe_right_held(gate_sc, caller_id) {
        log::warn!(
            "[kernel] syscall {:?} denied for tid {}: edge withholds the S2-T observation right",
            sc,
            caller_id
        );
        return Err(SyscallError::PermissionDenied);
    }
    Ok(())
}

/// Annex S2-T/S2-W §3: `GetProcs` and `GetProcs2` additionally need the
/// caller's `observe_tasks` right and `MemInfo` its `observe_memory` right, on
/// top of the allowlist bit (and whatever a permit-all allowlist says). Every
/// other opcode needs none, and neither does the kernel-context sentinel (tid 0),
/// which has no task. A missing or terminal task is denied.
fn observe_right_held(sc: ViSyscall, caller_id: usize) -> bool {
    let held: fn(&super::cap::CapSet) -> bool = match sc {
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

#[cfg(not(target_arch = "riscv32"))]
#[no_mangle]
#[allow(non_snake_case)] // ABI name required by the HAL trap vector — cannot be snake_case
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame) {
    #[cfg(all(feature = "x86-idt-cpl3-test", target_arch = "x86_64"))]
    if crate::hal::idt::handle_cpl3_probe_syscall(frame) {
        return;
    }

    #[cfg(feature = "bench-counters")]
    {
        crate::bench_counters::syscall();
        // The SYSCALL stub parks the interrupted CR3 in `scause` and, exactly
        // when it differs from a recorded kernel CR3, writes CR3 on entry and
        // again on exit. Mirror that test here (synthetic in-kernel frames
        // carry 0 and are not counted).
        #[cfg(target_arch = "x86_64")]
        {
            let kernel_cr3 = crate::hal::domain::kernel_cr3();
            if frame.scause != 0 && kernel_cr3 != 0 && frame.scause != kernel_cr3 {
                crate::bench_counters::domain_syscall_cr3();
            }
        }
    }

    let syscall_id = frame.regs[17];

    // Watchdog progress signal: a syscall proves the caller is making progress
    // (ViCell cells are poll-based — try_recv/yield every loop iteration), so
    // reset its CPU-monopoly counter.
    {
        let hart_id = super::hart_local::current_hart_id();
        let cid = super::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.run_ticks = 0;
                    t.rt_overrun_warned = false;
                }
            }
        }
    }

    let a0 = frame.regs[10];
    let a1 = frame.regs[11];
    let a2 = frame.regs[12];
    let a3 = frame.regs[13];

    let Some(syscall) = map_syscall(syscall_id, a0, a1, a2, a3) else {
        frame.regs[10] = usize::MAX;
        return;
    };

    let caller_id = super::current_task_id();

    if check_allowlist(syscall_id, caller_id).is_err() {
        frame.regs[10] = usize::MAX;
        return;
    }

    // SAFETY: csrs/csrc sstatus SUM (bit 18) enables S-mode access to user pages
    // for the duration of handle_syscall. Disabled immediately after to prevent
    // inadvertent user-page reads on subsequent kernel faults.
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x40000usize);
    }

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let pcie_function = is_pcie_function_call(&syscall);
    let power_call = matches!(syscall, Syscall::SystemPower { .. });
    let dma_export_call = is_dma_export_call(&syscall);
    let result = handle_syscall(caller_id, syscall);

    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

    frame.regs[10] = if pcie_function {
        encode_pcie_function_result(result)
    } else if power_call {
        encode_system_power_result(result)
    } else if dma_export_call {
        encode_dma_export_result(result)
    } else if typed_errors {
        encode_ipc_result(result)
    } else {
        encode_syscall_result(result, usize::MAX, supports_typed_oom)
    };
}

#[cfg(not(target_arch = "riscv32"))]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(target_arch = "riscv32")]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut crate::hal::arch::ViTrapFrame) {
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::syscall();
    // Promote u32 register slots to usize (= u32 on rv32) for arch-agnostic helpers.
    let syscall_id = frame.regs[17] as usize;

    // Watchdog: syscall proves the cell is making forward progress.
    {
        let hart_id = super::hart_local::current_hart_id();
        let cid = super::hart_local::ready::current_task_id_for(hart_id);
        if cid > 0 {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&cid) {
                    t.run_ticks = 0;
                    t.rt_overrun_warned = false;
                }
            }
        }
    }

    let a0 = frame.regs[10] as usize;
    let a1 = frame.regs[11] as usize;
    let a2 = frame.regs[12] as usize;
    let a3 = frame.regs[13] as usize;

    let Some(syscall) = map_syscall(syscall_id, a0, a1, a2, a3) else {
        frame.regs[10] = u32::MAX;
        return;
    };

    let caller_id = super::current_task_id();

    if check_allowlist(syscall_id, caller_id).is_err() {
        frame.regs[10] = u32::MAX;
        return;
    }

    // SAFETY: csrs/csrc sstatus SUM (bit 18) — same bit position on RV32 and RV64
    // per the RISC-V Privileged Spec §4.1.1. Disabled after handle_syscall.
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x40000usize);
    }

    let supports_typed_oom = supports_typed_oom(&syscall);
    let typed_errors = has_typed_errors(&syscall);
    let result = handle_syscall(caller_id, syscall);

    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

    frame.regs[10] = if typed_errors {
        encode_ipc_result(result) as u32
    } else {
        encode_syscall_result(result, u32::MAX as usize, supports_typed_oom) as u32
    };
}

#[cfg(target_arch = "riscv32")]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(test)]
#[path = "grant-owner-tests.rs"]
mod grant_owner_tests;

#[cfg(test)]
#[path = "grant-lease-tests.rs"]
mod grant_lease_tests;

#[cfg(test)]
mod tests {
    use super::{
        check_allowlist, consumes_staged_spawn_argv, encode_syscall_result, map_syscall,
        page_grant_authorizes_dma, reg_grant_authorizes_dma, staged_spawn_argv_cleanup,
        supports_typed_oom, syscall_to_vi, withhold_or_free, PageGrant, RegGrant, Syscall,
        SyscallError,
    };
    use crate::task::cap::CapSet;
    use crate::task::{scheduler::Scheduler, tcb::Task, SCHEDULER};
    use api::syscall::ViSyscall;
    use types::CellId;

    /// One live task holding the pre-annex rights (`LEGACY_OBSERVE_MUTATE`), so
    /// the allowlist cases below test the allowlist bit alone.
    fn with_scheduler_task<F>(allowlist: u64, f: F)
    where
        F: FnOnce(usize),
    {
        with_scheduler_task_caps(allowlist, CapSet::LEGACY_OBSERVE_MUTATE, f)
    }

    fn with_scheduler_task_caps<F>(allowlist: u64, caps: CapSet, f: F)
    where
        F: FnOnce(usize),
    {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut previous = SCHEDULER.lock();
        let saved = previous.take();
        let mut scheduler = Scheduler::new();
        let tid = 1;
        let mut task = alloc::boxed::Box::new(Task::new(
            tid,
            CellId(7),
            "allowlist-test",
            alloc::vec::Vec::new(),
        ));
        task.syscall_allowlist = allowlist;
        caps.apply_to(&mut task);
        scheduler.tasks.insert(tid, task);
        *previous = Some(scheduler);
        drop(previous);

        f(tid);

        let mut restore = SCHEDULER.lock();
        *restore = saved;
    }

    /// Annex S2-T/S2-W §6: the allowlist bit is necessary but no longer
    /// sufficient — a task that declares GetProcs (14), GetProcs2 (55) and
    /// MemInfo (56), even permit-all, is refused when its edge withheld X/M.
    /// Phase 06 S6-1: `TelemetrySnapshot` (bit 55) is gated by X exactly like
    /// `GetProcs2`; M alone never admits it.
    #[test]
    fn telemetry_syscalls_need_the_observe_rights_beyond_the_allowlist_bit() {
        let declared = (1u64 << 14) | (1u64 << 55) | (1u64 << 56);
        let tasks = [
            ViSyscall::GetProcs,
            ViSyscall::GetProcs2,
            ViSyscall::TelemetrySnapshot,
        ];
        for allowlist in [declared, u64::MAX] {
            with_scheduler_task_caps(allowlist, CapSet::EMPTY, |tid| {
                for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
                    assert_eq!(
                        check_allowlist(sc as usize, tid),
                        Err(SyscallError::PermissionDenied),
                        "{sc:?} with allowlist {allowlist:#x}"
                    );
                }
                // Unrelated opcodes are not gated by the rights.
                assert_eq!(check_allowlist(ViSyscall::Yield as usize, tid), Ok(()));
            });
            // X alone admits exactly the task-observation pair.
            let x_only = CapSet {
                observe_tasks: true,
                ..CapSet::EMPTY
            };
            with_scheduler_task_caps(allowlist, x_only, |tid| {
                for sc in tasks {
                    assert_eq!(check_allowlist(sc as usize, tid), Ok(()), "{sc:?}");
                }
                assert_eq!(
                    check_allowlist(ViSyscall::MemInfo as usize, tid),
                    Err(SyscallError::PermissionDenied)
                );
            });
            // M alone admits exactly MemInfo.
            let m_only = CapSet {
                observe_memory: true,
                ..CapSet::EMPTY
            };
            with_scheduler_task_caps(allowlist, m_only, |tid| {
                for sc in tasks {
                    assert_eq!(
                        check_allowlist(sc as usize, tid),
                        Err(SyscallError::PermissionDenied),
                        "{sc:?}"
                    );
                }
                assert_eq!(check_allowlist(ViSyscall::MemInfo as usize, tid), Ok(()));
            });
        }
        // The right never stands in for an undeclared bit.
        with_scheduler_task(0, |tid| {
            for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
                assert_eq!(
                    check_allowlist(sc as usize, tid),
                    Err(SyscallError::PermissionDenied),
                    "{sc:?}"
                );
            }
        });
        // Kernel context (tid 0) has no task and is not gated.
        for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
            let _guard = crate::TEST_STATE_LOCK.lock();
            assert_eq!(check_allowlist(sc as usize, 0), Ok(()), "{sc:?}");
        }
    }

    /// Annex S2-T/S2-W §6: the attested `CALLER_FLAG_VFS_MUTATE` needs bit 63
    /// in an explicit allowlist AND the `vfs_mutate` right.
    #[test]
    fn vfs_mutate_attestation_needs_the_right_and_the_declaration() {
        use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
        let declared = (1u64 << 62) | (1u64 << 63);
        let w_only = CapSet {
            vfs_mutate: true,
            ..CapSet::EMPTY
        };
        for (allowlist, caps, want) in [
            (
                declared,
                CapSet::LEGACY_OBSERVE_MUTATE,
                CALLER_FLAG_VFS_MUTATE,
            ),
            (declared, w_only, CALLER_FLAG_VFS_MUTATE),
            (
                declared,
                CapSet::LEGACY_OBSERVE_MUTATE.without_observe_mutate(),
                0,
            ),
            (
                declared,
                CapSet {
                    vfs_mutate: false,
                    ..CapSet::LEGACY_OBSERVE_MUTATE
                },
                0,
            ),
            (1u64 << 62, CapSet::LEGACY_OBSERVE_MUTATE, 0),
            (u64::MAX, CapSet::LEGACY_OBSERVE_MUTATE, 0),
        ] {
            with_scheduler_task_caps(allowlist, caps, |tid| {
                {
                    let mut guard = SCHEDULER.lock();
                    let sched = guard.as_mut().expect("test scheduler installed");
                    let task = sched.tasks.get_mut(&tid).expect("test task");
                    task.cell_generation = 1;
                    task.root_tid = tid;
                    assert!(
                        sched.publish_live_cell_owner(api::cell_owner::CellOwner::new(
                            7, 1, tid as u64
                        ))
                    );
                }
                let identity = super::attested_identity_of(tid).expect("live task has an identity");
                assert_eq!(
                    identity.flags & CALLER_FLAG_VFS_MUTATE,
                    want,
                    "allowlist {allowlist:#x}, caps {caps:?}"
                );
            });
        }
    }

    /// Phase 01 CP1 (Q): `CALLER_FLAG_CONFIG_WRITE` is attested exactly when the
    /// Cell's root holds `config_write`, independent of the allowlist, and it
    /// never appears for a Cell without the right.
    #[test]
    fn config_write_attestation_follows_the_cell_right_only() {
        use api::caller_identity::{CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_NET_RIGHTS_MASK};
        let q_only = CapSet {
            config_write: true,
            ..CapSet::EMPTY
        };
        let shell = CapSet {
            config_write: true,
            ..CapSet::LEGACY_OBSERVE_MUTATE
        };
        for (allowlist, caps, want) in [
            (1u64 << 62, q_only, CALLER_FLAG_CONFIG_WRITE),
            (u64::MAX, shell, CALLER_FLAG_CONFIG_WRITE),
            (1u64 << 62, CapSet::LEGACY_OBSERVE_MUTATE, 0),
            (
                u64::MAX,
                CapSet {
                    net_resolve: true,
                    net_connect: true,
                    net_listen: true,
                    net_l2: true,
                    ..CapSet::LEGACY_OBSERVE_MUTATE
                },
                0,
            ),
        ] {
            with_scheduler_task_caps(allowlist, caps, |tid| {
                {
                    let mut guard = SCHEDULER.lock();
                    let sched = guard.as_mut().expect("test scheduler installed");
                    let task = sched.tasks.get_mut(&tid).expect("test task");
                    task.cell_generation = 1;
                    task.root_tid = tid;
                    assert!(
                        sched.publish_live_cell_owner(api::cell_owner::CellOwner::new(
                            7, 1, tid as u64
                        ))
                    );
                }
                let identity = super::attested_identity_of(tid).expect("live task has an identity");
                assert_eq!(
                    identity.flags & CALLER_FLAG_CONFIG_WRITE,
                    want,
                    "caps {caps:?}"
                );
                // Q never leaks into or out of the network flags.
                assert_eq!(
                    identity.flags & CALLER_FLAG_NET_RIGHTS_MASK,
                    caps.net_caller_flags(),
                    "caps {caps:?}"
                );
            });
        }
    }

    /// KSSH K3: `CALLER_FLAG_KMS_SSH_SIGN` / `CALLER_FLAG_KMS_SSH_ADMIN` are
    /// attested exactly when the Cell's root holds the matching right,
    /// independent of the allowlist, never for a Cell without it, and without
    /// leaking into (or out of) the NET, CONFIG or VFS bits. A thread reports the
    /// right of its Cell root.
    #[test]
    fn kms_ssh_attestation_follows_the_cell_right_only() {
        use api::caller_identity::{
            CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN,
            CALLER_FLAG_NET_RIGHTS_MASK, CALLER_FLAG_VFS_MUTATE,
        };
        let sign_only = CapSet {
            kms_ssh_sign: true,
            ..CapSet::EMPTY
        };
        let admin_only = CapSet {
            kms_ssh_admin: true,
            ..CapSet::EMPTY
        };
        let ssh_cell = CapSet {
            kms_ssh_sign: true,
            net_resolve: true,
            net_connect: true,
            ..CapSet::LEGACY_OBSERVE_MUTATE
        };
        let shell = CapSet {
            config_write: true,
            ..CapSet::LEGACY_OBSERVE_MUTATE
        };
        let declared = (1u64 << 62) | (1u64 << 63);
        for (allowlist, caps, want) in [
            (1u64 << 62, sign_only, CALLER_FLAG_KMS_SSH_SIGN),
            (u64::MAX, sign_only, CALLER_FLAG_KMS_SSH_SIGN),
            (declared, ssh_cell, CALLER_FLAG_KMS_SSH_SIGN),
            (1u64 << 62, admin_only, CALLER_FLAG_KMS_SSH_ADMIN),
            (u64::MAX, admin_only, CALLER_FLAG_KMS_SSH_ADMIN),
            (1u64 << 62, CapSet::EMPTY, 0),
            (u64::MAX, CapSet::LEGACY_OBSERVE_MUTATE, 0),
            (u64::MAX, shell, 0),
            (
                u64::MAX,
                CapSet {
                    net_resolve: true,
                    net_connect: true,
                    net_listen: true,
                    net_l2: true,
                    config_write: true,
                    ..CapSet::LEGACY_OBSERVE_MUTATE
                },
                0,
            ),
        ] {
            with_scheduler_task_caps(allowlist, caps, |tid| {
                {
                    let mut guard = SCHEDULER.lock();
                    let sched = guard.as_mut().expect("test scheduler installed");
                    let task = sched.tasks.get_mut(&tid).expect("test task");
                    task.cell_generation = 1;
                    task.root_tid = tid;
                    assert!(
                        sched.publish_live_cell_owner(api::cell_owner::CellOwner::new(
                            7, 1, tid as u64
                        ))
                    );
                    // A thread of the same Cell generation holds no CapSet of its
                    // own; it must report whatever its Cell root holds.
                    let mut thread = alloc::boxed::Box::new(Task::new(
                        tid + 1,
                        CellId(7),
                        "kms-attestation-thread",
                        alloc::vec::Vec::new(),
                    ));
                    thread.cell_generation = 1;
                    thread.root_tid = tid;
                    sched.tasks.insert(tid + 1, thread);
                }
                let kms = CALLER_FLAG_KMS_SSH_SIGN | CALLER_FLAG_KMS_SSH_ADMIN;
                let thread_identity =
                    super::attested_identity_of(tid + 1).expect("live thread has an identity");
                assert_eq!(thread_identity.flags & kms, want, "thread of caps {caps:?}");
                let identity = super::attested_identity_of(tid).expect("live task has an identity");
                assert_eq!(identity.flags & kms, want, "caps {caps:?}");
                assert_eq!(
                    identity.flags & kms,
                    caps.kms_caller_flags(),
                    "caps {caps:?}"
                );
                // The KMS bits never leak into or out of the other attested flags.
                assert_eq!(
                    identity.flags & CALLER_FLAG_NET_RIGHTS_MASK,
                    caps.net_caller_flags(),
                    "caps {caps:?}"
                );
                assert_eq!(
                    identity.flags & CALLER_FLAG_CONFIG_WRITE,
                    caps.config_caller_flags(),
                    "caps {caps:?}"
                );
                assert_eq!(
                    identity.flags & CALLER_FLAG_VFS_MUTATE,
                    super::vfs_mutate_caller_flag(allowlist, caps.vfs_mutate),
                    "caps {caps:?}"
                );
            });
        }
    }

    /// Architecture review H-1: a revoke aimed at one thread removes the right
    /// from the whole Cell incarnation — the root and every sibling — so no
    /// thread keeps authority the Cell lost.
    #[test]
    fn cap_revoke_on_a_worker_removes_the_right_from_every_thread_of_the_cell() {
        use api::syscall::cap_mask as CM;
        const CONTROLLER: usize = 201;
        const ROOT: usize = 202;
        const WORKER: usize = 203;
        const SIBLING: usize = 204;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut previous = SCHEDULER.lock();
        let saved = previous.take();
        let mut scheduler = Scheduler::new();
        let mut controller = Task::new(CONTROLLER, CellId(20), "init", alloc::vec::Vec::new());
        super::super::cap::CapSet {
            spawn: true,
            ..super::super::cap::CapSet::EMPTY
        }
        .apply_to(&mut controller);
        scheduler
            .tasks
            .insert(CONTROLLER, alloc::boxed::Box::new(controller));
        let caps = super::super::cap::CapSet {
            spawn: true,
            supervisor: true,
            ..super::super::cap::CapSet::EMPTY
        };
        for tid in [ROOT, WORKER, SIBLING] {
            let mut task = Task::new(tid, CellId(21), "cell", alloc::vec::Vec::new());
            task.root_tid = ROOT;
            task.cell_generation = 0; // one incarnation: (cell, 0)
            caps.apply_to(&mut task);
            scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
        }
        *previous = Some(scheduler);
        drop(previous);

        assert!([ROOT, WORKER, SIBLING]
            .iter()
            .all(|&tid| super::caller_has_spawn(tid)));
        // The CapRevoke arm minus its teardown (none for these bits) and the
        // victim notification, which needs a live mailbox.
        let mask = CM::SPAWN | CM::SUPERVISOR;
        {
            let mut guard = SCHEDULER.lock();
            let sched = guard.as_mut().expect("scheduler");
            assert_eq!(
                super::cap_revoke_gate(sched, CONTROLLER, WORKER, mask),
                Ok(())
            );
        }
        assert_eq!(
            super::commit_cap_revoke(CONTROLLER, WORKER, ROOT, (CellId(21), 0), mask),
            Ok(true)
        );
        for tid in [ROOT, WORKER, SIBLING] {
            assert!(!super::caller_has_spawn(tid), "tid {tid} kept SpawnCap");
            assert!(
                !super::caller_has_supervisor(tid),
                "tid {tid} kept SupervisorCap"
            );
        }

        let mut restore = SCHEDULER.lock();
        *restore = saved;
    }

    /// Review of Phase 07: the record is narrowed even when the targeted thread
    /// exits during the teardown, and while the teardown runs the pending mask
    /// already withholds the right from every task of the Cell.
    #[test]
    fn cap_revoke_pending_mask_holds_and_survives_target_exit() {
        use api::syscall::cap_mask as CM;
        const CONTROLLER: usize = 211;
        const ROOT: usize = 212;
        const WORKER: usize = 213;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut previous = SCHEDULER.lock();
        let saved = previous.take();
        let mut scheduler = Scheduler::new();
        let mut controller = Task::new(CONTROLLER, CellId(22), "init", alloc::vec::Vec::new());
        super::super::cap::CapSet {
            spawn: true,
            ..super::super::cap::CapSet::EMPTY
        }
        .apply_to(&mut controller);
        scheduler
            .tasks
            .insert(CONTROLLER, alloc::boxed::Box::new(controller));
        for tid in [ROOT, WORKER] {
            let mut task = Task::new(tid, CellId(23), "cell", alloc::vec::Vec::new());
            task.root_tid = ROOT;
            task.cell_generation = 0; // one incarnation: (cell, 0)
            super::super::cap::CapSet {
                pcie_driver: true,
                supervisor: true,
                ..super::super::cap::CapSet::EMPTY
            }
            .apply_to(&mut task);
            scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
        }
        *previous = Some(scheduler);
        drop(previous);

        let mask = CM::PCIE_DRIVER;
        let pending = super::PendingRevoke {
            root_tid: ROOT,
            incarnation: (CellId(23), 0),
            mask,
        };
        SCHEDULER
            .lock()
            .as_mut()
            .and_then(|s| s.tasks.get_mut(&ROOT))
            .expect("root")
            .revoke_pending |= mask;
        // During the teardown: withheld from both tasks, the rest kept.
        assert!(!super::caller_has_pcie_driver(WORKER) && !super::caller_has_pcie_driver(ROOT));
        assert!(super::caller_has_supervisor(ROOT));
        // The targeted worker exits before the commit.
        SCHEDULER
            .lock()
            .as_mut()
            .expect("sched")
            .tasks
            .remove(&WORKER);
        assert_eq!(
            super::commit_cap_revoke(CONTROLLER, WORKER, ROOT, (CellId(23), 0), mask),
            Ok(false)
        );
        drop(pending);
        assert!(
            !super::caller_has_pcie_driver(ROOT),
            "root kept the revoked right"
        );
        assert!(super::caller_has_supervisor(ROOT));

        // A failed teardown withdraws only the pending bits.
        let failed = super::PendingRevoke {
            root_tid: ROOT,
            incarnation: (CellId(23), 0),
            mask: CM::SUPERVISOR,
        };
        SCHEDULER
            .lock()
            .as_mut()
            .and_then(|s| s.tasks.get_mut(&ROOT))
            .expect("root")
            .revoke_pending |= CM::SUPERVISOR;
        assert!(!super::caller_has_supervisor(ROOT));
        drop(failed);
        assert!(
            super::caller_has_supervisor(ROOT),
            "failed revoke left the right withheld"
        );

        let mut restore = SCHEDULER.lock();
        *restore = saved;
    }

    /// Raw opcode 502 (power-off) is accepted only from a Cell whose root image
    /// is signed with the program name `shutdown`. A task merely *named*
    /// `shutdown` (the basename of a spawner-chosen path, e.g. `/home/x/shutdown`)
    /// is denied: this is the architecture review C-1 regression check.
    #[test]
    fn raw_shutdown_is_accepted_only_for_the_signed_shutdown_image() {
        use api::cell_image::{ImageIdentity, ANCHOR_DEV};
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut previous = SCHEDULER.lock();
        let saved = previous.take();
        let mut scheduler = Scheduler::new();
        // tids ≥ 100: other tests latch tid 1 as trusted init for the process.
        // (tid, task name, signed image name, kernel-resolved install path)
        for (tid, name, image, named) in [
            (101usize, "shutdown", Some("shutdown"), true),
            (102, "shutdown", None, true), // unsigned bytes at …/shutdown
            (103, "shutdown", Some("cellbox"), true), // another signed program
            (104, "cellbox", Some("cellbox"), true),
            (105, "shutdown", Some("shutdown"), false), // caller-supplied ELF / sandbox / alias
            (106, "supervisor", Some("supervisor"), true),
        ] {
            let mut task = Task::new(tid, CellId(7), name, alloc::vec::Vec::new());
            match image {
                Some(program) => {
                    let signed =
                        ImageIdentity::signed([tid as u8; 32], ANCHOR_DEV, program.as_bytes())
                            .expect("valid image");
                    assert!(task.bind_image(signed));
                }
                None => assert!(task.bind_image(ImageIdentity::unsigned([tid as u8; 32]))),
            }
            task.launch_named = named;
            scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
        }
        *previous = Some(scheduler);
        drop(previous);

        assert_eq!(super::authorize_raw_shutdown(101), Ok(()));
        for tid in [102usize, 103, 104, 105, 106, 199] {
            assert_eq!(
                super::authorize_raw_shutdown(tid),
                Err(SyscallError::PermissionDenied),
                "tid {tid}"
            );
        }
        // The dispatcher refuses the denied callers itself, before any platform code.
        for tid in [102usize, 103, 104, 105, 106] {
            assert_eq!(
                super::handle_syscall(tid, Syscall::Shutdown),
                Err(SyscallError::PermissionDenied)
            );
        }
        // The same identity keys the launch edges: a task's own name never
        // counts, and a signed image counts only from a kernel-resolved path.
        for (tid, expected) in [(102usize, ""), (103, "cellbox"), (105, "")] {
            let (identity, _, _) = super::caller_launch_state(tid).expect("live task");
            assert_eq!(identity, expected, "tid {tid}");
        }

        let mut restore = SCHEDULER.lock();
        *restore = saved;
    }

    /// A staged command line belongs to the caller: it survives every syscall
    /// that is not a launch, and a launch attempt always clears it — including a
    /// refusal that never reaches the launch boundary.
    ///
    /// Both halves are load-bearing. The shell stages an argv, reads the cell
    /// through VFS, and only then spawns; a syscall that clears the staging on
    /// the way loses the command line (`httpd 9091 /tmp/resp.txt` listened on
    /// :8080). And a refused attempt must not leave a command line behind for a
    /// later child.
    #[test]
    fn staged_argv_outlives_other_syscalls_and_a_launch_attempt_clears_it() {
        with_scheduler_task(0, |tid| {
            let non_spawn = Syscall::GetTime { op: 0 };
            let spawn = Syscall::SpawnFromPath {
                path_ptr: 0,
                path_len: 0,
            };

            crate::cell::state_stash::discard_spawn_argv(tid);
            assert_eq!(crate::cell::state_stash::stage_spawn_argv(tid, b"argv"), 4);
            assert!(!consumes_staged_spawn_argv(&non_spawn));
            assert!(staged_spawn_argv_cleanup(&non_spawn, tid).is_none());
            assert_eq!(
                crate::cell::state_stash::take_spawn_argv(tid),
                Some(b"argv".to_vec())
            );

            assert_eq!(crate::cell::state_stash::stage_spawn_argv(tid, b"argv"), 4);
            assert!(consumes_staged_spawn_argv(&spawn));
            drop(staged_spawn_argv_cleanup(&spawn, tid));
            assert_eq!(crate::cell::state_stash::take_spawn_argv(tid), None);
        });
    }

    /// Staging is task-local, so it reaches no live record but its own: a task
    /// id with no task behind it stores nothing.
    #[test]
    fn staging_an_argv_without_a_live_task_stores_nothing() {
        const ABSENT_TID: usize = 0xDEAD_0BEE;
        crate::cell::state_stash::discard_spawn_argv(ABSENT_TID);
        assert_eq!(
            crate::cell::state_stash::stage_spawn_argv(ABSENT_TID, b"argv"),
            0
        );
        assert_eq!(crate::cell::state_stash::take_spawn_argv(ABSENT_TID), None);
    }

    #[test]
    fn telemetry_snapshot_maps_all_four_args_and_validates_them() {
        use crate::task::telemetry::request_is_valid;
        use api::telemetry::{request, HEADER_LEN};
        let syscall = map_syscall(ViSyscall::TelemetrySnapshot as usize, 0x3000, 4096, 3, 0)
            .expect("TelemetrySnapshot must decode");
        match syscall {
            Syscall::TelemetrySnapshot {
                out_ptr,
                out_len,
                flags,
                reserved,
            } => {
                assert_eq!((out_ptr, out_len, flags, reserved), (0x3000, 4096, 3, 0));
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::TelemetrySnapshot));
        // A transient allocation failure reaches the caller as the typed OOM code.
        assert!(supports_typed_oom(&syscall));
        assert!(request_is_valid(0x3000, HEADER_LEN, request::ALL, 0));
        assert!(
            !request_is_valid(0x3000, HEADER_LEN - 1, 0, 0),
            "short buffer"
        );
        assert!(!request_is_valid(0x3004, 4096, 0, 0), "misaligned buffer");
        assert!(!request_is_valid(0x3000, 4096, 1 << 2, 0), "unknown flag");
        assert!(!request_is_valid(0x3000, 4096, 0, 1), "nonzero a3");
        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::TelemetrySnapshot as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
    }

    #[test]
    fn wait_completion_refuses_mask_bits_above_31_instead_of_dropping_them() {
        use api::completion::source::{IRQ, TIMER};
        let decode =
            |a0: usize| match map_syscall(ViSyscall::WaitCompletion as usize, a0, 5, 0, 0x6000) {
                Some(Syscall::WaitCompletion { mask, out_ptr, .. }) => (mask, out_ptr),
                other => panic!("decoded wrong syscall variant: {other:?}"),
            };
        let (mask, out_ptr) = decode((IRQ | TIMER) as usize);
        assert_eq!((mask, out_ptr), (IRQ | TIMER, 0x6000));
        assert!(super::super::completion_wait::source_is_valid(
            mask,
            Some(1)
        ));
        #[cfg(target_pointer_width = "64")]
        {
            let (high, _) = decode((1usize << 32) | TIMER as usize);
            assert!(!super::super::completion_wait::source_is_valid(
                high,
                Some(1)
            ));
        }
    }

    #[test]
    fn get_procs2_maps_args_and_dispatch_variant() {
        let syscall = map_syscall(ViSyscall::GetProcs2 as usize, 0x1000, 7, 0, 0)
            .expect("GetProcs2 must decode");
        match syscall {
            Syscall::GetProcs2 { buf_ptr, buf_len } => {
                assert_eq!(buf_ptr, 0x1000);
                assert_eq!(buf_len, 7);
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::GetProcs2));
    }

    #[test]
    fn get_procs2_allowlist_denies_without_bit_55() {
        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::GetProcs2 as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
    }

    #[test]
    fn get_procs2_allowlist_allows_with_bit_55() {
        with_scheduler_task(1u64 << 55, |tid| {
            assert_eq!(check_allowlist(ViSyscall::GetProcs2 as usize, tid), Ok(()));
        });
    }

    #[test]
    fn syscall_result_encoding_preserves_generic_and_additive_oom_codes() {
        assert_eq!(encode_syscall_result(Ok(9), usize::MAX, false), 9);
        assert_eq!(
            encode_syscall_result(Err(SyscallError::InvalidInput), usize::MAX, true),
            usize::MAX
        );
        assert_eq!(
            encode_syscall_result(Err(SyscallError::OutOfMemory), usize::MAX, true),
            usize::MAX - 1
        );
        assert_eq!(
            encode_syscall_result(Err(SyscallError::OutOfMemory), u32::MAX as usize, true),
            u32::MAX as usize - 1
        );
        assert_eq!(
            encode_syscall_result(Err(SyscallError::OutOfMemory), usize::MAX, false),
            usize::MAX
        );
        assert!(supports_typed_oom(&Syscall::SpawnFromPath {
            path_ptr: 0,
            path_len: 0,
        }));
        assert!(!supports_typed_oom(&Syscall::MemInfo {
            out_ptr: 0,
            out_len: 0,
        }));
    }

    /// Phase 09B K1: `QueryRetirement` (431) decodes a0..a2, is admitted past
    /// an empty allowlist, and its refusals are typed: bad length, no record,
    /// not a watcher. A watcher passes the gate and reaches the user copy.
    #[test]
    fn query_retirement_is_gated_by_the_verdict_with_typed_errors() {
        use crate::cell::retirement_verdict;
        let syscall = map_syscall(ViSyscall::QueryRetirement as usize, 77, 0x5000, 96, 0)
            .expect("QueryRetirement must decode");
        match syscall {
            Syscall::QueryRetirement {
                root_tid,
                out_ptr,
                out_len,
            } => {
                assert_eq!((root_tid, out_ptr, out_len), (77, 0x5000, 96));
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), None);
        assert!(super::has_typed_errors(&syscall));
        let len = api::retirement::RETIREMENT_RECORD_LEN;
        let query = |tid, root_tid, out_ptr, out_len| {
            super::handle_syscall(
                tid,
                Syscall::QueryRetirement {
                    root_tid,
                    out_ptr,
                    out_len,
                },
            )
        };
        with_scheduler_task_caps(0, CapSet::EMPTY, |tid| {
            retirement_verdict::tests::reset();
            assert_eq!(
                check_allowlist(ViSyscall::QueryRetirement as usize, tid),
                Ok(())
            );
            let watched = api::cell_owner::CellOwner::new(0x431, 3, 77);
            let foreign = api::cell_owner::CellOwner::new(0x432, 3, 78);
            assert!(retirement_verdict::open(watched, &[tid]));
            assert!(retirement_verdict::open(foreign, &[tid + 1]));

            let bad_len = query(tid, 77, 0x5000, len - 1);
            assert_eq!(bad_len, Err(SyscallError::InvalidInput));
            assert_eq!(super::encode_ipc_result(bad_len), usize::MAX - 3);
            let missing = query(tid, 79, 0x5000, len);
            assert_eq!(missing, Err(SyscallError::FileNotFound));
            assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
            let denied = query(tid, 78, 0x5000, len);
            assert_eq!(denied, Err(SyscallError::PermissionDenied));
            assert_eq!(super::encode_ipc_result(denied), usize::MAX);
            assert_eq!(
                query(tid, 77, 0, len),
                Err(SyscallError::InvalidInput),
                "gate passed"
            );
            retirement_verdict::tests::reset();
        });
    }

    /// Inserts a live task into the scheduler installed by `with_scheduler_task_caps`.
    fn insert_block_route_task(tid: usize, cell: u64, generation: u64, caps: CapSet) {
        let mut task = alloc::boxed::Box::new(Task::new(
            tid,
            CellId(cell),
            "block-route-test",
            alloc::vec::Vec::new(),
        ));
        task.cell_generation = generation;
        caps.apply_to(&mut task);
        SCHEDULER
            .lock()
            .as_mut()
            .expect("scheduler")
            .tasks
            .insert(tid, task);
    }

    fn block_routes_snapshot(
    ) -> [Option<api::block_route::ViBlockRouteV1>; api::block_route::MAX_BLOCK_ROUTES] {
        core::array::from_fn(crate::task::drivers::driver_cell::block_route_record)
    }

    /// Phase 09C D4: `RegisterBlockDriver` (416) decodes a0..a3 with typed
    /// errors, and binds only the route of the device the caller owns.
    #[test]
    fn register_block_driver_binds_only_the_callers_owned_device_route() {
        use crate::resource_registry::{claim_bdf_owner, release_bdfs_for};
        use crate::task::drivers::driver_cell::{deregister_all_for, reset_block_routes};
        use api::block_route::MAX_BLOCK_ROUTES;
        const BDF: usize = 0x4160;
        const FOREIGN_BDF: usize = 0x4161;
        const BASE: usize = 0x4_1600_0000;
        let syscall = map_syscall(ViSyscall::RegisterBlockDriver as usize, 1, 0xFA, 2, 3)
            .expect("RegisterBlockDriver must decode");
        match syscall {
            Syscall::RegisterBlockDriver { kind, id, reserved } => {
                assert_eq!((kind, id, reserved), (1, 0xFA, (2, 3)));
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert!(super::has_typed_errors(&syscall));

        let register = |tid, kind, id, reserved| {
            super::handle_syscall(tid, Syscall::RegisterBlockDriver { kind, id, reserved })
        };
        let driver = CapSet {
            pcie_driver: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(u64::MAX, driver, |tid| {
            reset_block_routes();
            SCHEDULER
                .lock()
                .as_mut()
                .expect("scheduler")
                .tasks
                .get_mut(&tid)
                .expect("caller")
                .cell_generation = 3;
            insert_block_route_task(2, 8, 5, driver);
            insert_block_route_task(3, 9, 1, CapSet::EMPTY);
            insert_block_route_task(
                4,
                10,
                1,
                CapSet {
                    usb_driver: true,
                    ..CapSet::EMPTY
                },
            );
            insert_block_route_task(6, 11, 1, driver);
            SCHEDULER
                .lock()
                .as_mut()
                .expect("scheduler")
                .tasks
                .get_mut(&6)
                .expect("retiring task")
                .state = crate::task::tcb::TaskState::Retiring;
            insert_block_route_task(
                5,
                10,
                1,
                CapSet {
                    usb_driver: true,
                    ..CapSet::EMPTY
                },
            );
            assert!(claim_bdf_owner(BDF as u32, tid));
            assert!(claim_bdf_owner(FOREIGN_BDF as u32, 2));
            // `RequestMmio` records the claiming task id as the window owner.
            assert!(crate::resource_registry::request_mmio_unchecked(4, BASE, 0x1000).is_ok());
            let empty = block_routes_snapshot();

            for (kind, id, reserved) in [
                (0, BDF, (0, 0)),
                (3, BDF, (0, 0)),
                (0x1_0001, BDF, (0, 0)),
                (1, 0x1_0000, (0, 0)),
                (2, 0, (0, 0)),
                (1, BDF, (1, 0)),
                (1, BDF, (0, 1)),
            ] {
                let malformed = register(tid, kind, id, reserved);
                assert_eq!(malformed, Err(SyscallError::InvalidInput), "{kind} {id:#x}");
                assert_eq!(super::encode_ipc_result(malformed), usize::MAX - 3);
            }
            for (caller, kind, id) in [(tid, 1, FOREIGN_BDF), (tid, 1, 0x4162), (tid, 2, BASE)] {
                let not_owner = register(caller, kind, id, (0, 0));
                assert_eq!(not_owner, Err(SyscallError::FileNotFound), "{kind} {id:#x}");
                assert_eq!(super::encode_ipc_result(not_owner), usize::MAX - 2);
            }
            assert!(claim_bdf_owner(0x4163, 3));
            assert!(claim_bdf_owner(0x4164, 6));
            for (caller, bdf) in [(3, 0x4163), (6, 0x4164)] {
                let denied = register(caller, 1, bdf, (0, 0));
                assert_eq!(
                    denied,
                    Err(SyscallError::PermissionDenied),
                    "no cap / retiring"
                );
                assert_eq!(super::encode_ipc_result(denied), usize::MAX);
            }
            assert_eq!(
                block_routes_snapshot(),
                empty,
                "refusals leave the table untouched"
            );

            assert_eq!(register(tid, 1, BDF, (0, 0)), Ok(0), "first bind appends");
            assert_eq!(register(tid, 1, BDF, (0, 0)), Ok(0), "repeat is idempotent");
            assert_eq!(
                register(4, 2, BASE, (0, 0)),
                Ok(0),
                "second device, second slot"
            );
            let routes = block_routes_snapshot();
            let first = routes[0].expect("slot 0");
            assert_eq!(
                (first.kind, first.id, first.provider()),
                (1, BDF as u64, Some((1, 7, 3)))
            );
            let second = routes[1].expect("slot 1");
            assert_eq!(
                (second.kind, second.id, second.provider()),
                (2, BASE as u64, Some((4, 10, 1)))
            );
            assert!(routes[2].is_none());

            assert_eq!(
                register(5, 2, BASE, (0, 0)),
                Err(SyscallError::FileNotFound),
                "another task of the same Cell does not own the window task 4 claimed"
            );
            assert_eq!(block_routes_snapshot(), routes);

            deregister_all_for(tid);
            let released = block_routes_snapshot()[0].expect("key kept");
            assert_eq!(
                (released.kind, released.id, released.provider()),
                (1, BDF as u64, None)
            );
            release_bdfs_for(tid);
            assert!(claim_bdf_owner(BDF as u32, 2));
            assert_eq!(
                register(2, 1, BDF, (0, 0)),
                Ok(0),
                "new owner regains the same slot"
            );
            assert_eq!(
                block_routes_snapshot()[0].and_then(|r| r.provider()),
                Some((2, 8, 5))
            );
            assert_eq!(
                block_routes_snapshot()[1],
                routes[1],
                "sibling route untouched"
            );

            for bdf in 0x4170..0x4170 + (MAX_BLOCK_ROUTES - 2) {
                assert!(claim_bdf_owner(bdf as u32, 2));
                assert_eq!(register(2, 1, bdf, (0, 0)), Ok(0));
            }
            assert!(claim_bdf_owner(0x4180, 2));
            let full = block_routes_snapshot();
            let refused = register(2, 1, 0x4180, (0, 0));
            assert_eq!(refused, Err(SyscallError::PermissionDenied), "table full");
            assert_eq!(super::encode_ipc_result(refused), usize::MAX);
            assert_eq!(block_routes_snapshot(), full);

            assert_eq!(
                crate::cell::service_registry::lookup(api::syscall::service::BLOCK_DRIVER),
                None
            );
            assert_eq!(
                super::handle_syscall(
                    tid,
                    Syscall::LookupService {
                        service_id: api::syscall::service::BLOCK_DRIVER
                    }
                ),
                Ok(0)
            );

            for owner in [1, 2, 3, 4, 5, 6] {
                release_bdfs_for(owner);
                deregister_all_for(owner);
            }
            crate::resource_registry::release_mmio_for(4);
            reset_block_routes();
        });
    }

    /// Phase 09C D4: `LookupBlockRoute` (432) is admitted past the allowlist,
    /// gated on BlockIoCap or SpawnCap, and its refusals are typed.
    #[test]
    fn lookup_block_route_is_gated_on_block_io_or_spawn_with_typed_errors() {
        use crate::task::drivers::driver_cell::{
            block_route_record, deregister_all_for, publish_block_route, reset_block_routes,
        };
        use api::block_route::{
            DeviceKey, ViBlockRouteV1, BLOCK_DEVICE_PCI, BLOCK_ROUTE_RECORD_LEN,
        };
        let syscall = map_syscall(ViSyscall::LookupBlockRoute as usize, 3, 0x5000, 40, 0)
            .expect("LookupBlockRoute must decode");
        match syscall {
            Syscall::LookupBlockRoute {
                index,
                out_ptr,
                out_len,
                reserved,
            } => {
                assert_eq!((index, out_ptr, out_len, reserved), (3, 0x5000, 40, 0));
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), None);
        assert!(super::has_typed_errors(&syscall));
        let len = BLOCK_ROUTE_RECORD_LEN;
        let lookup = |tid, index, out_ptr, out_len, reserved| {
            super::handle_syscall(
                tid,
                Syscall::LookupBlockRoute {
                    index,
                    out_ptr,
                    out_len,
                    reserved,
                },
            )
        };
        let key = DeviceKey::new(BLOCK_DEVICE_PCI, 0xFA).expect("valid BDF");

        let block_io = CapSet {
            block_io: true,
            ..CapSet::EMPTY
        };
        let spawn = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        for caps in [CapSet::EMPTY, block_io, spawn] {
            with_scheduler_task_caps(0, caps, |tid| {
                reset_block_routes();
                assert_eq!(publish_block_route(key, (9, 90, 2)), Some(0));
                assert_eq!(
                    check_allowlist(ViSyscall::LookupBlockRoute as usize, tid),
                    Ok(())
                );
                if caps == CapSet::EMPTY {
                    let denied = lookup(tid, 0, 0x5000, len, 0);
                    assert_eq!(denied, Err(SyscallError::PermissionDenied));
                    assert_eq!(super::encode_ipc_result(denied), usize::MAX);
                    reset_block_routes();
                    return;
                }
                for (out_len, reserved) in [(len - 1, 0), (len + 1, 0), (len, 1)] {
                    let bad = lookup(tid, 0, 0x5000, out_len, reserved);
                    assert_eq!(bad, Err(SyscallError::InvalidInput));
                    assert_eq!(super::encode_ipc_result(bad), usize::MAX - 3);
                }
                for index in [1, 8, usize::MAX] {
                    let missing = lookup(tid, index, 0x5000, len, 0);
                    assert_eq!(missing, Err(SyscallError::FileNotFound), "index {index}");
                    assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
                }
                assert_eq!(
                    lookup(tid, 0, 0, len, 0),
                    Err(SyscallError::InvalidInput),
                    "gate passed"
                );

                let record = block_route_record(0).expect("bound slot");
                let decoded = ViBlockRouteV1::decode(&record.encode()).expect("valid record bytes");
                assert_eq!(decoded.device(), key);
                assert_eq!(decoded.provider(), Some((9, 90, 2)));
                deregister_all_for(9);
                let released = block_route_record(0).expect("key kept");
                let decoded =
                    ViBlockRouteV1::decode(&released.encode()).expect("valid record bytes");
                assert_eq!(
                    (decoded.device(), decoded.provider(), decoded.flags),
                    (key, None, 0)
                );
                reset_block_routes();
            });
        }
    }

    /// Phase 09C R3: `BLOCK_DRIVER` is retired; not even a SpawnCap holder may
    /// register it.
    #[test]
    fn register_service_refuses_the_retired_block_driver_id() {
        let spawn = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(0, spawn, |tid| {
            let refused = super::handle_syscall(
                tid,
                Syscall::RegisterService {
                    service_id: api::syscall::service::BLOCK_DRIVER,
                    tid,
                },
            );
            assert_eq!(refused, Err(SyscallError::PermissionDenied));
            assert_eq!(
                crate::cell::service_registry::lookup(api::syscall::service::BLOCK_DRIVER),
                None
            );
        });
    }

    /// ADR-0029 §3: `QueryMachineFacts` (433) is admitted past the allowlist,
    /// gated on SpawnCap, validates its arguments before computing anything,
    /// and reports "no facts" while no x86 profile is bound.
    #[test]
    fn query_machine_facts_is_gated_on_spawn_with_typed_errors() {
        use api::machine_facts::MACHINE_FACTS_CAPACITY as CAP;
        let syscall = map_syscall(ViSyscall::QueryMachineFacts as usize, 0x5000, CAP, 0, 0)
            .expect("QueryMachineFacts must decode");
        match syscall {
            Syscall::QueryMachineFacts {
                out_ptr,
                out_len,
                a2,
                a3,
            } => {
                assert_eq!((out_ptr, out_len, a2, a3), (0x5000, CAP, 0, 0));
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), None);
        assert!(super::has_typed_errors(&syscall));
        let query = |tid, out_len, a2, a3| {
            super::handle_syscall(
                tid,
                Syscall::QueryMachineFacts {
                    out_ptr: 0x5000,
                    out_len,
                    a2,
                    a3,
                },
            )
        };
        let spawn = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        for caps in [CapSet::EMPTY, spawn] {
            with_scheduler_task_caps(0, caps, |tid| {
                assert_eq!(
                    check_allowlist(ViSyscall::QueryMachineFacts as usize, tid),
                    Ok(())
                );
                if caps == CapSet::EMPTY {
                    let denied = query(tid, CAP, 0, 0);
                    assert_eq!(denied, Err(SyscallError::PermissionDenied));
                    assert_eq!(super::encode_ipc_result(denied), usize::MAX);
                    return;
                }
                for (out_len, a2, a3) in [
                    (CAP - 1, 0, 0),
                    (CAP + 1, 0, 0),
                    (0, 0, 0),
                    (CAP, 1, 0),
                    (CAP, 0, 1),
                ] {
                    let bad = query(tid, out_len, a2, a3);
                    assert_eq!(bad, Err(SyscallError::InvalidInput), "{out_len} {a2} {a3}");
                    assert_eq!(super::encode_ipc_result(bad), usize::MAX - 3);
                }
                assert!(
                    crate::board::x86_machine_inputs().is_none(),
                    "host tests bind no profile"
                );
                let missing = query(tid, CAP, 0, 0);
                assert_eq!(missing, Err(SyscallError::FileNotFound));
                assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
            });
        }
    }

    /// ADR-0031 CP1 §6: with two functions of one vendor:device, the first
    /// driver claims A, a second driver gets B, a third gets nothing, and an
    /// owner's re-query still reaches its own function.
    #[test]
    fn vendor_lookup_skips_functions_owned_by_another_driver() {
        use crate::resource_registry::{claim_bdf_owner, release_bdfs_for};
        use crate::task::drivers::pcie_ecam::{self, Bar, PciDevice};
        const VENDOR: u16 = 0x1b36;
        const DEVICE: u16 = 0xfa04;
        let function = |fun| PciDevice {
            bdf: (0xee, 3, fun),
            vendor_id: VENDOR,
            device_id: DEVICE,
            class: 0xff,
            subclass: 0,
            prog_if: 0,
            bars: [Bar::None; 6],
            msix: None,
            pm: None,
        };
        pcie_ecam::test_insert_devices(&[function(0), function(1)]);
        let (first, second, third) = (0x9001, 0x9002, 0x9003);
        let find = |tid| {
            pcie_ecam::find_vendor_device(VENDOR, DEVICE, |bdf| super::pcie_usable_by(tid, bdf))
                .map(|d| d.canonical_bdf())
        };
        let a = find(first).expect("function A offered");
        assert!(claim_bdf_owner(a, first));
        let b = find(second).expect("function B offered to a second driver");
        assert_ne!(a, b);
        assert!(claim_bdf_owner(b, second));
        assert_eq!(find(third), None);
        assert_eq!(find(second), Some(b));
        release_bdfs_for(first);
        assert_eq!(
            find(third),
            Some(a),
            "a reaped owner's function is offered again"
        );
        release_bdfs_for(second);
        pcie_ecam::test_remove_devices(VENDOR);
    }

    /// 434/435 refuse a caller without PcieDriverCap, and their sentinels stay
    /// distinct so a driver can tell "keep polling" from "not yours".
    #[test]
    fn pcie_function_calls_are_gated_and_typed() {
        use api::pcie_function as pf;
        let query = Syscall::QueryPcieFunction {
            bdf: 0x18,
            out_ptr: 0x5000,
            out_len: pf::PCIE_FUNCTION_RECORD_LEN,
            reserved: 0,
        };
        let alloc = Syscall::AllocPcieIrq {
            bdf: 0x18,
            want: 1,
            mode: pf::IRQ_MODE_ALLOW_MSIX,
            reserved: 0,
        };
        assert!(super::is_pcie_function_call(&query) && super::is_pcie_function_call(&alloc));
        for call in [query, alloc] {
            let result = super::handle_syscall(0x9abc, call);
            assert!(matches!(result, Err(SyscallError::PermissionDenied)));
            assert_eq!(
                super::encode_pcie_function_result(result),
                pf::ERR_PERMISSION_DENIED
            );
        }
        let encode = |e| super::encode_pcie_function_result(Err(e));
        assert_eq!(encode(SyscallError::NotSupported), pf::ERR_NOT_SUPPORTED);
        assert_eq!(encode(SyscallError::OutOfMemory), pf::ERR_NO_VECTORS);
        assert_eq!(encode(SyscallError::FileNotFound), pf::ERR_NOT_FOUND);
        assert_eq!(encode(SyscallError::InvalidInput), pf::ERR_INVALID_INPUT);
    }

    /// 442 decodes with its reserved words, uses the typed sentinels, refuses a
    /// caller without PcieDriverCap, and on a host (no VirtIO MMIO) answers a
    /// PcieDriverCap holder `NotSupported` once the arguments are well formed.
    #[test]
    fn probe_virtio_mmio_is_gated_and_typed() {
        use api::syscall::virtio_probe as vp;
        let decoded = super::map_syscall(442, 0x1000_1000, 0, 0, 7);
        assert!(matches!(
            decoded,
            Some(Syscall::ProbeVirtioMmio {
                base: 0x1000_1000,
                reserved: (0, 0, 7)
            })
        ));
        assert_eq!(
            decoded.as_ref().and_then(super::syscall_to_vi),
            Some(api::syscall::ViSyscall::ProbeVirtioMmio)
        );
        let call = |reserved| Syscall::ProbeVirtioMmio {
            base: 0x1000_1000,
            reserved,
        };
        assert!(super::has_typed_errors(&call((0, 0, 0))));
        assert!(matches!(
            super::handle_syscall(0x9abc, call((0, 0, 0))),
            Err(SyscallError::PermissionDenied)
        ));
        let encode = |e| super::encode_ipc_result(Err(e));
        assert_eq!(encode(SyscallError::FileNotFound), vp::ERR_NOT_FOUND);
        assert_eq!(encode(SyscallError::InvalidInput), vp::ERR_INVALID_INPUT);
        let driver = CapSet {
            pcie_driver: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(u64::MAX, driver, |tid| {
            assert!(matches!(
                super::handle_syscall(tid, call((0, 1, 0))),
                Err(SyscallError::InvalidInput)
            ));
            assert!(matches!(
                super::handle_syscall(tid, call((0, 0, 0))),
                Err(SyscallError::NotSupported)
            ));
        });
    }

    /// CP1 §5 (D-4): 436 decodes to `SystemPower` with bit 49 and is refused
    /// with `PermissionDenied` to any caller without `SupervisorCap`, before
    /// the action is looked at, including holders of other authority.
    #[test]
    fn system_power_is_refused_without_supervisor_cap() {
        use api::syscall::system_power as sp;
        let decoded = super::map_syscall(436, sp::ACTION_POWEROFF, 0, 0, 0);
        assert!(matches!(
            decoded,
            Some(Syscall::SystemPower {
                action: sp::ACTION_POWEROFF,
                a1: 0,
                a2: 0,
                a3: 0
            })
        ));
        assert_eq!(
            decoded.as_ref().and_then(super::syscall_to_vi),
            Some(api::syscall::ViSyscall::SystemPower)
        );
        let call = |action| Syscall::SystemPower {
            action,
            a1: 0,
            a2: 0,
            a3: 0,
        };
        for action in [sp::ACTION_REBOOT, sp::ACTION_POWEROFF] {
            let result = super::handle_syscall(0x9abc, call(action));
            assert!(matches!(result, Err(SyscallError::PermissionDenied)));
            assert_eq!(
                super::encode_system_power_result(result),
                sp::ERR_PERMISSION_DENIED
            );
        }
        let other_authority = CapSet {
            spawn: true,
            pcie_driver: true,
            block_io: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(u64::MAX, other_authority, |tid| {
            for action in [sp::ACTION_REBOOT, sp::ACTION_POWEROFF] {
                assert!(matches!(
                    super::handle_syscall(tid, call(action)),
                    Err(SyscallError::PermissionDenied)
                ));
            }
        });
    }

    /// With `SupervisorCap`, an unknown action or a non-zero a1..a3 is
    /// `InvalidInput`, and a poweroff with no recorded ACPI facts (no
    /// `\_S5_`) is `NotSupported`; each keeps its own sentinel.
    #[test]
    fn system_power_checks_action_and_arguments_behind_the_gate() {
        use api::syscall::system_power as sp;
        let supervisor = CapSet {
            supervisor: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(u64::MAX, supervisor, |tid| {
            let run = |action, a1, a2, a3| {
                super::encode_system_power_result(super::handle_syscall(
                    tid,
                    Syscall::SystemPower { action, a1, a2, a3 },
                ))
            };
            for action in [0, 3, usize::MAX] {
                assert_eq!(run(action, 0, 0, 0), sp::ERR_INVALID_INPUT);
            }
            assert_eq!(run(sp::ACTION_POWEROFF, 1, 0, 0), sp::ERR_INVALID_INPUT);
            assert_eq!(run(sp::ACTION_REBOOT, 0, 0, 1), sp::ERR_INVALID_INPUT);
            assert_eq!(run(sp::ACTION_POWEROFF, 0, 0, 0), sp::ERR_NOT_SUPPORTED);
        });
    }

    /// ADR-0029 §3 (CP1 Q9): only trusted init, naming itself, may register
    /// `MACHINE_PIN`; a SpawnCap holder that is not init is refused.
    #[test]
    fn register_service_admits_machine_pin_only_from_trusted_init_itself() {
        use api::syscall::service::MACHINE_PIN;
        let spawn = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(0, spawn, |tid| {
            let register = |tid, target| {
                super::handle_syscall(
                    tid,
                    Syscall::RegisterService {
                        service_id: MACHINE_PIN,
                        tid: target,
                    },
                )
            };
            if super::TRUSTED_INIT_TID.load(core::sync::atomic::Ordering::Acquire) != tid {
                assert_eq!(
                    register(tid, tid),
                    Err(SyscallError::PermissionDenied),
                    "not init"
                );
                assert_eq!(crate::cell::service_registry::lookup(MACHINE_PIN), None);
            }
            super::publish_trusted_init(tid);
            assert!(super::caller_is_trusted_init(tid));
            assert_eq!(
                register(tid, tid + 1),
                Err(SyscallError::PermissionDenied),
                "not itself"
            );
            assert_eq!(
                register(tid, 0),
                Err(SyscallError::PermissionDenied),
                "not itself"
            );
            assert_eq!(register(tid, tid), Ok(0));
            assert_eq!(
                crate::cell::service_registry::lookup(MACHINE_PIN),
                Some(tid)
            );
            crate::cell::service_registry::clear_tid(tid);
        });
    }

    /// H-11: SpawnCap alone cannot rebind a well-known id (the USB host driver
    /// holds spawn for its LAN child); a SupervisorCap holder still rebinds it,
    /// and the spawn-only cell keeps publishing its own private id.
    #[test]
    fn register_service_refuses_well_known_hijack_by_a_spawn_only_cell() {
        use api::syscall::service::VFS;
        const HIJACKER: usize = 0x5_1D01;
        const SUPERVISOR: usize = 0x5_1D02;
        const PRIVATE_ID: u16 = 0x7A5E;
        with_scheduler_task_caps(0, CapSet::EMPTY, |_| {
            for (tid, caps) in [
                (
                    HIJACKER,
                    CapSet {
                        spawn: true,
                        usb_driver: true,
                        ..CapSet::EMPTY
                    },
                ),
                (
                    SUPERVISOR,
                    CapSet {
                        spawn: true,
                        supervisor: true,
                        ..CapSet::EMPTY
                    },
                ),
            ] {
                let mut task = alloc::boxed::Box::new(Task::new(
                    tid,
                    CellId(tid as u64),
                    "register-service-test",
                    alloc::vec::Vec::new(),
                ));
                caps.apply_to(&mut task);
                SCHEDULER
                    .lock()
                    .as_mut()
                    .expect("test scheduler")
                    .tasks
                    .insert(tid, task);
            }
            let register = |caller, service_id, tid| {
                super::handle_syscall(caller, Syscall::RegisterService { service_id, tid })
            };
            assert_eq!(
                register(HIJACKER, VFS, HIJACKER),
                Err(SyscallError::PermissionDenied)
            );
            assert_ne!(crate::cell::service_registry::lookup(VFS), Some(HIJACKER));
            assert_eq!(register(SUPERVISOR, VFS, SUPERVISOR), Ok(0));
            assert_eq!(crate::cell::service_registry::lookup(VFS), Some(SUPERVISOR));
            assert_eq!(register(HIJACKER, PRIVATE_ID, HIJACKER), Ok(0));
            crate::cell::service_registry::clear_tid(HIJACKER);
            crate::cell::service_registry::clear_tid(SUPERVISOR);
        });
    }

    #[test]
    fn mem_info_maps_args_and_requires_bit_56() {
        let syscall = map_syscall(ViSyscall::MemInfo as usize, 0x2000, 32, 0, 0)
            .expect("MemInfo must decode");
        match syscall {
            Syscall::MemInfo { out_ptr, out_len } => {
                assert_eq!(out_ptr, 0x2000);
                assert_eq!(out_len, 32);
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::MemInfo));
        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::MemInfo as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 56, |tid| {
            assert_eq!(check_allowlist(ViSyscall::MemInfo as usize, tid), Ok(()));
        });
    }

    #[test]
    fn get_cpu_count_maps_and_is_always_permitted() {
        let syscall = map_syscall(ViSyscall::GetCpuCount as usize, 0, 0, 0, 0)
            .expect("GetCpuCount must decode");
        assert!(matches!(syscall, Syscall::GetCpuCount));
        assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::GetCpuCount));
        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::GetCpuCount as usize, tid),
                Ok(())
            );
        });
    }

    /// Without `bench-counters`, raw 504 is exactly as unknown as before: no
    /// decode, and the allowlist gate denies it even with MemInfo's bit.
    #[cfg(not(feature = "bench-counters"))]
    #[test]
    fn bench_counters_opcode_is_unknown_without_the_feature() {
        assert!(map_syscall(504, 0x2000, 64, 0, 0).is_none());
        with_scheduler_task(1u64 << 56, |tid| {
            assert_eq!(
                check_allowlist(504, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
    }

    /// With `bench-counters`, raw 504 decodes outside `ViSyscall` and is gated
    /// exactly like MemInfo: allowlist bit 56 plus the `observe_memory` right.
    #[cfg(feature = "bench-counters")]
    #[test]
    fn bench_counters_opcode_maps_and_is_gated_like_mem_info() {
        let syscall = map_syscall(504, 0x2000, 64, 0, 0).expect("504 must decode");
        match syscall {
            Syscall::BenchCounters { out_ptr, out_len } => {
                assert_eq!(out_ptr, 0x2000);
                assert_eq!(out_len, 64);
            }
            other => panic!("decoded wrong syscall variant: {other:?}"),
        }
        assert_eq!(syscall_to_vi(&syscall), None);
        assert_eq!(ViSyscall::from(504), ViSyscall::Unknown);
        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(504, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 55, |tid| {
            assert_eq!(
                check_allowlist(504, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 56, |tid| {
            assert_eq!(check_allowlist(504, tid), Ok(()));
            // A short buffer is refused before any user access.
            assert_eq!(
                super::handle_syscall(
                    tid,
                    Syscall::BenchCounters {
                        out_ptr: 0x2000,
                        out_len: 63
                    }
                ),
                Err(SyscallError::InvalidInput)
            );
        });
        // The bit alone is not enough: the edge must grant observe_memory.
        for allowlist in [1u64 << 56, u64::MAX] {
            with_scheduler_task_caps(allowlist, CapSet::EMPTY, |tid| {
                assert_eq!(
                    check_allowlist(504, tid),
                    Err(SyscallError::PermissionDenied)
                );
            });
        }
        let m_only = CapSet {
            observe_memory: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(1u64 << 56, m_only, |tid| {
            assert_eq!(check_allowlist(504, tid), Ok(()));
        });
        // Kernel context (tid 0) is not gated, as for MemInfo.
        let _guard = crate::TEST_STATE_LOCK.lock();
        assert_eq!(check_allowlist(504, 0), Ok(()));
    }

    #[test]
    fn cwd_syscalls_map_args_and_require_bit_60() {
        let chdir =
            map_syscall(ViSyscall::Chdir as usize, 0x3000, 4, 0, 0).expect("Chdir must decode");
        assert!(matches!(
            chdir,
            Syscall::ChDir {
                path_ptr: 0x3000,
                path_len: 4
            }
        ));
        assert_eq!(syscall_to_vi(&chdir), Some(ViSyscall::Chdir));

        let getcwd =
            map_syscall(ViSyscall::Getcwd as usize, 0x4000, 64, 0, 0).expect("Getcwd must decode");
        assert!(matches!(
            getcwd,
            Syscall::GetCwd {
                buf_ptr: 0x4000,
                buf_len: 64
            }
        ));
        assert_eq!(syscall_to_vi(&getcwd), Some(ViSyscall::Getcwd));

        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::Chdir as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
            assert_eq!(
                check_allowlist(ViSyscall::Getcwd as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 60, |tid| {
            assert_eq!(check_allowlist(ViSyscall::Chdir as usize, tid), Ok(()));
            assert_eq!(check_allowlist(ViSyscall::Getcwd as usize, tid), Ok(()));
        });
    }

    #[test]
    fn fstat_maps_args_requires_bit_61_and_leaves_106_as_seek() {
        let fstat =
            map_syscall(ViSyscall::Fstat as usize, 7, 0x5000, 32, 0).expect("Fstat must decode");
        assert!(matches!(
            fstat,
            Syscall::Fstat {
                fd: 7,
                out_ptr: 0x5000,
                out_len: 32
            }
        ));
        assert_eq!(syscall_to_vi(&fstat), Some(ViSyscall::Fstat));

        let seek = map_syscall(106, 7, usize::MAX, 2, 0).expect("Seek 106 must decode");
        assert!(matches!(
            seek,
            Syscall::Seek {
                fd: 7,
                offset: -1,
                whence: 2
            }
        ));
        let rename = map_syscall(ViSyscall::Rename as usize, 0x1000, 10, 0x2000, 20)
            .expect("Rename must decode");
        assert!(matches!(
            rename,
            Syscall::Rename {
                old_ptr: 0x1000,
                old_len: 10,
                new_ptr: 0x2000,
                new_len: 20
            }
        ));
        assert_eq!(syscall_to_vi(&rename), Some(ViSyscall::Rename));

        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::Rename as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 62, |tid| {
            assert_eq!(check_allowlist(ViSyscall::Rename as usize, tid), Ok(()));
        });
        with_scheduler_task(1u64 << 63, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::Rename as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task((1u64 << 62) | (1u64 << 63), |tid| {
            assert_eq!(check_allowlist(ViSyscall::Rename as usize, tid), Ok(()));
        });

        with_scheduler_task(0, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::Fstat as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        with_scheduler_task(1u64 << 61, |tid| {
            assert_eq!(check_allowlist(ViSyscall::Fstat as usize, tid), Ok(()));
        });
    }

    #[test]
    fn dma_authority_requires_live_owner_binding_and_contained_range() {
        const OWNER: usize = 77;
        const BASE: usize = 0x5100_0000;
        const SIZE: usize = 0x4000;
        let binding = (CellId(9), 3);
        let page_grant = PageGrant {
            base: BASE,
            size: SIZE,
            owner: OWNER,
            owner_cell: binding.0,
            owner_generation: binding.1,
            shared_to: None,
            domain: None,
        };
        assert!(page_grant_authorizes_dma(
            &page_grant,
            OWNER,
            binding,
            BASE + 0x1000,
            0x2000
        ));
        assert!(!page_grant_authorizes_dma(
            &page_grant,
            OWNER + 1,
            binding,
            BASE,
            0x1000
        ));
        assert!(!page_grant_authorizes_dma(
            &page_grant,
            OWNER,
            (binding.0, binding.1 + 1),
            BASE,
            0x1000
        ));
        assert!(!page_grant_authorizes_dma(
            &page_grant,
            OWNER,
            binding,
            BASE + SIZE - 0x1000,
            0x2000
        ));

        let reg_grant = RegGrant {
            base: BASE,
            size: SIZE,
            owner: OWNER,
            owner_cell: binding.0,
            owner_generation: binding.1,
            shared_to: None,
            domain: None,
        };
        assert!(reg_grant_authorizes_dma(
            &reg_grant, OWNER, binding, BASE, SIZE
        ));
        assert!(!reg_grant_authorizes_dma(
            &reg_grant,
            OWNER,
            binding,
            BASE - 0x1000,
            0x2000
        ));
    }

    #[test]
    fn vfs_holder_release_before_reap_frees_without_orphaning_quarantine() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        const BASE: usize = 0x4f00_0000;
        const HOLDER: usize = 30_601;
        const OWNER: usize = 30_602;
        const GENERATION: u64 = 41;

        let before = crate::memory::pin::quarantined_pages();
        assert_eq!(
            crate::memory::pin::pin_vfs_lease(BASE, 4096, OWNER, HOLDER, 1, GENERATION),
            Ok(())
        );
        assert!(crate::memory::pin::mark_vfs_lease_pending_revoke(
            HOLDER, OWNER, GENERATION
        ));
        assert!(crate::memory::pin::release_vfs_lease(HOLDER, OWNER, GENERATION).is_empty());

        assert!(!withhold_or_free(BASE, 1));
        assert_eq!(crate::memory::pin::quarantined_pages(), before);
        assert!(crate::memory::pin::holder_of(BASE, 4096).is_none());
    }

    // ---- NET-OD: owner-death watch grant and TryRecv delivery -----------------

    const NET_TID: usize = 41_001;
    const NET_CELL: u64 = 44;
    const NET_GEN: u64 = 5;
    const CLIENT_TID: usize = 41_002;
    const CLIENT_CELL: u64 = 45;
    const CLIENT_GEN: u64 = 3;
    const OTHER_TID: usize = 41_003;
    const OTHER_CELL: u64 = 46;
    const OTHER_GEN: u64 = 2;
    /// A provider that is not the watcher.
    const FOREIGN_TID: usize = 41_009;

    /// The watching cell (receive context already set to the client, as the
    /// kernel does on delivery) and two live, published client principals.
    fn owner_watch_scheduler() -> Scheduler {
        let mut sched = Scheduler::new();
        let mut watcher = alloc::boxed::Box::new(Task::new(
            NET_TID,
            CellId(NET_CELL),
            "net-watcher",
            alloc::vec::Vec::new(),
        ));
        watcher.cell_generation = NET_GEN;
        watcher.root_tid = NET_TID;
        watcher.current_caller_cell_id = CLIENT_CELL;
        watcher.current_caller_cell_generation = CLIENT_GEN;
        sched.tasks.insert(NET_TID, watcher);
        for (tid, cell, generation) in [
            (CLIENT_TID, CLIENT_CELL, CLIENT_GEN),
            (OTHER_TID, OTHER_CELL, OTHER_GEN),
        ] {
            let mut client = alloc::boxed::Box::new(Task::new(
                tid,
                CellId(cell),
                "net-client",
                alloc::vec::Vec::new(),
            ));
            client.cell_generation = generation;
            client.root_tid = tid;
            sched.tasks.insert(tid, client);
            assert!(
                sched.publish_live_cell_owner(api::cell_owner::CellOwner::new(
                    cell, generation, tid as u64
                ))
            );
        }
        sched
    }

    /// One `watch_live_cell_owner` call by the watcher cell with exactly
    /// `registry` published (and `pause` applied) for the duration of the call
    /// only: the global registry is shared with tests that do not take
    /// `TEST_STATE_LOCK`.
    fn owner_watch(
        sched: &mut Scheduler,
        registry: &[(u16, usize, u64, u64)],
        pause: Option<(u16, usize)>,
        cell: u64,
        generation: u64,
    ) -> Option<(api::cell_owner::CellOwner, u64)> {
        for &(service, tid, cell_id, gen) in registry {
            assert!(crate::cell::service_registry::register(
                service, tid, cell_id, gen
            ));
        }
        if let Some((service, tid)) = pause {
            assert!(crate::cell::service_registry::pause(service, tid));
        }
        let result = sched.watch_live_cell_owner(NET_TID, CellId(cell), generation);
        for &(_, tid, _, _) in registry {
            crate::cell::service_registry::clear_tid(tid);
        }
        result
    }

    fn assert_owner_watch(
        label: &str,
        want: bool,
        registry: &[(u16, usize, u64, u64)],
        pause: Option<(u16, usize)>,
        mutate: impl FnOnce(&mut Scheduler),
        cell: u64,
        generation: u64,
    ) {
        let mut sched = owner_watch_scheduler();
        mutate(&mut sched);
        let granted = owner_watch(&mut sched, registry, pause, cell, generation);
        assert_eq!(granted.is_some(), want, "{label}");
        // A refusal installs nothing.
        assert_eq!(sched.cell_owner_watch_count(), usize::from(want), "{label}");
    }

    /// Every service id the registry can bind that is not in the owner-watch table.
    fn services_outside_owner_watch_table() -> alloc::vec::Vec<u16> {
        use api::syscall::service as s;
        [
            s::VFS,
            s::NET,
            s::INPUT,
            s::CONFIG,
            s::COMPOSITOR,
            s::SILO,
            s::HOTSWAP_DEMO,
            s::NET_BROKER,
            s::BLOCK_DRIVER,
            s::NIC_DRIVER,
            s::SUPERVISOR,
            s::GPU_DRIVER,
            s::KMS,
            s::AI,
            s::OCEL_JS,
        ]
        .into_iter()
        .filter(|id| !crate::task::scheduler::OWNER_WATCH_SERVICES.contains(id))
        .collect()
    }

    /// The grant for one table service: its Active registry-bound provider
    /// cell serving exactly the principal it names. `service` is the only
    /// registration in play, so the case proves that service's own entry.
    fn assert_owner_watch_grant_for(service: u16) {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let bound = [(service, NET_TID, NET_CELL, NET_GEN)];
        let nothing = |_: &mut Scheduler| {};

        // Granted: the Active binding, the attested serving context, a live
        // client generation. The watch names the client root.
        let mut sched = owner_watch_scheduler();
        let (owner, token) = owner_watch(&mut sched, &bound, None, CLIENT_CELL, CLIENT_GEN)
            .expect("registered watcher serving the client");
        assert_eq!(
            owner,
            api::cell_owner::CellOwner::new(CLIENT_CELL, CLIENT_GEN, CLIENT_TID as u64)
        );
        assert!(token > 0);
        assert_eq!(sched.cell_owner_watch_count(), 1);
        // Only the watcher's own token is cancellable, and only by that token.
        sched.cancel_cell_owner_watch(CLIENT_TID, token);
        sched.cancel_cell_owner_watch(OTHER_TID, token);
        assert_eq!(sched.cell_owner_watch_count(), 1);
        sched.cancel_cell_owner_watch(NET_TID, token + 1);
        assert_eq!(sched.cell_owner_watch_count(), 1);
        sched.cancel_cell_owner_watch(NET_TID, token);
        assert_eq!(sched.cell_owner_watch_count(), 0);

        assert_owner_watch(
            "baseline",
            true,
            &bound,
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Unregistered: refused.
        assert_owner_watch(
            "unregistered",
            false,
            &[],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Hot-swap quiesce: a Paused binding is refused.
        assert_owner_watch(
            "paused",
            false,
            &bound,
            Some((service, NET_TID)),
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Re-registered to another provider (tid, cell, generation).
        assert_owner_watch(
            "re-registered to another provider",
            false,
            &[(service, FOREIGN_TID, 47, 6)],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Same CellId, other generation: the stale watcher generation cannot hold it.
        assert_owner_watch(
            "stale watcher generation",
            false,
            &[(service, NET_TID, NET_CELL, NET_GEN + 1)],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "other watcher cell",
            false,
            &[(service, NET_TID, NET_CELL + 1, NET_GEN)],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Cell 0 is the kernel's allocation, never a provider binding.
        assert_owner_watch(
            "cell 0",
            false,
            &[(service, NET_TID, 0, NET_GEN)],
            None,
            |sched| {
                sched.tasks.get_mut(&NET_TID).expect("watcher").cell_id = CellId(0);
            },
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // The attested serving context must name the principal asked for.
        assert_owner_watch(
            "serving another cell",
            false,
            &bound,
            None,
            |sched| {
                sched
                    .tasks
                    .get_mut(&NET_TID)
                    .expect("watcher")
                    .current_caller_cell_id = OTHER_CELL;
            },
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "asking for another cell",
            false,
            &bound,
            None,
            nothing,
            OTHER_CELL,
            OTHER_GEN,
        );
        assert_owner_watch(
            "no serving context",
            false,
            &bound,
            None,
            |sched| {
                sched
                    .tasks
                    .get_mut(&NET_TID)
                    .expect("watcher")
                    .clear_current_caller_context();
            },
            CLIENT_CELL,
            CLIENT_GEN,
        );
        // Stale client generation: asked for, or recorded, but never live.
        assert_owner_watch(
            "stale client generation",
            false,
            &bound,
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN + 1,
        );
        assert_owner_watch(
            "serving a dead generation",
            false,
            &bound,
            None,
            |sched| {
                sched
                    .tasks
                    .get_mut(&NET_TID)
                    .expect("watcher")
                    .current_caller_cell_generation = CLIENT_GEN + 1;
            },
            CLIENT_CELL,
            CLIENT_GEN + 1,
        );
        // Dead client root.
        assert_owner_watch(
            "dead client root",
            false,
            &bound,
            None,
            |sched| {
                assert!(sched.tasks.remove(&CLIENT_TID).is_some());
            },
            CLIENT_CELL,
            CLIENT_GEN,
        );
    }

    /// The grant is the registry-bound live provider cell of a table service
    /// serving exactly the principal it names; everything else (and the VFS
    /// rule) is as before.
    #[test]
    fn owner_watch_grant_matrix() {
        use api::syscall::service::{COMPOSITOR, NET};
        for &service in &crate::task::scheduler::OWNER_WATCH_SERVICES {
            assert_owner_watch_grant_for(service);
        }

        let _guard = crate::TEST_STATE_LOCK.lock();
        let nothing = |_: &mut Scheduler| {};
        // Another service's provider with the same Cell identity is not a table
        // service, whichever table service the matrix is exercising.
        for service in services_outside_owner_watch_table() {
            assert_owner_watch(
                "other service provider",
                false,
                &[(service, NET_TID, NET_CELL, NET_GEN)],
                None,
                nothing,
                CLIENT_CELL,
                CLIENT_GEN,
            );
        }
        // A watcher bound to neither table service (both bound elsewhere) is
        // refused; a binding of one table service gives no right to the cell
        // that holds another service's binding.
        assert_owner_watch(
            "every table service bound to another provider",
            false,
            &[
                (NET, FOREIGN_TID, 47, 6),
                (COMPOSITOR, FOREIGN_TID + 1, 48, 7),
            ],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "NET bound to the watcher, COMPOSITOR bound elsewhere",
            true,
            &[
                (NET, NET_TID, NET_CELL, NET_GEN),
                (COMPOSITOR, FOREIGN_TID, 47, 6),
            ],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "COMPOSITOR bound to the watcher, NET bound elsewhere",
            true,
            &[
                (COMPOSITOR, NET_TID, NET_CELL, NET_GEN),
                (NET, FOREIGN_TID, 47, 6),
            ],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "both bound to the watcher, one paused",
            true,
            &[
                (NET, NET_TID, NET_CELL, NET_GEN),
                (COMPOSITOR, NET_TID, NET_CELL, NET_GEN),
            ],
            Some((NET, NET_TID)),
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );

        // Regression: the registered VFS cell is granted exactly as before, with
        // no service registration, and is held to the same serving-context proof.
        crate::fast_ipc::set_vfs_handler_cell(NET_CELL as usize);
        assert_owner_watch(
            "vfs cell",
            true,
            &[],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
        assert_owner_watch(
            "vfs cell, other principal",
            false,
            &[],
            None,
            nothing,
            OTHER_CELL,
            OTHER_GEN,
        );
        crate::fast_ipc::clear_vfs_if_cell(NET_CELL as usize);
        assert_owner_watch(
            "vfs cleared",
            false,
            &[],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
    }

    /// Tripwire: who may hold an owner watch is an explicit owner decision.
    /// Adding a service here is a reviewed change with its own consumer and
    /// grant-matrix coverage, never a silent widening.
    #[test]
    fn owner_watch_services_are_an_explicit_table() {
        assert_eq!(
            crate::task::scheduler::OWNER_WATCH_SERVICES,
            [
                api::syscall::service::NET,
                api::syscall::service::COMPOSITOR,
                api::syscall::service::XE_GPU
            ]
        );
    }

    /// `TryRecv` takes a queued owner death only for the wildcard mask, FIFO,
    /// one exact record at a time, and never a `NotifyOnExit` death.
    #[test]
    fn try_recv_takes_owner_death_only_for_mask_zero() {
        use super::{peek_owner_death_for_try_recv as peek, remove_owner_death as remove};
        let mut task = Task::new(41_010, CellId(NET_CELL), "try-recv", alloc::vec::Vec::new());
        assert_eq!(peek(&task, 0), None);

        task.pending_owner_deaths.push((11, 101, 0xDEAD));
        task.pending_owner_deaths.push((12, 102, 7));
        task.pending_deaths.push((900, 5));

        // A masked (dependency) receive never sees an owner death.
        for mask in [1usize, 101, 102, 900, usize::MAX] {
            assert_eq!(peek(&task, mask), None, "mask {mask}");
        }
        assert_eq!(task.pending_owner_deaths.len(), 2);

        // Wildcard: the oldest record, and peeking removes nothing.
        assert_eq!(peek(&task, 0), Some((11, 101, 0xDEAD)));
        assert_eq!(peek(&task, 0), Some((11, 101, 0xDEAD)));
        assert_eq!(task.pending_owner_deaths.len(), 2);

        // Commit removes exactly the delivered record; an unknown token is a no-op.
        remove(&mut task, 99);
        assert_eq!(task.pending_owner_deaths.len(), 2);
        remove(&mut task, 11);
        assert_eq!(peek(&task, 0), Some((12, 102, 7)));
        remove(&mut task, 12);
        assert_eq!(peek(&task, 0), None);
        assert!(task.pending_owner_deaths.is_empty());

        // NotifyOnExit deaths are untouched and never offered by TryRecv.
        assert_eq!(task.pending_deaths, alloc::vec![(900, 5)]);
    }

    /// CP1 #4: a nonzero `WaitIrq` base is admitted only when it is the exact
    /// base of an MMIO window the caller was granted; zero stays valid.
    #[test]
    fn wait_irq_base_requires_the_callers_exact_granted_window() {
        use super::wait_irq_base_permitted;
        const OWNER: usize = 0x7E1A;
        const BASE: usize = 0x7E1A_0000_0000;
        let _guard = crate::TEST_STATE_LOCK.lock();
        assert!(crate::resource_registry::request_mmio_unchecked(OWNER, BASE, 0x1000).is_ok());

        assert!(wait_irq_base_permitted(OWNER, 0));
        assert!(wait_irq_base_permitted(OWNER, BASE));
        // Another caller, an interior address and an ungranted base are refused.
        assert!(!wait_irq_base_permitted(OWNER + 1, BASE));
        assert!(!wait_irq_base_permitted(OWNER, BASE + 0x200));
        assert!(!wait_irq_base_permitted(OWNER, BASE + 0x1000));

        crate::resource_registry::release_mmio_for(OWNER);
        assert!(!wait_irq_base_permitted(OWNER, BASE));
        assert!(wait_irq_base_permitted(OWNER, 0));
    }

    /// CP1 #10: an `ShmAlloc` frame maps only for the incarnation that
    /// allocated it; another Cell or another generation of the same Cell is
    /// refused, as is a handle that was never issued.
    #[test]
    fn shm_frame_is_owned_by_exactly_the_allocating_incarnation() {
        use super::{shm_owned_by, shm_register, shm_take_for};
        use crate::cell::state_stash::Incarnation;
        const FRAME: usize = 0x5A0_0000;
        let owner = Incarnation {
            cell: 0x5A1,
            generation: 3,
        };
        shm_register(FRAME, owner, 4096, None, 0x3);

        assert!(shm_owned_by(FRAME, owner));
        assert!(!shm_owned_by(
            FRAME,
            Incarnation {
                cell: 0x5A2,
                generation: 3
            }
        ));
        assert!(!shm_owned_by(
            FRAME,
            Incarnation {
                cell: 0x5A1,
                generation: 4
            }
        ));
        assert!(!shm_owned_by(FRAME + 0x1000, owner));

        assert_eq!(shm_take_for(owner), (alloc::vec![FRAME], 4096));
        assert!(!shm_owned_by(FRAME, owner));
    }

    /// CP1 #10: root-retirement reclaim removes the dead incarnation's records
    /// and nothing else — not its successor generation's, not a peer's.
    #[test]
    fn shm_reclaim_takes_only_the_dead_incarnations_records() {
        use super::{shm_owned_by, shm_register, shm_take_for};
        use crate::cell::state_stash::Incarnation;
        let dead = Incarnation {
            cell: 0x5B1,
            generation: 2,
        };
        let successor = Incarnation {
            cell: 0x5B1,
            generation: 3,
        };
        let peer = Incarnation {
            cell: 0x5B2,
            generation: 2,
        };
        shm_register(0x5B0_0000, dead, 4096, None, 0x3);
        shm_register(0x5B0_1000, successor, 4096, None, 0x3);
        shm_register(0x5B0_2000, dead, 4096, None, 0x3);
        shm_register(0x5B0_3000, peer, 4096, None, 0x3);

        assert_eq!(
            shm_take_for(dead),
            (alloc::vec![0x5B0_0000, 0x5B0_2000], 8192)
        );
        assert!(!shm_owned_by(0x5B0_0000, dead));
        assert!(!shm_owned_by(0x5B0_2000, dead));
        assert!(shm_owned_by(0x5B0_1000, successor));
        assert!(shm_owned_by(0x5B0_3000, peer));
        assert_eq!(shm_take_for(dead), (alloc::vec![], 0));

        assert_eq!(shm_take_for(successor), (alloc::vec![0x5B0_1000], 4096));
        assert_eq!(shm_take_for(peer), (alloc::vec![0x5B0_3000], 4096));
    }

    #[test]
    fn shm_accessible_by_owner_and_authorized_recipient() {
        use super::{shm_accessible_by, shm_register};
        use crate::cell::state_stash::Incarnation;
        const FRAME: usize = 0x5C0_0000;
        let owner = Incarnation {
            cell: 10,
            generation: 1,
        };
        shm_register(FRAME, owner, 4096, Some(20), 0x3);

        assert!(shm_accessible_by(FRAME, 10, 1).is_some());
        assert!(shm_accessible_by(FRAME, 20, 1).is_some());
        assert!(shm_accessible_by(FRAME, 30, 1).is_none());
        assert!(shm_accessible_by(FRAME, 10, 2).is_none()); // Different generation
    }
}
