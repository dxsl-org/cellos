//! IPC System Calls (Inspired by Tock OS)
//!
//! This module defines the interface between "Cells/Silos" and the Kernel.
//! See [docs/architecture/03-driver-strategy.md] for the full rationale.

use super::tcb::TaskState;
use crate::sync::Spinlock;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
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

/// Set of physical frames currently issued via `ShmAlloc`.
/// `ShmMap` accepts only handles that appear here, preventing a malicious
/// cell from mapping arbitrary kernel/cell-owned frames into its address
/// space via a forged handle.
///
/// NOTE: This is still a single global pool — any cell that knows a peer's
/// outstanding handle can map it. A per-owner ACL is the proper fix; this
/// table is the minimum bar to stop "ShmMap kernel_text_phys" attacks.
static SHM_HANDLES: Spinlock<Option<BTreeSet<usize>>> = Spinlock::new(None);

fn shm_handles_lock() -> &'static Spinlock<Option<BTreeSet<usize>>> {
    &SHM_HANDLES
}

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

fn shm_register(handle: usize) {
    let mut guard = shm_handles_lock().lock();
    if guard.is_none() {
        *guard = Some(BTreeSet::new());
    }
    if let Some(set) = guard.as_mut() {
        set.insert(handle);
    }
}

fn shm_is_valid(handle: usize) -> bool {
    let guard = shm_handles_lock().lock();
    guard.as_ref().is_some_and(|set| set.contains(&handle))
}

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
}

static PAGE_GRANT_TABLE: Spinlock<Option<BTreeMap<usize, PageGrant>>> = Spinlock::new(None);

fn grant_table_lock() -> &'static Spinlock<Option<BTreeMap<usize, PageGrant>>> {
    &PAGE_GRANT_TABLE
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
                let _ = crate::memory::paging::unmap_page(f);
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
/// Lock order: FRAME_ALLOCATOR → (map_page/unmap_page → KERNEL_ROOT). Mirrors
/// `Stack::drop`. Must NOT be called while already holding FRAME_ALLOCATOR.
fn free_grant_pages(base: usize, n_pages: usize) {
    use crate::memory::frame::FRAME_ALLOCATOR;
    use crate::memory::paging::Flags;
    const PAGE_SIZE: usize = 4096;
    let kernel_rwx = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );

    let mut fa = FRAME_ALLOCATOR.lock();
    if let Some(alloc) = fa.as_mut() {
        for i in 0..n_pages {
            let f = base + i * PAGE_SIZE;
            let _ = crate::memory::paging::unmap_page(f);
            let _ = crate::memory::paging::map_page(alloc, f, f, kernel_rwx);
            alloc.deallocate_frame(f);
        }
    }
    crate::memory::paging::tlb_flush_all();
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
    let entry = {
        let mut table = reg_grant_table_lock().lock();
        let owned = table
            .as_ref()
            .and_then(|grants| grants.get(&reg_id))
            .filter(|grant| grant.owner == caller_id)
            .map(|grant| (grant.base, grant.size));
        match owned {
            Some((base, size)) => {
                refuse_if_pinned("GrantUnregister", reg_id, base, size)?;
                table.as_mut().and_then(|grants| grants.remove(&reg_id))
            }
            None => None,
        }
    }
    .ok_or(SyscallError::PermissionDenied)?;
    free_grant_pages(entry.base, grant_pages_for_size(entry.size));
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
pub(crate) fn reap_grants_for_task(dead_tid: usize) {
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

    // ── PAGE_GRANT_TABLE pass ─────────────────────────────────────────────────
    let owned: alloc::vec::Vec<PageGrant> = {
        let mut tbl = grant_table_lock().lock();
        let mut owned = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            // Clear grantee references (no removal needed — owner keeps the entry).
            for grant in map.values_mut() {
                if grant.shared_to.is_some_and(|(tid, _)| tid == dead_tid) {
                    grant.shared_to = None;
                }
            }
            // Collect and remove owned entries.
            let owned_keys: alloc::vec::Vec<usize> = map
                .iter()
                .filter(|(_, g)| g.owner == dead_tid)
                .map(|(k, _)| *k)
                .collect();
            owned = owned_keys.iter().filter_map(|k| map.remove(k)).collect();
        }
        owned
    }; // PAGE_GRANT_TABLE lock released

    for grant in &owned {
        if withhold_or_free(grant.base, grant_pages_for_size(grant.size)) {
            continue;
        }
        free_grant_pages(grant.base, grant_pages_for_size(grant.size));
    }

    // ── REG_GRANT_TABLE pass ──────────────────────────────────────────────────
    let reg_owned: alloc::vec::Vec<RegGrant> = {
        let mut tbl = reg_grant_table_lock().lock();
        let mut removed = alloc::vec::Vec::new();
        if let Some(map) = tbl.as_mut() {
            // Clear grantee references when the grantee dies.
            for grant in map.values_mut() {
                if grant.shared_to.is_some_and(|(tid, _)| tid == dead_tid) {
                    grant.shared_to = None;
                }
            }
            let mut owned_keys = alloc::vec::Vec::new();
            let mut orphan_keys = alloc::vec::Vec::new();
            for (&key, grant) in map.iter_mut() {
                if grant.owner == dead_tid {
                    if let Some((grantee, _)) = grant.shared_to {
                        if crate::memory::pin::vfs_holder_of_owner(
                            grant.base,
                            grant_allocated_bytes(grant.size),
                            dead_tid,
                        )
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
                } else if grant.owner == 0 && grant.shared_to.is_none() {
                    orphan_keys.push(key);
                }
            }
            owned_keys.extend(orphan_keys);
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
/// returns `true`: leaking the frames is the only safe answer left.
fn withhold_or_free(base: usize, pages: usize) -> bool {
    match crate::memory::pin::withhold_pinned_frames(base, pages) {
        crate::memory::pin::FrameTransfer::Free => false,
        crate::memory::pin::FrameTransfer::Withheld => true,
        crate::memory::pin::FrameTransfer::Full => {
            log::error!(
                "[grant] quarantine full: leaking {pages} page(s) at {base:#x} protected by a live pin"
            );
            true
        }
    }
}

/// Return quarantined frames owned by `tid` to the allocator once the driver has
/// acknowledged that nothing can still reach them.
///
/// The acknowledgement point is `iommu::cleanup_cell(tid)`: with the cell's IOTLB
/// entries flushed and its DDT/context entries zeroed, no device it authorised
/// can address the frames. Call this immediately after that teardown, and only
/// with a task id — the pin registry is keyed by task, not by cell.
///
/// Order-insensitive with respect to [`reap_grants_for_task`]: an acknowledgement
/// that arrives first drops the pins, so the reaper then frees the frames itself.
///
/// Must run with neither FRAME_ALLOCATOR nor SCHEDULER held.
pub(crate) fn release_acked_frames(tid: usize) {
    for (base, pages) in crate::memory::pin::acknowledge(tid) {
        log::info!(
            "[grant] task {tid} acknowledged: releasing {pages} quarantined page(s) at {base:#x}"
        );
        free_grant_pages(base, pages);
    }
}

pub(crate) fn release_vfs_holder_leases(tid: usize) {
    for (base, pages) in crate::memory::pin::release_vfs_holder_leases(tid) {
        log::info!(
            "[grant] VFS holder {tid} died: releasing {pages} quarantined page(s) at {base:#x}"
        );
        free_grant_pages(base, pages);
    }
}

/// Release the exact VFS lease whose owner death cleared the holder's caller
/// context. This runs outside SCHEDULER after grant reaping has quarantined any
/// owner-dead frames.
pub(crate) fn release_vfs_context_lease(release: super::scheduler::VfsLeaseRelease) {
    for (base, pages) in crate::memory::pin::release_vfs_lease(
        release.holder_tid,
        release.grant_owner,
        release.request_generation,
    ) {
        free_grant_pages(base, pages);
    }
}

/// Result of a System Call
pub type SyscallResult = core::result::Result<usize, SyscallError>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SyscallError {
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

/// Encode the additive spawn-OOM result while preserving all other legacy errors.
fn encode_syscall_result(
    result: SyscallResult,
    error_sentinel: usize,
    supports_typed_oom: bool,
) -> usize {
    match result {
        Ok(value) => value,
        Err(SyscallError::OutOfMemory) if supports_typed_oom => error_sentinel - 1,
        Err(_) => error_sentinel,
    }
}

fn supports_typed_spawn_oom(syscall: &Syscall) -> bool {
    matches!(
        syscall,
        Syscall::SpawnFromMem { .. }
            | Syscall::SpawnFromPath { .. }
            | Syscall::SpawnFromElf { .. }
            | Syscall::SpawnReplacement { .. }
            | Syscall::SpawnPinned { .. }
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
fn caller_has_cap<F: Fn(&crate::task::tcb::Task) -> bool>(caller_id: usize, check: F) -> bool {
    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|t| check(t))
        .unwrap_or(false)
}

fn caller_has_block_io(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.block_io_cap.is_some())
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
        .and_then(|s| s.tasks.get(&caller_id))
        .map(|t| t.block_regions)
        .unwrap_or(0);
    let end = match sector.checked_add(count) {
        Some(e) => e,
        None => return false,
    };
    const GRANTABLE: [(u8, u64, u64); 4] = [
        (0b001, dl::PART_FAT32_BASE_LBA, dl::PART_FAT32_SECTORS), // P1 (PART_DATA)
        (0b010, dl::PART_LFS_BASE_LBA, dl::PART_LFS_SECTORS),     // P4 (PART_LFS)
        (0b100, dl::PART_SRV_BASE_LBA, dl::PART_SRV_SECTORS), // P5 (SRV/RedoxFS, co-granted w/ LFS)
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
    caller_has_cap(caller_id, |t| t.network_cap.is_some())
}

fn caller_has_hypervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.hypervisor_cap.is_some())
}
#[cfg(feature = "test-hooks")]
fn caller_has_development_silo_registration(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |task| {
        task.root_tid == caller_id && task.development_silo_registration_cap.is_some()
    })
}

fn caller_has_spawn(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.spawn_cap.is_some())
}

const SPAWN_ARGV_KEY: u64 = 0x0061_7267_7600_0000;
const SPAWN_ARGV_MAX: usize = 512;

fn spawn_argv_slot(task_id: usize) -> u64 {
    crate::cell::state_stash::spawn_argv_key(task_id)
}

fn governed_spawn_request(
    caller_id: usize,
    child_ceiling: super::cap::CapSet,
    priority: u8,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (generation, caller_authority) = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|task| (task.cell_generation, super::cap::CapSet::of_task(task)))
        .ok_or(SyscallError::PermissionDenied)?;
    let argv = crate::cell::state_stash::take_spawn_argv(caller_id);
    Ok(crate::loader::SpawnRequest::governed_caller(
        caller_id,
        generation,
        caller_authority,
        child_ceiling,
        priority,
        argv,
    ))
}

fn governed_replacement_request(
    caller_id: usize,
    route_ceiling: super::cap::CapSet,
    replacement: crate::cell::hotswap::ReplacementReservation,
) -> Result<crate::loader::SpawnRequest, SyscallError> {
    let (generation, caller_authority) = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|task| (task.cell_generation, super::cap::CapSet::of_task(task)))
        .ok_or(SyscallError::PermissionDenied)?;
    let argv = crate::cell::state_stash::take_spawn_argv(caller_id);
    Ok(crate::loader::SpawnRequest::governed_replacement(
        caller_id,
        generation,
        caller_authority,
        route_ceiling,
        replacement,
        argv,
    ))
}

fn caller_launch_state(caller_id: usize) -> Option<(String, bool, bool)> {
    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|task| {
            (
                task.name.clone(),
                task.spawn_cap.is_some(),
                task.supervisor_cap.is_some(),
            )
        })
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

fn caller_has_supervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.supervisor_cap.is_some())
}

fn caller_has_pcie_driver(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.pcie_driver_cap.is_some())
}

fn caller_has_mmio_device(caller_id: usize, device: u8) -> bool {
    caller_has_cap(caller_id, |t| t.mmio_devices & device != 0)
}

fn caller_has_platform(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.platform_cap.is_some())
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

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
fn record_end_containing(base: usize, size: usize, ptr: usize) -> Option<usize> {
    let end = base.checked_add(size)?;
    (base <= ptr && ptr < end).then_some(end)
}

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
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
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
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
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
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
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
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
            .map(|task| TaskCopyView::of(task))
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
        payload: alloc::vec::Vec<u8>,
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
        let mut payload = alloc::vec::Vec::new();
        payload
            .try_reserve_exact(msg.payload().len())
            .map_err(|_| ())?;
        payload.extend_from_slice(msg.payload());
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
pub fn attested_identity_of(sender_tid: usize) -> Option<api::caller_identity::CallerIdentity> {
    let sched_guard = super::SCHEDULER.lock();
    let sched = sched_guard.as_ref()?;
    let task = sched.tasks.get(&sender_tid)?;
    let owner = sched.resolve_live_cell_owner(task.cell_id, task.cell_generation)?;
    (owner.cell_id == task.cell_id.0 && owner.generation == task.cell_generation).then_some(
        api::caller_identity::CallerIdentity {
            cell_id: task.cell_id.0,
            generation: task.cell_generation,
            sender_tid: sender_tid as u64,
        },
    )
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
    for (base, pages) in released {
        free_grant_pages(base, pages);
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
    for (base, pages) in
        crate::memory::pin::release_vfs_lease(caller_id, grant_owner, request_generation)
    {
        free_grant_pages(base, pages);
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
    vfs_context: Option<VfsGrantContext>,
}

struct GrantSliceAccess {
    base: usize,
    size: usize,
    size_out: Option<*mut usize>,
}

fn authorize_grant_slice_locked(
    request: GrantSliceRequest,
    grant_owner: usize,
    shared_to_tid: Option<usize>,
    base: usize,
    size: usize,
) -> Result<Option<GrantSliceAccess>, SyscallError> {
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
    if request.vfs_context.is_some_and(|context| {
        !install_vfs_lease_if_context_live(request.caller_id, context, base, size, request.grant_id)
    }) {
        return Ok(None);
    }
    Ok(Some(GrantSliceAccess {
        base,
        size,
        size_out,
    }))
}

/// Resolve a grant and publish its matching VFS lease in one table transaction.
///
/// This is the GrantSlice/free linearization point. If teardown removes the
/// entry first, neither table can resolve it. If this lookup wins, a VFS lease
/// reaches the pin registry before the table lock is released, so GrantFree or
/// GrantUnregister observes the pin and refuses.
fn resolve_and_lease_grant(
    caller_id: usize,
    grant_id: usize,
    size_out_ptr: usize,
    vfs_context: Option<VfsGrantContext>,
) -> Result<Option<GrantSliceAccess>, SyscallError> {
    let request = GrantSliceRequest {
        caller_id,
        grant_id,
        size_out_ptr,
        vfs_context,
    };
    {
        let tbl = grant_table_lock().lock();
        if let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) {
            return authorize_grant_slice_locked(
                request,
                grant.owner,
                grant.shared_to.as_ref().map(|(tid, _)| *tid),
                grant.base,
                grant.size,
            );
        }
    }
    let tbl = reg_grant_table_lock().lock();
    let Some(grant) = tbl.as_ref().and_then(|map| map.get(&grant_id)) else {
        return Ok(None);
    };
    authorize_grant_slice_locked(
        request,
        grant.owner,
        grant.shared_to.as_ref().map(|(tid, _)| *tid),
        grant.base,
        grant.size,
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
fn write_caller_identity(buf_ptr: usize, buf_len: usize, sender_tid: usize) {
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
    TaskCopyView::sas().write_bytes(dst, &trailer).ok(); // best-effort: if the write fails (bad ptr), receiver sees zeroed trailer → deny
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

fn encode_task_state(state: &TaskState) -> u32 {
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
    Reply { caller: usize, result: usize },
    /// 3: SetTimer (Wake up after ticks)
    SetTimer { deadline: usize },
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
    Spawn { entry: usize, arg: usize },
    /// 9: FutexWait (Wait for value at address)
    FutexWait { addr: usize, val: u32 },
    /// 10: FutexWake (Wake up waiting tasks)
    FutexWake { addr: usize, count: usize },
    /// 11: Log (Debug Print)
    Log { msg_ptr: usize, msg_len: usize },
    /// 12: Grant (Zero Copy)
    Grant {
        target: usize,
        ptr: usize,
        len: usize,
        flags: usize,
    },
    /// 13: Map (Zero Copy)
    Map { grant_id: usize },
    /// 14: Exit (Terminate Process)
    Exit { code: usize },
    /// 61: ForceExit — terminate another task by TID; non-blocking return to caller.
    ForceExit { tid: usize },
    /// 204: NotifyOnExit — register the caller to be notified when `watched` dies.
    NotifyOnExit { watched: usize },
    /// 205: RegisterService — register `tid` as the current provider of `service_id`.
    /// SpawnCap owns the namespace; fixed-ID self-registration uses narrower caps.
    RegisterService { service_id: u16, tid: usize },
    /// 206: LookupService — resolve `service_id` to its live provider tid (open; 0 = none).
    LookupService { service_id: u16 },
    /// 207: Heartbeat — caller asserts liveness; (re)arm the hung-detection deadline
    /// `interval` ticks ahead (0 = disable).
    Heartbeat { interval: usize },
    /// 6: Exec (Spawn from file)
    Exec { path_ptr: usize, path_len: usize },
    /// 10: SpawnFromMem (Spawn from Memory buffer via Struct)
    SpawnFromMem { args_ptr: usize },
    /// 12: SpawnFromPath (Spawn cell by filesystem path)
    /// ABI: path_ptr in a0, path_len in a1.
    SpawnFromPath { path_ptr: usize, path_len: usize },
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
    SpawnSetDirs { carrier_ptr: usize },
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
    CancelCellOwnerWatch { token: u64 },
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
    /// 13: OpenCap — open a file and return a CapId.
    OpenCap { path_ptr: usize, path_len: usize },
    /// 14: ReadCap — read bytes from a cap-backed file.
    ReadCap {
        cap_id: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 15: CloseCap — revoke a capability.
    CloseCap { cap_id: usize },
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
    StatCap { cap_id: usize },
    /// 231: TruncateCap — truncate file to len bytes via cap.
    TruncateCap { cap_id: usize, len: usize },
    /// 232: SyncCap — flush dirty pages to block device via cap.
    SyncCap { cap_id: usize },
    /// 233: GrantDma — map PCIe DMA range into IOMMU for calling Cell.
    /// a0 = bdf: u32, a1 = phys: u64, a2 = size: usize.
    GrantDma { bdf: u32, phys: u64, size: usize },
    /// 8: Wait (Wait for task)
    Wait { pid: usize },
    /// 20: ShmAlloc
    ShmAlloc { size: usize },
    /// 21: ShmMap
    ShmMap { handle: usize, target_pid: usize },
    /// 30: GetProcs
    GetProcs { buf_ptr: usize, buf_len: usize },
    /// 239: GetProcs2
    GetProcs2 { buf_ptr: usize, buf_len: usize },
    /// 243: MemInfo
    MemInfo { out_ptr: usize, out_len: usize },

    // --- Legacy / Compatibility Layer ---
    /// 100: Service Lookup (Find driver ID by name)
    ServiceLookup { name_ptr: usize, name_len: usize },
    /// 101: Open (Path -> FD)
    Open { path_ptr: usize, path_len: usize },
    /// 102: Read (FD, Buffer -> Bytes Read)
    Read {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 103: Close (FD)
    Close { fd: usize },
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
    /// 107: ChDir (Change Directory)
    ChDir { path_ptr: usize, path_len: usize },
    /// 108: GetCwd (Get Current Directory)
    GetCwd { buf_ptr: usize, buf_len: usize },
    /// 109: Write (FD, Buffer -> Bytes Written)
    Write {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 110: MkDir (Path)
    MkDir { path_ptr: usize, path_len: usize },
    /// 111: Create (Path -> FD)
    /// 111: Create (Path -> FD)
    Create { path_ptr: usize, path_len: usize },
    /// 104: Yield (Legacy)
    Yield,
    /// 106: Seek (FD, Offset, Whence)
    Seek {
        fd: usize,
        offset: isize,
        whence: usize,
    },
    /// 107: FileOp (Op, Arg1, Arg2)
    FileOp { op: usize, arg1: usize, arg2: usize },
    /// 120: GetTime (Op)
    GetTime { op: usize },
    /// 300: GpuFlush — copy cell pixel buffer to VirtIO GPU framebuffer.
    GpuFlush {
        data_ptr: usize,
        data_len: usize,
        xy: usize,
        wh: usize,
    },
    /// 218: AudioPlay — write raw PCM (S16LE/2ch/44100) to the VirtIO sound output.
    AudioPlay { buf_ptr: usize, buf_len: usize },
    /// 219: CapRevoke — strip capabilities from a live cell at runtime.
    CapRevoke { target_tid: usize, cap_mask: u32 },
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
    NetTx { frame_ptr: usize, frame_len: usize },
    /// 311: NetRx — receive one pending Ethernet frame from the VirtIO NIC.
    NetRx { buf_ptr: usize, buf_len: usize },
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
    StateStashClear { key: usize },
    /// 413: FreezeCell — freeze a running Cell. Requires SupervisorCap.
    FreezeCell { target_tid: usize },
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
    KillCell { target_tid: usize, exit_code: u32 },
    /// 416: RegisterBlockDriver — announce caller as the active block driver.
    RegisterBlockDriver,
    /// 417: RegisterNicDriver — announce caller as the active NIC driver.
    RegisterNicDriver,
    /// 418: FindPcieDevice — locate a PCIe device by class/subclass/prog_if.
    FindPcieDevice {
        class: u8,
        subclass: u8,
        prog_if: u8,
        out_ptr: usize,
    },
    /// 419: QueryHotswapReady — check whether `target_tid` has called sys_hotswap_ready().
    /// Returns 1 if ready, 0 if not yet, usize::MAX if tid is unknown.
    QueryHotswapReady { target_tid: usize },
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
    BlkRead { sector: u64, buf_ptr: usize },
    /// 501: BlkWrite — write one 512-byte sector to the VirtIO block device.
    BlkWrite { sector: u64, buf_ptr: usize },
    /// 502: Shutdown — trigger SBI SRST system shutdown (S-mode → OpenSBI). No return.
    Shutdown,
    /// 503: BlkFlush — flush the VirtIO block device write cache to the backing image.
    BlkFlush,
    /// 208: GrantAlloc — allocate n pages as a zero-copy Grant region.
    GrantAlloc { size: usize },
    /// 209: GrantShare — share a Grant region with `target_cell` under `perm`.
    GrantShare {
        grant_id: usize,
        target_cell: usize,
        perm: usize,
    },
    /// 210: GrantSlice — return the user-space pointer for a Grant the caller owns/holds.
    GrantSlice {
        grant_id: usize,
        size_out_ptr: usize,
    },
    /// 211: GrantFree — unmap + deallocate a Grant region.
    GrantFree { grant_id: usize },
    /// 212: BlkReadAsync — synchronous-but-zero-copy sector read into a Grant buffer.
    BlkReadAsync { sector: u64, grant_id: usize },
    /// 213: RequestMmio — claim exclusive MMIO range for a peripheral Driver Cell.
    RequestMmio { base: usize, len: usize },
    /// 214: GetRandom — fill a caller buffer with VirtIO-RNG entropy bytes.
    GetRandom { buf_ptr: usize, len: usize },
    /// 215: GrantRegister — allocate a persistent pre-pinned Grant buffer (lifetime = cell exit).
    GrantRegister { size: usize },
    /// 216: GrantUnregister — explicitly release a registered buffer.
    GrantUnregister { reg_id: usize },
    /// 217: WaitForEvent — block until `mask` bits fire or `deadline` ticks pass.
    /// `deadline = None` means block indefinitely.
    WaitForEvent { mask: u32, deadline: Option<u64> },
    /// 249: synchronize an owned Grant subrange before device submission.
    GrantCacheSyncBegin {
        grant_id: usize,
        offset: usize,
        len: usize,
    },
    /// 250: synchronize device output and release one exact Grant pin.
    GrantCacheSyncComplete { token: usize },
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
    CreateVm { guest_pages: usize },
    /// 221: CreateVcpu — create a vCPU with `entry_pc` in `vm_id` → vcpu_id.
    CreateVcpu { vm_id: usize, entry_pc: u64 },
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
    /// 224: VcpuRegs — read (write=false) or write (write=true) GP registers.
    VcpuRegs {
        vm_id: usize,
        vcpu_id: usize,
        buf_ptr: usize,
        write: bool,
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
    WaitIrq { irq: u8, mmio_base: usize },
    /// 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
    /// Populates the kernel BAR allowlist used by `sys_request_mmio`.
    /// Requires singleton PlatformCap.
    RegisterPcieBar { bdf: u32, base: usize, len: usize },
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
    ReadLog { buf_ptr: usize, max: usize },
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
        Syscall::Log { .. } => V::Log,
        Syscall::SetTimer { .. } => V::SetTimer,
        Syscall::ShmAlloc { .. } => V::ShmAlloc,
        Syscall::ShmMap { .. } => V::ShmMap,
        Syscall::GetProcs { .. } => V::GetProcs,
        Syscall::GetProcs2 { .. } => V::GetProcs2,
        Syscall::MemInfo { .. } => V::MemInfo,
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
        Syscall::RegisterBlockDriver => V::RegisterBlockDriver,
        Syscall::RegisterNicDriver => V::RegisterNicDriver,
        Syscall::FindPcieDevice { .. } => V::FindPcieDevice,
        Syscall::Exec { .. } => V::Exec,
        Syscall::LookupService { .. } => V::LookupService,
        Syscall::Heartbeat { .. } => V::Heartbeat,
        Syscall::GrantAlloc { .. } => V::GrantAlloc,
        Syscall::GrantShare { .. } => V::GrantShare,
        Syscall::GrantSlice { .. } => V::GrantSlice,
        Syscall::GrantFree { .. } => V::GrantFree,
        Syscall::BlkReadAsync { .. } => V::BlkReadAsync,
        Syscall::RequestMmio { .. } => V::RequestMmio,
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
        | Syscall::RegisterService { .. }
        | Syscall::CapRevoke { .. }
        // SpawnCap / VFS-provider gated at dispatch; see ViSyscall::allowlist_bit.
        | Syscall::SpawnSetDirs { .. }
        | Syscall::QueryDirHandles { .. }
        | Syscall::ResolveCellOwner { .. }
        | Syscall::WatchCellOwner { .. }
        | Syscall::CancelCellOwnerWatch { .. }
        | Syscall::ResolveCellOwnerRecord { .. }
        | Syscall::WatchCellOwnerRecord { .. } => return None,
        // Raw block-I/O (500-503): ZST BlockIoCap gated at dispatch.
        Syscall::BlkRead { .. }
        | Syscall::BlkWrite { .. }
        | Syscall::BlkFlush
        | Syscall::Shutdown => return None,
        // Legacy / internal variants without allowlist bits.
        _ => return None,
    })
}

/// Dispatches a system call to the appropriate handler.
///
/// `caller_id` is the ID of the task invoking the syscall.
pub fn handle_syscall(caller_id: usize, syscall: Syscall) -> SyscallResult {
    let Some(allowed) = syscall_allowlist_for(caller_id) else {
        log::warn!("[kernel] syscall denied for non-live tid {}", caller_id);
        return Err(SyscallError::PermissionDenied);
    };

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
                crate::audit::log_event(
                    crate::audit::AuditEvent::SyscallDenied,
                    &crate::audit::encode_u32x2(caller_id as u32, bit as u32),
                );
                return Err(SyscallError::PermissionDenied);
            }
        }
    }

    match syscall {
        // --- Hubris ABI Implementation ---
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
            // (Input-service sends fall back to a bounded queue — see ipc_try_send.)
            match super::ipc_try_send(caller_id, target, msg_ptr, msg_len) {
                Ok(()) => Ok(0),           // delivered (or queued for the input service)
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
                    write_caller_identity(buf_ptr, buf_len, sender_tid);
                }
            };

            let mut vfs_context_drop = None;
            let death_info = {
                let mut guard = super::SCHEDULER.lock();
                guard.as_mut().and_then(|sched| {
                    let t = sched.tasks.get_mut(&caller_id)?;
                    vfs_context_drop = t.begin_receive_context(mask);
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
            finish_vfs_context_drop(caller_id, vfs_context_drop);

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
                    let mut data = alloc::vec::Vec::new();
                    if data.try_reserve_exact(msg.payload().len()).is_err() {
                        return Err(());
                    }
                    data.extend_from_slice(msg.payload());
                    let caller_view = TaskCopyView::of(t);
                    let (sender_cell_id, sender_generation) = match wire_header {
                        Some(header) => (header.sender_cell_id, header.sender_generation),
                        None => sender_cell_context_in_sched(sched, sender_tid),
                    };
                    Ok(Some((
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        caller_view,
                        data,
                    )))
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
                    let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
                    match res {
                        Ok(0) => {
                            super::yield_cpu();
                            // Peek the next event without removing it,
                            // copy payload, then commit the exact record.
                            let snap = {
                                let guard = super::SCHEDULER.lock();
                                guard.as_ref().map_or(
                                    Ok(ResumeSnapshot::Wake { sender_tid: 0 }),
                                    |sched| {
                                        sched.tasks.get(&caller_id).map_or(
                                            Ok(ResumeSnapshot::Wake { sender_tid: 0 }),
                                            |task| snapshot_resume(task, mask),
                                        )
                                    },
                                )
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
                                        ..
                                    } = &snap
                                    {
                                        task.set_received_caller_context(
                                            *sender_tid,
                                            *sender_cell_id,
                                            *sender_generation,
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
            }
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    if pos < t.pending_msgs.len()
                        && t.pending_msgs.as_slice()[pos].sender_tid == sender_tid
                    {
                        t.pending_msgs.remove(pos);
                    }
                    t.set_received_caller_context(sender_tid, sender_cell_id, sender_generation);
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
                    let mut tmp = alloc::vec::Vec::new();
                    if tmp.try_reserve_exact(record.payload().len()).is_err() {
                        return Err(());
                    }
                    tmp.extend_from_slice(record.payload());
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
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    if pos < t.pending_msgs.len()
                        && t.pending_msgs.as_slice()[pos].sender_tid == sender_tid
                    {
                        t.pending_msgs.remove(pos);
                    }
                    if let Some(header) = wire_header {
                        // Commit caller context together with removal so a
                        // reply after scatter carries the delivered sender's
                        // identity and request generation.
                        t.set_received_caller_context(
                            sender_tid,
                            header.sender_cell_id,
                            header.sender_generation,
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
                    let mut data = alloc::vec::Vec::new();
                    if data.try_reserve_exact(record.payload().len()).is_err() {
                        return Err(());
                    }
                    data.extend_from_slice(record.payload());
                    let caller_view = TaskCopyView::of(t);
                    let (sender_cell_id, sender_generation) = match wire_header {
                        Some(header) => (header.sender_cell_id, header.sender_generation),
                        None => sender_cell_context_in_sched(sched, sender_tid),
                    };
                    Ok(Some((
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        caller_view,
                        data,
                    )))
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
                                let guard = super::SCHEDULER.lock();
                                guard.as_ref().map_or(
                                    Ok(ResumeSnapshot::Wake { sender_tid: 0 }),
                                    |sched| {
                                        sched.tasks.get(&caller_id).map_or(
                                            Ok(ResumeSnapshot::Wake { sender_tid: 0 }),
                                            |task| snapshot_resume(task, mask),
                                        )
                                    },
                                )
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
                                        ..
                                    } = &snap
                                    {
                                        task.set_received_caller_context(
                                            *sender_tid,
                                            *sender_cell_id,
                                            *sender_generation,
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
            }
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&caller_id) {
                    if pos < t.pending_msgs.len()
                        && t.pending_msgs.as_slice()[pos].sender_tid == sender_tid
                    {
                        t.pending_msgs.remove(pos);
                    }
                    t.set_received_caller_context(sender_tid, sender_cell_id, sender_generation);
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
                    let mut data = alloc::vec::Vec::new();
                    if data.try_reserve_exact(msg.payload().len()).is_err() {
                        return Err(());
                    }
                    data.extend_from_slice(msg.payload());
                    // t is no longer borrowed here.
                    let (sender_cell_id, sender_generation) = inline_cell_context
                        .unwrap_or_else(|| sender_cell_context_in_sched(sched, sender_tid));
                    Ok(Some((
                        pos,
                        sender_tid,
                        sender_cell_id,
                        sender_generation,
                        wire_header,
                        delivery_id,
                        caller_view,
                        data,
                    )))
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
                }
                if attest_caller {
                    write_caller_identity(buf_ptr, buf_len, sender_tid);
                }
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    if let Some(t) = sched.tasks.get_mut(&caller_id) {
                        // Commit by delivery_id (wire) or position+sender_tid (inline).
                        if let Some(did) = delivery_id {
                            if let Some(p) = t
                                .pending_msgs
                                .iter()
                                .position(|m| m.wire_header().is_some_and(|h| h.delivery_id == did))
                            {
                                t.pending_msgs.remove(p);
                            }
                        } else if pos < t.pending_msgs.len()
                            && t.pending_msgs.as_slice()[pos].sender_tid == sender_tid
                        {
                            t.pending_msgs.remove(pos);
                        }
                        t.set_received_caller_context(
                            sender_tid,
                            sender_cell_id,
                            sender_generation,
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
                        write_caller_identity(buf_ptr, buf_len, id);
                    }
                    Ok(id)
                }
                Err(_) => Err(SyscallError::InvalidCommand),
            }
        }
        Syscall::Spawn { entry, arg } => {
            let drivers = alloc::vec::Vec::new();
            let name = "thread";
            // A spawned thread is the same cell running more TIDs: it inherits the
            // parent cell's identity on every axis the kernel gates — CellId (so its
            // allocations charge the parent's quota, not the unlimited CellId(0) slot),
            // the transferable CapSet, the syscall allowlist, and the PKU protection
            // domain. Singleton caps (supervisor/pcie_driver/platform) are deliberately
            // NOT propagated: they carry a one-holder invariant.
            //
            // Snapshot the parent identity under the lock, then DROP it before
            // spawn_with_arg (which re-locks SCHEDULER — Spinlock is not reentrant),
            // then re-lock to apply. Mirrors the CellId fix-up on the cell-spawn path
            // (loader.rs:174-186). Fail-safe: an unresolved caller DENIES the spawn —
            // it must never fall back to CellId(0), which is exactly the quota-escape
            // this closes.
            let (parent_cell_id, parent_caps, parent_allowlist, parent_pku) = {
                let mut sched_opt = super::SCHEDULER.lock();
                let sched = match sched_opt.as_mut() {
                    Some(s) => s,
                    None => return Err(SyscallError::Unknown),
                };
                match sched.tasks.get(&caller_id) {
                    Some(t) => (
                        t.cell_id,
                        super::cap::CapSet::of_task(t),
                        t.syscall_allowlist,
                        (t.pku_key, t.pku_value),
                    ),
                    None => return Err(SyscallError::Unknown),
                }
            };

            // A refused thread spawn is `TryAgain`, not a fault: both the per-cell
            // thread cap and a fragmented allocator surface as OutOfMemory, and
            // both must leave the caller running. This path used to reach an
            // `.expect` in the scheduler, so an unprivileged cell looping here
            // could panic the kernel — never-die broken from userspace.
            let tid = match super::spawn_with_arg(name, parent_cell_id, drivers, entry, arg) {
                Ok(t) => t,
                Err(ViError::OutOfMemory) => return Err(SyscallError::TryAgain),
                Err(_) => return Err(SyscallError::Unknown),
            };

            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(t) = sched.tasks.get_mut(&tid) {
                    parent_caps.apply_to(t);
                    t.syscall_allowlist = parent_allowlist;
                    t.pku_key = parent_pku.0;
                    t.pku_value = parent_pku.1;
                }
            }
            Ok(tid)
        }
        Syscall::Wait { pid } => {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(target) = sched.tasks.get_mut(&pid) {
                    if matches!(target.state, TaskState::Terminated | TaskState::Retiring) {
                        // A retiring root-generation record is terminal even
                        // while it remains dispatch-visible for remote
                        // quiescence.
                        let code = target.exit_code.unwrap_or(0);
                        return Ok(code);
                    } else {
                        // Add to waiters
                        target.waiters.push(caller_id);
                    }
                } else {
                    return Err(SyscallError::InvalidDriverId); // Task not found
                }

                // Block caller
                if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                    caller.state = TaskState::Waiting { target: pid };
                }
            }
            super::yield_cpu(); // Block
                                // Resume with exit code (set by Exit handler)
            if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                return Ok(sched
                    .tasks
                    .get(&caller_id)
                    .and_then(|t| t.reply_value)
                    .unwrap_or(0));
            }
            Ok(0)
        }
        Syscall::ShmAlloc { size: _ } => {
            // Allocate a single frame from the global allocator and register
            // it in the SHM handle table so subsequent ShmMap calls can
            // verify the caller isn't forging an arbitrary physical address.
            let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
            if let Some(allocator) = frame_guard.as_mut() {
                if let Some(frame) = allocator.allocate_frame() {
                    drop(frame_guard);
                    shm_register(frame);
                    return Ok(frame);
                }
            }
            Err(SyscallError::BufferTooSmall)
        }
        Syscall::ShmMap {
            handle,
            target_pid: _,
        } => {
            // CRITICAL: handle must be a frame previously issued by ShmAlloc.
            // Without this check, a cell could pass `handle = kernel_text_phys`
            // and obtain a user-accessible mapping to kernel code.
            if !shm_is_valid(handle) {
                return Err(SyscallError::PermissionDenied);
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
        Syscall::FutexWait { addr, val } => {
            // Returns Ok(0) if blocked (then yield), Err(TryAgain) if val mismatch
            match super::futex_wait(caller_id, addr, val) {
                Ok(_) => {
                    super::yield_cpu(); // Block
                    Ok(0)
                }
                Err(_) => Err(SyscallError::TryAgain),
            }
        }
        Syscall::FutexWake { addr, count } => {
            if let Ok(n) = super::futex_wake(caller_id, addr, count) {
                Ok(n)
            } else {
                Err(SyscallError::Unknown) // Should not fail typically
            }
        }
        Syscall::Log { msg_ptr, msg_len } => {
            if let Ok(msg) = read_user_string(caller_id, msg_ptr, msg_len, MAX_LOG_MSG) {
                crate::task::print_user_log(&msg);
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
                let has_spawn = sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.spawn_cap.is_some())
                    .unwrap_or(false);
                if !has_spawn {
                    return Err(SyscallError::PermissionDenied);
                }

                // Gate 2: protect system service cells (VFS=block_io_cap, net=network_cap).
                // Killing them mid-I/O leaves driver state inconsistent; use hot-swap instead.
                let target_is_system = sched
                    .tasks
                    .get(&tid)
                    .map(|t| t.block_io_cap.is_some() || t.network_cap.is_some())
                    .unwrap_or(false);
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

            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                // Gate 1: caller must hold SpawnCap (same authority as ForceExit).
                let has_spawn = sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.spawn_cap.is_some())
                    .unwrap_or(false);
                if !has_spawn {
                    return Err(SyscallError::PermissionDenied);
                }

                // Gate 1b: refuse bits whose authority is AMBIENT — already handed out
                // and exercised WITHOUT a per-use syscall re-check, so clearing the TCB
                // field does not actually revoke it. HYPERVISOR (H-ext CSR access) and
                // MMIO device windows (mapped into the cell's page tables) both persist
                // after the field is cleared: the cell keeps poking the hardware. Until
                // the eager teardown lands (unmap_dma + IOTLB flush, MMIO page-table
                // unmap — .agents/260712-1901 P01-P05), revoking these would be a lie.
                // Refuse them so the shipped syscall is honest. block_io/network are
                // refused by Gate 2 below; SPAWN and BLKREGION are re-checked at each
                // use (lazy revocation is sound for them).
                const AMBIENT_UNTIL_TEARDOWN: u32 = CM::HYPERVISOR | CM::MMIO_MASK;
                if cap_mask & AMBIENT_UNTIL_TEARDOWN != 0 {
                    crate::audit::log_event(
                        crate::audit::AuditEvent::CapRevoked,
                        &crate::audit::encode_u32x2(target_tid as u32, cap_mask),
                    );
                    log::warn!(
                        "[kernel] CapRevoke: refused ambient-authority bits \
                        mask={:#010x} from task {} (teardown not implemented)",
                        cap_mask,
                        caller_id
                    );
                    return Err(SyscallError::NotSupported);
                }

                // Gate 2: protect system service cells — revoking I/O caps from a
                // running VFS/net service mid-flight corrupts driver state. Use
                // the supervisor freeze/pause/replacement flow instead.
                let target_is_system = sched
                    .tasks
                    .get(&target_tid)
                    .map(|t| t.block_io_cap.is_some() || t.network_cap.is_some())
                    .unwrap_or(false);
                if target_is_system {
                    return Err(SyscallError::PermissionDenied);
                }

                let task = match sched.tasks.get_mut(&target_tid) {
                    Some(t) => t,
                    None => return Err(SyscallError::InvalidCommand),
                };

                // Apply revocation — clear each indicated cap field.
                if cap_mask & CM::BLOCK_IO != 0 {
                    task.block_io_cap = None;
                }
                if cap_mask & CM::NETWORK != 0 {
                    task.network_cap = None;
                }
                if cap_mask & CM::SPAWN != 0 {
                    task.spawn_cap = None;
                }
                if cap_mask & CM::HYPERVISOR != 0 {
                    task.hypervisor_cap = None;
                }

                // Parameterised sub-fields: clear the indicated bits, preserving the rest.
                let mmio_revoke = ((cap_mask >> CM::MMIO_SHIFT) & 0xFF) as u8;
                task.mmio_devices &= !mmio_revoke;
                let blk_revoke = ((cap_mask >> CM::BLKREGION_SHIFT) & 0xFF) as u8;
                task.block_regions &= !blk_revoke;
            } else {
                return Err(SyscallError::InvalidCommand);
            }

            crate::audit::log_event(
                crate::audit::AuditEvent::CapRevoked,
                &crate::audit::encode_u32x2(target_tid as u32, cap_mask),
            );
            log::info!(
                "[kernel] CapRevoke: task {} revoked mask={:#010x} from task {}",
                caller_id,
                cap_mask,
                target_tid
            );

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
                let has_spawn = sched
                    .tasks
                    .get(&caller_id)
                    .is_some_and(|task| task.spawn_cap.is_some());
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
                        wt.pending_deaths.push((watched, 0));
                    }
                }
            }
            Ok(0)
        }

        Syscall::RegisterService { service_id, tid } => {
            // Development Silo readiness is self-published only after artifact,
            // VM, entropy, guest READY, and public-key validation. The dedicated
            // non-delegable cap is minted solely for the governed `/bin/silo`
            // root task in test-hooks kernels; HypervisorCap alone is insufficient.
            #[cfg(feature = "test-hooks")]
            if service_id == api::syscall::service::SILO
                && tid == 0
                && caller_has_development_silo_registration(caller_id)
            {
                return if crate::cell::service_registry::register(service_id, caller_id) {
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
                crate::cell::service_registry::register(service_id, caller_id);
                return Ok(0);
            }
            // Driver Cell self-registration path: a PCIe GPU driver or the
            // display-only BCM mailbox driver may register itself with tid=0.
            if (caller_has_pcie_driver(caller_id)
                || caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY))
                && tid == 0
                && service_id == api::syscall::service::GPU_DRIVER
            {
                crate::task::drivers::driver_cell::register_gpu_driver(caller_id);
                crate::cell::service_registry::register(service_id, caller_id);
                return Ok(0);
            }
            // Privileged: only SpawnCap holders (the supervisor) own the service
            // namespace — same authority gate as NotifyOnExit/ForceExit. Prevents a
            // cell from hijacking a well-known endpoint (e.g. the VFS service).
            if !caller_has_spawn(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            let tid_is_live = super::SCHEDULER.lock().as_ref().is_some_and(|scheduler| {
                scheduler.tasks.get(&tid).is_some_and(|task| {
                    !matches!(task.state, TaskState::Retiring | TaskState::Terminated)
                })
            });
            if !tid_is_live {
                return Err(SyscallError::InvalidInput);
            }
            if crate::cell::service_registry::register(service_id, tid) {
                Ok(0)
            } else {
                Err(SyscallError::InvalidInput)
            }
        }
        Syscall::LookupService { service_id } => {
            // Open to all cells: resolve the live provider tid (0 = none registered),
            // so a client reconnects transparently after the supervisor respawns a
            // service. The dynamic replacement for the boot-order `ServiceLookup` hardcode.
            Ok(crate::cell::service_registry::lookup(service_id).unwrap_or(0))
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
        Syscall::Write {
            fd,
            buf_ptr,
            buf_len,
        } => {
            let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
            let written = super::file_write(fd, &bytes);
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
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            // A successful spawn consumed any staged directory-handle set inside
            // task creation. Clearing unconditionally covers the failure paths:
            // a set left staged by a spawn that never produced a child would be
            // handed to whichever child this caller created next.
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| match e {
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
            if len == 0 {
                return Err(SyscallError::InvalidInput);
            }
            let base = {
                let guard = grant_table_lock().lock();
                let table = guard.as_ref().ok_or(SyscallError::InvalidInput)?;
                let g = table.get(&grant_id).ok_or(SyscallError::InvalidInput)?;
                if g.owner != caller_id {
                    return Err(SyscallError::PermissionDenied);
                }
                if len > g.size {
                    return Err(SyscallError::InvalidInput);
                }
                g.base
            };
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
                crate::loader::launch_profile::LaunchRoute::Elf,
                &path_str,
            )?;
            log::info!(
                "[loader] SpawnFromElf: {} ({} bytes from grant)",
                path_str,
                len
            );
            let elf_bytes = unsafe { core::slice::from_raw_parts(base as *const u8, len) };
            let request = match governed_spawn_request(
                caller_id,
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
            ) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            let spawned = crate::loader::spawn_gated(elf_bytes, &path_str, request);
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
            if !path_str.starts_with('/') {
                return Err(SyscallError::InvalidInput);
            }
            let profile = authorize_launch_edge(
                caller_id,
                crate::loader::launch_profile::LaunchRoute::Pinned,
                &path_str,
            )?;
            let request = match governed_spawn_request(caller_id, profile.child_ceiling, priority) {
                Ok(request) => request,
                Err(error) => {
                    crate::task::dir_inherit::clear_staged(caller_id);
                    return Err(error);
                }
            };
            let spawned = crate::loader::spawn_from_path(&path_str, request);
            crate::task::dir_inherit::clear_staged(caller_id);
            let task_id = spawned.map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => {
                    log::warn!(
                        "[loader] spawn OOM: op=SpawnPinned caller={} path={}",
                        caller_id,
                        path_str
                    );
                    SyscallError::OutOfMemory
                }
                _ => SyscallError::InvalidInput,
            })?;
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
                    .open(&path_str, api::fs::OpenMode::ReadWrite)
                    .map_err(|_| SyscallError::FileNotFound)?
            };
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let cap_id = crate::cell::cap_registry::CAP_TABLE.lock().alloc(
                cell_id,
                crate::cell::cap_registry::CapResource::File { file: Some(file) },
                api::cap::CapPerms::FILE_RW.0,
            );
            Ok(cap_id.0 as usize)
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
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            let read_result = boxed_file.read(&mut kbuf);
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
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
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let seek_result = boxed_file.seek(pos);
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
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
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let write_result = boxed_file.write(&bytes);
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
            match write_result {
                Ok(n) => Ok(n),
                Err(_) => Err(SyscallError::Unknown),
            }
        }

        Syscall::StatCap { cap_id } => {
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let result = boxed_file.size();
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
            result
                .map(|s| s as usize)
                .map_err(|_| SyscallError::Unknown)
        }

        Syscall::TruncateCap { cap_id, len } => {
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let result = boxed_file.truncate(len as u64);
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::SyncCap { cap_id } => {
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut boxed_file = crate::cell::cap_registry::CAP_TABLE
                .lock()
                .park_file(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            let result = boxed_file.sync();
            crate::cell::cap_registry::CAP_TABLE
                .lock()
                .unpark_file(crate::cell::cap_registry::CapId(cap_id as u64), boxed_file);
            result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
        }

        Syscall::GrantDma { bdf, phys, size } => {
            if phys & 0xFFF != 0 || size & 0xFFF != 0 || size == 0 {
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
            let Some(iova) =
                super::drivers::iommu::map_dma_for_cell(caller_id as u64, bdf, phys, size)
            else {
                let rolled_back = crate::memory::pin::rollback_pin(phys_base, size, caller_id);
                debug_assert!(rolled_back, "failed DMA map must release its exact pin");
                drop(publication);
                log::warn!("[iommu] Cell {caller_id} DMA grant denied: no active mapping backend");
                return Err(SyscallError::Unknown);
            };
            super::drivers::pcie_ecam::enable_bus_master(bdf);
            publication.commit();
            log::info!(
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
            let cell_id = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&caller_id)
                    .map(|t| t.cell_id)
                    .unwrap_or(types::CellId(0))
            } else {
                types::CellId(0)
            };
            let mut table = crate::cell::cap_registry::CAP_TABLE.lock();
            table
                .verify(crate::cell::cap_registry::CapId(cap_id as u64), cell_id)
                .map_err(|_| SyscallError::PermissionDenied)?;
            table.revoke(crate::cell::cap_registry::CapId(cap_id as u64));
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
                profile.child_ceiling,
                api::TaskPriority::Normal as u8,
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
                    let t = hal::hpet::now_ns() as usize;
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
                    // 62.5 MHz CNTPCT on QEMU ARM64 virt → 62_500 ticks/ms
                    #[cfg(target_arch = "aarch64")]
                    let ms = (hal::timer::read_ticks() / 62_500) as usize;
                    // HPET already returns nanoseconds; ÷ 1_000_000 → ms
                    #[cfg(target_arch = "x86_64")]
                    let ms = (hal::hpet::now_ns() / 1_000_000) as usize;
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
                // Unknown op — return 0 for backward compatibility
                _ => Ok(0),
            }
        }
        Syscall::AudioPlay { .. } => {
            // No Sound Cell registered; virtio_sound.rs removed (kernel boundary law).
            // Syscall 218 is reserved — do not reuse. A VirtIO Sound Driver Cell
            // will handle audio in G2 via direct MMIO + IPC, no kernel driver needed.
            log::warn!("[audio] AudioPlay: no Sound Cell registered (G2 TODO)");
            Err(SyscallError::Unknown)
        }
        Syscall::GpuFlush {
            data_ptr,
            data_len,
            xy,
            wh,
        } => {
            // If a GPU Driver Cell is registered, forward the flush via fire-and-forget IPC.
            // The Cell owns the VirtIO GPU hardware; we copy nothing — the Cell reads
            // data_ptr directly from the SAS (single address space).
            {
                use crate::task::drivers::driver_cell::GPU_DRIVER_CELL;
                use core::sync::atomic::Ordering;
                let gpu_cell = GPU_DRIVER_CELL.load(Ordering::Acquire);
                if gpu_cell != 0 {
                    // AppContext message envelope [0xAC, 0x00] followed by GPU payload.
                    let mut msg = [0u8; 23];
                    msg[0] = 0xAC;
                    msg[1] = 0x00;
                    msg[2] = 0x10; // OP_FLUSH
                    msg[3..7].copy_from_slice(&(xy as u32).to_le_bytes());
                    msg[7..11].copy_from_slice(&(wh as u32).to_le_bytes());
                    msg[11..19].copy_from_slice(&(data_ptr as u64).to_le_bytes());
                    msg[19..23].copy_from_slice(&(data_len as u32).to_le_bytes());
                    let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
                    return Ok(0);
                }
            }
            // Warn only once — compositor calls this every frame tick.
            {
                use core::sync::atomic::{AtomicBool, Ordering};
                static WARNED: AtomicBool = AtomicBool::new(false);
                if !WARNED.swap(true, Ordering::Relaxed) {
                    log::warn!(
                        "[gpu_flush] GPU Driver Cell not registered — software fallback active"
                    );
                }
            }
            Err(SyscallError::Unknown)
        }
        Syscall::GpuCursor {
            op,
            data_ptr,
            xy,
            hot,
        } => {
            // If a GPU Driver Cell is registered, forward the cursor op via fire-and-forget IPC.
            {
                use crate::task::drivers::driver_cell::GPU_DRIVER_CELL;
                use core::sync::atomic::Ordering;
                let gpu_cell = GPU_DRIVER_CELL.load(Ordering::Acquire);
                if gpu_cell != 0 {
                    match op {
                        0 => {
                            // AppContext envelope + OP_CUR_SET payload.
                            let mut msg = [0u8; 19];
                            msg[0] = 0xAC;
                            msg[1] = 0x00;
                            msg[2] = 0x11;
                            msg[3..11].copy_from_slice(&(data_ptr as u64).to_le_bytes());
                            msg[11..15].copy_from_slice(&(xy as u32).to_le_bytes());
                            msg[15..19].copy_from_slice(&(hot as u32).to_le_bytes());
                            let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
                        }
                        1 => {
                            // AppContext envelope + OP_CUR_MOVE payload.
                            let mut msg = [0u8; 7];
                            msg[0] = 0xAC;
                            msg[1] = 0x00;
                            msg[2] = 0x12;
                            msg[3..7].copy_from_slice(&(xy as u32).to_le_bytes());
                            let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
                        }
                        _ => return Err(SyscallError::InvalidInput),
                    }
                    return Ok(0);
                }
            }
            // Warn only once.
            {
                use core::sync::atomic::{AtomicBool, Ordering};
                static WARNED: AtomicBool = AtomicBool::new(false);
                if !WARNED.swap(true, Ordering::Relaxed) {
                    log::warn!("[gpu_cursor] GPU Driver Cell not registered");
                }
            }
            Err(SyscallError::Unknown)
        }
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
            let is_shell =
                caller_launch_state(caller_id).is_some_and(|(name, _, _)| name.as_str() == "shell");
            if is_shell && (raw_key != SPAWN_ARGV_KEY || buf_len > SPAWN_ARGV_MAX) {
                return Err(SyscallError::PermissionDenied);
            }
            let stash_key = if raw_key == SPAWN_ARGV_KEY {
                spawn_argv_slot(caller_id)
            } else {
                raw_key
            };
            let bytes = read_user_slice(
                caller_id,
                buf_ptr,
                buf_len,
                crate::cell::state_stash::MAX_STASH_LEN,
            )?;
            Ok(crate::cell::state_stash::stash(stash_key, &bytes))
        }
        Syscall::StateRestore {
            key,
            buf_ptr,
            buf_len,
        } => {
            validate_user_buf(buf_ptr, buf_len, crate::cell::state_stash::MAX_STASH_LEN)?;
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(buf_len)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(buf_len, 0);
            if key as u64 == SPAWN_ARGV_KEY {
                let personal_key = spawn_argv_slot(caller_id);
                let n = crate::cell::state_stash::restore(personal_key, &mut kbuf);
                if n > 0 {
                    crate::cell::state_stash::remove(personal_key);
                    write_user_slice(
                        caller_id,
                        buf_ptr,
                        &kbuf[..n],
                        crate::cell::state_stash::MAX_STASH_LEN,
                    )?;
                }
                return Ok(n);
            }
            let n = crate::cell::state_stash::restore(key as u64, &mut kbuf);
            if n > 0 {
                write_user_slice(
                    caller_id,
                    buf_ptr,
                    &kbuf[..n],
                    crate::cell::state_stash::MAX_STASH_LEN,
                )?;
            }
            Ok(n)
        }
        // 412: StateStashClear — delete the stash entry for `key`, freeing its slot.
        // No-op when the key is absent (idempotent). Returns 0 always.
        Syscall::StateStashClear { key } => {
            let raw_key = key as u64;
            let stash_key = if raw_key == SPAWN_ARGV_KEY {
                spawn_argv_slot(caller_id)
            } else {
                raw_key
            };
            crate::cell::state_stash::remove(stash_key);
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
            Ok(task_id)
        }

        // ── Driver Cell Registration (P00) ───────────────────────────────────

        // 416: RegisterBlockDriver — announce caller as the active block device driver.
        // Stores tid in BLOCK_DRIVER_CELL and registers under service::BLOCK_DRIVER
        // so VFS can resolve it via LookupService.
        Syscall::RegisterBlockDriver => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::task::drivers::driver_cell::register_block_driver(caller_id);
            // Bypass the SpawnCap gate — PcieDriverCap is the authority for driver
            // namespace registration. Direct write into service registry.
            crate::cell::service_registry::register(api::syscall::service::BLOCK_DRIVER, caller_id);
            Ok(0)
        }

        // 417: RegisterNicDriver — announce caller as the active NIC driver.
        Syscall::RegisterNicDriver => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::task::drivers::driver_cell::register_nic_driver(caller_id);
            crate::cell::service_registry::register(api::syscall::service::NIC_DRIVER, caller_id);
            Ok(0)
        }

        // 418: FindPcieDevice — query ECAM table for a device by class triple.
        // Writes a `PcieDeviceInfo` record to `out_ptr` and returns 1 if found.
        // Requires PcieDriverCap; also records BDF ownership in resource_registry.
        Syscall::FindPcieDevice {
            class,
            subclass,
            prog_if,
            out_ptr,
        } => {
            if !caller_has_pcie_driver(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            match crate::task::drivers::pcie_ecam::find_class(class, subclass, prog_if) {
                None => Ok(0), // device not present → VirtIO fallback
                Some(dev) => {
                    let bdf: u32 =
                        (dev.bdf.0 as u32) << 8 | (dev.bdf.1 as u32) << 3 | (dev.bdf.2 as u32);
                    let bar0_base = dev.bars[0].base_addr();
                    let bar0_len: u64 = match dev.bars[0] {
                        crate::task::drivers::pcie_ecam::Bar::Memory32 { size, .. } => size as u64,
                        crate::task::drivers::pcie_ecam::Bar::Memory64 { size, .. } => size,
                        _ => 0x4000, // fallback 16 KiB
                    };
                    // Record BDF → caller ownership for IOMMU gate.
                    crate::resource_registry::register_bdf_owner(bdf, caller_id);
                    // Write the 20-byte PcieDeviceInfo to the cell's out_ptr.
                    // SAFETY: SAS — caller's virtual address == kernel's virtual address.
                    // The cell is responsible for passing a valid, writeable pointer.
                    if out_ptr != 0 {
                        let mut dev_bytes = [0u8; 24];
                        dev_bytes[0..4].copy_from_slice(&bdf.to_ne_bytes());
                        dev_bytes[4..8].copy_from_slice(&1u32.to_ne_bytes());
                        dev_bytes[8..16].copy_from_slice(&bar0_base.to_ne_bytes());
                        dev_bytes[16..24].copy_from_slice(&bar0_len.to_ne_bytes());
                        write_user_slice(caller_id, out_ptr, &dev_bytes, MAX_USER_BUF)?;
                    }
                    Ok(1)
                }
            }
        }

        // 234: WaitIrq — block until hardware IRQ fires (Driver Cell).
        // ISR calls irq_wait::signal_irq (atomic only; no lock, no scheduler access).
        // Scheduler sweep (pick_next) does the actual Ready transition.
        Syscall::WaitIrq { irq, mmio_base } => {
            if !caller_has_pcie_driver(caller_id) && !caller_has_platform(caller_id) {
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
        // Populates PCIE_BARS allowlist (resource_registry) so Driver Cells can
        // claim the BAR via sys_request_mmio. Records BDF → caller ownership.
        Syscall::RegisterPcieBar { bdf, base, len } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::resource_registry::register_pcie_bar(base, len);
            crate::resource_registry::register_bdf_owner(bdf, caller_id);
            // Any RegisterPcieBar call from the Platform Cell marks it as active;
            // Phase 08 uses this to gate out the kernel ECAM scan.
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            Ok(0)
        }

        // 236: RegisterPciDevice — Platform Cell announces a device with class/BAR info.
        // Populates PCI_DEVICES so sys_find_pcie_device queries work without kernel ECAM scan.
        // After each registration, attempt deferred IOMMU init (no-op until IOMMU device appears).
        Syscall::RegisterPciDevice {
            bdf,
            cls,
            bar0_base,
            bar0_size,
        } => {
            if !caller_has_platform(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            crate::task::drivers::pcie_ecam::register_device(bdf, cls, bar0_base, bar0_size);
            crate::task::drivers::pcie_ecam::mark_platform_registered();
            crate::task::drivers::iommu::try_deferred_init();
            Ok(0)
        }

        // 237: ReadLog — drain the user-log ring buffer into a cell-provided buffer.
        // The caller must have allowlist bit 54 (ReadLog) set in its manifest.
        // Returns the number of bytes actually copied (0 when nothing available).
        Syscall::ReadLog { buf_ptr, max } => {
            if max == 0 {
                return Ok(0);
            }
            if buf_ptr == 0 {
                return Err(SyscallError::InvalidInput);
            }
            let max = max.min(4096);
            let mut kbuf = alloc::vec::Vec::new();
            kbuf.try_reserve_exact(max)
                .map_err(|_| SyscallError::OutOfMemory)?;
            kbuf.resize(max, 0);
            let n = crate::task::read_log_ring(&mut kbuf);
            if n > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
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
            let n_pages = size.div_ceil(PAGE_SIZE);
            let paddr = match alloc_grant_pages(n_pages) {
                Some(paddr) => paddr,
                None => return Ok(0),
            };
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
            table.as_mut().unwrap().insert(
                paddr,
                PageGrant {
                    base: paddr,
                    size,
                    owner: caller_id,
                    owner_cell,
                    owner_generation,
                    shared_to: None,
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
        } => {
            let vfs_context = match current_vfs_grant_lookup(caller_id) {
                VfsGrantLookup::NotVfs => None,
                VfsGrantLookup::MissingContext => return Ok(usize::MAX),
                VfsGrantLookup::Active(context) if context.pending_revoke => return Ok(usize::MAX),
                VfsGrantLookup::Active(context) => Some(context),
            };
            let Some(access) =
                resolve_and_lease_grant(caller_id, grant_id, size_out_ptr, vfs_context)?
            else {
                return Ok(usize::MAX);
            };
            super::user_out::write_resolved_optional_usize(access.size_out, access.size);
            Ok(access.base)
        }

        Syscall::GrantFree { grant_id } => {
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
                    .map(|g| (g.base, g.size));
                match owned {
                    None => None,
                    Some((base, size)) => {
                        refuse_if_pinned("GrantFree", grant_id, base, size)?;
                        tbl.as_mut().and_then(|m| m.remove(&grant_id))
                    }
                }
            };
            let entry = match entry {
                Some(e) => e,
                None => return Err(SyscallError::PermissionDenied),
            };
            free_grant_pages(entry.base, grant_pages_for_size(entry.size));
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
            #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
            {
                const MAX_DIMENSION: usize = 8192;
                const PAGE_SIZE: usize = 4096;
                if !caller_has_mmio_device(caller_id, crate::resource_registry::DEV_DISPLAY) {
                    return Err(SyscallError::PermissionDenied);
                }
                let mmio = hal_soc_bcm27xx::BCM2837.mmio;
                if !crate::resource_registry::owns_exact_mmio(
                    types::CellId(caller_id as u64),
                    mmio.mailbox_base,
                    mmio.mailbox_grant_size,
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
                let width = (packed_dimensions >> 16) as usize;
                let height = (packed_dimensions & 0xffff) as usize;
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
                if covered > size || end > mmio.peripheral_base {
                    return Err(SyscallError::InvalidInput);
                }
                let allocator_end = crate::memory::frame::FRAME_ALLOCATOR
                    .lock()
                    .as_ref()
                    .map(|allocator| allocator.memory_end())
                    .ok_or(SyscallError::Unknown)?;
                if base < allocator_end {
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
                    Ok(0)
                } else {
                    // This cannot be recovered safely: pages are firmware-owned
                    // and remain mapped until reboot, so display stays fail-stop.
                    Err(SyscallError::Unknown)
                }
            }
            #[cfg(not(all(target_arch = "aarch64", feature = "board-rpi3")))]
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
            let n_pages = size.div_ceil(PAGE_SIZE);
            let paddr = match alloc_grant_pages(n_pages) {
                Some(paddr) => paddr,
                None => return Ok(0),
            };
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
            table.as_mut().unwrap().insert(
                paddr,
                RegGrant {
                    base: paddr,
                    size,
                    owner: caller_id,
                    owner_cell,
                    owner_generation,
                    shared_to: None,
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
            // PlatformCap bypass: Platform Cell may claim any MMIO range
            // (including the ECAM config-space window, which is not in PCIE_BARS).
            // Overlap check still applies — no two cells may share a byte.
            // x86 SAS: the boot identity map covers MMIO kernel-only; a granted
            // window must gain PTE_USER (+PCD, NX) before the ring-3 cell touches
            // it. BAR windows may not be mapped at all yet — this also creates
            // the identity mapping. riscv/aarch64 map cell MMIO user at boot.
            #[cfg(target_arch = "x86_64")]
            fn user_map(base: usize, len: usize) {
                crate::memory::paging::map_mmio_user_x86(base, len);
            }
            #[cfg(not(target_arch = "x86_64"))]
            fn user_map(_base: usize, _len: usize) {}

            if caller_has_platform(caller_id) {
                return match crate::resource_registry::request_mmio_unchecked(
                    types::CellId(caller_id as u64),
                    base,
                    len,
                ) {
                    Ok(()) => {
                        user_map(base, len);
                        Ok(0)
                    }
                    Err(types::ViError::AlreadyExists) => Ok(2),
                    Err(_) => Ok(3),
                };
            }
            // PCIe BAR path: cells with PcieDriverCap may claim any BAR
            // discovered during ECAM scan (registered in resource_registry::PCIE_BARS).
            if caller_has_pcie_driver(caller_id) && crate::resource_registry::is_pcie_bar(base, len)
            {
                return match crate::resource_registry::request_mmio(
                    types::CellId(caller_id as u64),
                    base,
                    len,
                    crate::resource_registry::DEV_PCIE,
                ) {
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
                    .and_then(|s| s.tasks.get(&caller_id))
                    .map(|t| t.mmio_devices)
                    .unwrap_or(0)
            };
            if allowed_devices == 0 {
                return Err(SyscallError::PermissionDenied);
            }
            match crate::resource_registry::request_mmio(
                types::CellId(caller_id as u64),
                base,
                len,
                allowed_devices,
            ) {
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
                #[cfg(feature = "dev-weak-rng")]
                {
                    static WEAK_RNG_WARNED: core::sync::atomic::AtomicBool =
                        core::sync::atomic::AtomicBool::new(false);
                    if !WEAK_RNG_WARNED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                        log::warn!(
                            "[kernel] GetRandom: no VirtIO-RNG — serving WEAK xorshift32 \
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
                #[cfg(not(feature = "dev-weak-rng"))]
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
            validate_user_buf(
                out_ptr,
                core::mem::size_of::<api::hypervisor::ViVmExit>(),
                MAX_USER_BUF,
            )?;
            // SAFETY: pointer validated above; SAS means it's also valid in kernel.
            let exit_out = out_ptr as *mut api::hypervisor::ViVmExit;
            unsafe {
                crate::hypervisor::registry::run_vcpu(
                    caller_id, vm_id, vcpu_id, budget_ns, exit_out,
                )
            }
            .map_err(|_| SyscallError::NotSupported)
        }

        Syscall::VcpuRegs {
            vm_id,
            vcpu_id,
            buf_ptr,
            write,
        } => {
            if !caller_has_hypervisor(caller_id) {
                return Err(SyscallError::PermissionDenied);
            }
            // 32 registers × 8 bytes = 256 bytes.
            validate_user_buf(buf_ptr, 256, MAX_USER_BUF)?;
            crate::hypervisor::registry::vcpu_regs(caller_id, vm_id, vcpu_id, buf_ptr, write)
                .map_err(|_| SyscallError::NotSupported)
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
            // Overflow guard: gpa+len and src_ptr+len must not wrap.
            gpa.checked_add(len as u64)
                .ok_or(SyscallError::InvalidInput)?;
            validate_user_buf(src_ptr, len, MAX_USER_BUF)?;
            crate::hypervisor::registry::write_guest_memory(caller_id, vm_id, gpa, src_ptr, len)
                .map_err(|_| SyscallError::InvalidInput)
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
            gpa.checked_add(len as u64)
                .ok_or(SyscallError::InvalidInput)?;
            validate_user_buf(dst_ptr, len, MAX_USER_BUF)?;
            crate::hypervisor::registry::read_guest_memory(caller_id, vm_id, gpa, dst_ptr, len)
                .map_err(|_| SyscallError::InvalidInput)
        }
    }
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
        ViSyscall::RegisterBlockDriver => Syscall::RegisterBlockDriver,
        ViSyscall::RegisterNicDriver => Syscall::RegisterNicDriver,
        ViSyscall::FindPcieDevice => Syscall::FindPcieDevice {
            class: a0 as u8,
            subclass: a1 as u8,
            prog_if: a2 as u8,
            out_ptr: a3,
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
            let mask = a0 as u32;
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
            write: a3 != 0,
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
        },
        ViSyscall::Chdir => Syscall::ChDir {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::Getcwd => Syscall::GetCwd {
            buf_ptr: a0,
            buf_len: a1,
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
/// `Err(SyscallError::PermissionDenied)` if the opcode's bit is not set.
/// The SCHEDULER lock is acquired and released here — callers must NOT hold it.
fn check_allowlist(syscall_id: usize, caller_id: usize) -> Result<(), SyscallError> {
    let sc = ViSyscall::from(syscall_id);
    let bit = sc.allowlist_bit();
    // Bit 36 gates raw block-I/O opcodes (500/501/503) and BlkReadAsync (212).
    let blk_io_bit: Option<u8> = if matches!(syscall_id, 500 | 501 | 503 | 212) {
        Some(36)
    } else {
        None
    };

    // Raw opcodes with a dedicated `map_syscall` fallback mapping. These are
    // intentionally absent from `ViSyscall` (Law 1: keeps experimental ids out
    // of the stable ABI) so they decode as `Unknown` — but they are NOT unknown:
    // 500/501/503 are gated by bit 36 below + the ZST BlockIoCap at the handler;
    // 502 and the legacy raw ops (3/100/110/111) predate the bitmap and stay
    // always-permitted, matching their pre-Phase-31b behavior.
    let known_raw = matches!(syscall_id, 3 | 100 | 110 | 111 | 500 | 501 | 502 | 503);

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
    if let Some(b) = bit {
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
    Ok(())
}

#[cfg(not(target_arch = "riscv32"))]
#[no_mangle]
#[allow(non_snake_case)] // ABI name required by the HAL trap vector — cannot be snake_case
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame) {
    #[cfg(all(feature = "x86-idt-cpl3-test", target_arch = "x86_64"))]
    if crate::hal::idt::handle_cpl3_probe_syscall(frame) {
        return;
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

    let supports_typed_oom = supports_typed_spawn_oom(&syscall);
    let result = handle_syscall(caller_id, syscall);

    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

    frame.regs[10] = encode_syscall_result(result, usize::MAX, supports_typed_oom);
}

#[cfg(not(target_arch = "riscv32"))]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(target_arch = "riscv32")]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "Rust" fn ViCell_syscall_dispatch(frame: &mut crate::hal::arch::ViTrapFrame) {
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

    let supports_typed_oom = supports_typed_spawn_oom(&syscall);
    let result = handle_syscall(caller_id, syscall);

    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
    }

    frame.regs[10] = encode_syscall_result(result, u32::MAX as usize, supports_typed_oom) as u32;
}

#[cfg(target_arch = "riscv32")]
const _: crate::hal::SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(test)]
mod tests {
    use super::{
        check_allowlist, encode_syscall_result, map_syscall, page_grant_authorizes_dma,
        reg_grant_authorizes_dma, supports_typed_spawn_oom, syscall_to_vi, withhold_or_free,
        PageGrant, RegGrant, Syscall, SyscallError,
    };
    use crate::task::{scheduler::Scheduler, tcb::Task, SCHEDULER};
    use api::syscall::ViSyscall;
    use types::CellId;

    fn with_scheduler_task<F>(allowlist: u64, f: F)
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
        scheduler.tasks.insert(tid, task);
        *previous = Some(scheduler);
        drop(previous);

        f(tid);

        let mut restore = SCHEDULER.lock();
        *restore = saved;
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
        assert!(supports_typed_spawn_oom(&Syscall::SpawnFromPath {
            path_ptr: 0,
            path_len: 0,
        }));
        assert!(!supports_typed_spawn_oom(&Syscall::MemInfo {
            out_ptr: 0,
            out_len: 0,
        }));
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
        assert!(map_syscall(255, 0, 0, 0, 0).is_none());

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
        assert_eq!(
            crate::memory::pin::release_vfs_lease(HOLDER, OWNER, GENERATION),
            alloc::vec![]
        );

        assert!(!withhold_or_free(BASE, 1));
        assert_eq!(crate::memory::pin::quarantined_pages(), before);
        assert!(crate::memory::pin::holder_of(BASE, 4096).is_none());
    }
}
