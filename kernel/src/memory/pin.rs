//! Async Pinning Registry — memory regions an in-flight asynchronous operation
//! may still be reading or writing.
//!
//! # Contract
//!
//! A region is *pinned* while an agent outside the owning cell's control — a
//! device programmed through the IOMMU, a kernel ISR, or a service holding a raw
//! pointer into the region — may still touch its frames. While a region is
//! pinned:
//!
//! * the owner may not free it: `GrantFree` and `GrantUnregister` refuse rather
//!   than hand the frames back to the allocator, and
//! * if the owner dies, the frames are **quarantined** — withheld from the frame
//!   allocator instead of returned to it.
//!
//! Quarantine is the third option between the two failure modes. Returning a
//! pinned frame leaves a device writing into whatever cell is allocated it next;
//! blocking the death until the pin clears breaks the never-die property, since
//! init restarts every permanent service and a cell dying mid-operation is the
//! ordinary case. Nothing here ever delays a death: marking is a bounded scan of
//! a fixed table and takes no other lock.
//!
//! Quarantine ends only on an explicit driver acknowledgement
//! ([`acknowledge`]) — never on a timer, never implicitly. Frames with no
//! acknowledgement stay withheld for the life of the boot, which leaks memory
//! but cannot corrupt another cell.
//!
//! # Lock order
//!
//! `REGISTRY` is a **leaf**: no other lock is acquired while it is held, and it
//! is never held across `FRAME_ALLOCATOR`, `KERNEL_ROOT` or `SCHEDULER`. The
//! grant teardown path therefore runs
//!
//! ```text
//! PAGE_GRANT_TABLE / REG_GRANT_TABLE → REGISTRY → (all released) → FRAME_ALLOCATOR → KERNEL_ROOT
//! ```
//!
//! `FRAME_ALLOCATOR` is the OUTER lock of the last pair, contrary to what the
//! older comments on `reap_grants_for_task` and `yield_cpu` claimed:
//! `free_grant_pages` takes `FRAME_ALLOCATOR` first and holds it across
//! `unmap_page`/`map_page`, each of which takes `KERNEL_ROOT` internally.
//! Neither is ever acquired while `SCHEDULER` is held — the watchdog defers its
//! reap list precisely so `yield_cpu` can drain it after dropping `SCHEDULER`.

use crate::sync::Spinlock;
use alloc::vec::Vec;

const PAGE_SIZE: usize = 4096;

/// Regions the kernel tracks as pinned at one time, across all cells.
///
/// A fixed table rather than a map: the count must not be driven by anything a
/// caller supplies, and the teardown paths that scan it run on cell death.
const MAX_PINNED_REGIONS: usize = 128;

/// Per-owner ceiling, so one driver cell cannot starve the table.
///
/// Sized from the largest real consumer: the e1000 cell authorises 34 regions
/// (two rings plus 16 TX and 16 RX buffers).
const MAX_PINS_PER_TASK: usize = 48;

/// Frame ranges the quarantine can hold at one time. Entries are only created
/// by the reaper, one per grant a dying cell had pinned, so the bound is the
/// number of grants that can be in flight across all dead cells at once.
const MAX_QUARANTINED_REGIONS: usize = 64;
const MAX_VFS_LEASES: usize = 32;

/// Why a region could not be pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    /// Zero length, or `base + len` overflows the address space.
    InvalidRange,
    /// The global table is full.
    TableFull,
    /// This owner already holds [`MAX_PINS_PER_TASK`] regions.
    TaskLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsLeaseError {
    InvalidRange,
    TableFull,
    TaskLimit,
    MissingCallerContext,
    WrongOwner,
    WrongGrantee,
    DuplicateLease,
}

/// Who holds a pin overlapping a queried region.
#[derive(Debug, Clone, Copy)]
pub struct PinHolder {
    /// Page-aligned base of the pinned region.
    pub base: usize,
    /// Length of the pinned region in pages.
    pub pages: usize,
    /// Task id that owns the memory participating in the operation.
    pub owner: usize,
    /// Outstanding pin requests against this region. Released together by
    /// [`acknowledge`]; there is no per-request release today.
    pub holds: u32,
    /// The owner has died and these frames are withheld from the allocator.
    pub quarantined: bool,
    /// Exact VFS request generation when this hold belongs to a VFS lease.
    pub request_generation: u64,
    /// VFS task holding the lease, or 0 for owner-wide DMA pins.
    pub holder_tid: usize,
}

#[derive(Clone, Copy)]
struct PinEntry {
    base: usize,
    /// Zero marks a free slot.
    pages: usize,
    owner: usize,
    holds: u32,
    quarantined: bool,
}

#[derive(Clone, Copy)]
struct VfsLeaseEntry {
    base: usize,
    pages: usize,
    grant_owner: usize,
    holder_tid: usize,
    grant_id: usize,
    request_generation: u64,
    quarantined: bool,
}

/// Frames the reaper declined to return to the allocator, awaiting an
/// acknowledgement from `owner`.
#[derive(Clone, Copy)]
struct FrameHold {
    base: usize,
    /// Zero marks a free slot.
    pages: usize,
    release: ReleaseKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReleaseKey {
    None,
    Owner(usize),
    VfsLease {
        holder_tid: usize,
        grant_owner: usize,
        request_generation: u64,
    },
}

const EMPTY_PIN: PinEntry = PinEntry {
    base: 0,
    pages: 0,
    owner: 0,
    holds: 0,
    quarantined: false,
};

const EMPTY_HOLD: FrameHold = FrameHold {
    base: 0,
    pages: 0,
    release: ReleaseKey::None,
};

const EMPTY_VFS_LEASE: VfsLeaseEntry = VfsLeaseEntry {
    base: 0,
    pages: 0,
    grant_owner: 0,
    holder_tid: 0,
    grant_id: 0,
    request_generation: 0,
    quarantined: false,
};

struct Registry {
    pins: [PinEntry; MAX_PINNED_REGIONS],
    vfs_leases: [VfsLeaseEntry; MAX_VFS_LEASES],
    quarantine: [FrameHold; MAX_QUARANTINED_REGIONS],
}

/// One lock covers both tables: they are only ever consulted together, and a
/// single leaf lock removes any ordering question between them.
static REGISTRY: Spinlock<Registry> = Spinlock::new(Registry {
    pins: [EMPTY_PIN; MAX_PINNED_REGIONS],
    vfs_leases: [EMPTY_VFS_LEASE; MAX_VFS_LEASES],
    quarantine: [EMPTY_HOLD; MAX_QUARANTINED_REGIONS],
});

/// Page-aligned `(base, page_count)` covering `[base, base + len)`.
fn span(base: usize, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let start = base & !(PAGE_SIZE - 1);
    let end = base.checked_add(len)?.checked_add(PAGE_SIZE - 1)? & !(PAGE_SIZE - 1);
    Some((start, (end - start) / PAGE_SIZE))
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 + b.1 * PAGE_SIZE && b.0 < a.0 + a.1 * PAGE_SIZE
}

fn pin_holder_from_pin(entry: &PinEntry) -> PinHolder {
    PinHolder {
        base: entry.base,
        pages: entry.pages,
        owner: entry.owner,
        holds: entry.holds,
        quarantined: entry.quarantined,
        request_generation: 0,
        holder_tid: 0,
    }
}

fn pin_holder_from_vfs_lease(entry: &VfsLeaseEntry) -> PinHolder {
    PinHolder {
        base: entry.base,
        pages: entry.pages,
        owner: entry.grant_owner,
        holds: 1,
        quarantined: entry.quarantined,
        request_generation: entry.request_generation,
        holder_tid: entry.holder_tid,
    }
}

fn same_vfs_lease(
    entry: &VfsLeaseEntry,
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> bool {
    entry.pages != 0
        && entry.holder_tid == holder_tid
        && entry.grant_owner == grant_owner
        && entry.request_generation == request_generation
}

fn hold_quarantined_frames(
    reg: &mut Registry,
    base: usize,
    pages: usize,
    release: ReleaseKey,
) -> bool {
    if pages == 0 {
        return false;
    }
    match reg.quarantine.iter_mut().find(|entry| entry.pages == 0) {
        Some(slot) => {
            *slot = FrameHold {
                base,
                pages,
                release,
            };
            true
        }
        None => false,
    }
}

fn release_quarantined_frames<F>(reg: &mut Registry, mut matches: F) -> Vec<(usize, usize)>
where
    F: FnMut(ReleaseKey) -> bool,
{
    let mut released = Vec::new();
    for hold in reg.quarantine.iter_mut() {
        if hold.pages != 0 && matches(hold.release) {
            released.push((hold.base, hold.pages));
            *hold = EMPTY_HOLD;
        }
    }
    released
}

/// Record `[base, base + len)` as participating in an in-flight operation owned
/// by task `owner`, so the region cannot be freed underneath it.
///
/// Re-pinning a range already held by the same owner bumps its hold count
/// instead of consuming a second slot.
///
/// # Errors
/// [`PinError::InvalidRange`] for an empty or overflowing range,
/// [`PinError::TaskLimit`] once the owner holds [`MAX_PINS_PER_TASK`] regions,
/// [`PinError::TableFull`] when no slot is left. Callers must fail the operation
/// closed: authorising a device against memory the kernel cannot protect is the
/// use-after-free this registry exists to prevent.
pub fn pin(base: usize, len: usize, owner: usize) -> Result<(), PinError> {
    let (start, pages) = span(base, len).ok_or(PinError::InvalidRange)?;
    let mut reg = REGISTRY.lock();
    let mut owned = 0usize;
    let mut free_slot = None;
    let mut existing = None;
    for (i, e) in reg.pins.iter().enumerate() {
        if e.pages == 0 {
            if free_slot.is_none() {
                free_slot = Some(i);
            }
            continue;
        }
        if e.owner == owner {
            owned += 1;
            if e.base == start && e.pages == pages && !e.quarantined {
                existing = Some(i);
                break;
            }
        }
    }
    if let Some(i) = existing {
        reg.pins[i].holds = reg.pins[i].holds.saturating_add(1);
        return Ok(());
    }
    if owned >= MAX_PINS_PER_TASK {
        return Err(PinError::TaskLimit);
    }
    let slot = free_slot.ok_or(PinError::TableFull)?;
    reg.pins[slot] = PinEntry {
        base: start,
        pages,
        owner,
        holds: 1,
        quarantined: false,
    };
    Ok(())
}

/// The first pin overlapping `[base, base + len)`, if any.
///
/// Teardown paths call this to decide whether frames may be released.
pub fn holder_of(base: usize, len: usize) -> Option<PinHolder> {
    let probe = span(base, len)?;
    let reg = REGISTRY.lock();
    reg.pins
        .iter()
        .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
        .map(pin_holder_from_pin)
        .or_else(|| {
            reg.vfs_leases
                .iter()
                .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
                .map(pin_holder_from_vfs_lease)
        })
}

/// The overlapping VFS lease for `grant_owner`, if any.
///
/// Grant teardown uses this while holding a grant-table lock to distinguish a
/// live VFS per-request lease from a plain shared registered grant. The lock
/// order stays grant table -> pin registry because this function takes only the
/// pin leaf.
pub(crate) fn vfs_holder_of_owner(
    base: usize,
    len: usize,
    grant_owner: usize,
) -> Option<PinHolder> {
    let probe = span(base, len)?;
    let reg = REGISTRY.lock();
    reg.vfs_leases
        .iter()
        .find(|e| {
            e.pages != 0 && e.grant_owner == grant_owner && overlaps(probe, (e.base, e.pages))
        })
        .map(pin_holder_from_vfs_lease)
}

/// Mark every pin owned by `tid` as quarantined and report how many.
///
/// Called from the grant reaper before the grant tables are swept. Marking does
/// not free anything and does not wait: the dying task proceeds immediately.
pub fn quarantine_task(tid: usize) -> usize {
    let mut reg = REGISTRY.lock();
    let mut marked = 0;
    for e in reg.pins.iter_mut() {
        if e.pages != 0 && e.owner == tid {
            e.quarantined = true;
            marked += 1;
        }
    }
    for e in reg.vfs_leases.iter_mut() {
        if e.pages != 0 && e.grant_owner == tid {
            e.quarantined = true;
            marked += 1;
        }
    }
    marked
}

/// Take custody of `pages` frames at `base` that the reaper declined to free,
/// charging them to the pin holder `owner` whose acknowledgement releases them.
///
/// Only the reaper calls this, and only for frames it was otherwise about to
/// return to the allocator. That is what makes [`acknowledge`] safe to free
/// blindly: a range that never came out of a grant — an MMIO window a driver
/// authorised for DMA, or a grant whose ownership passed to a live grantee —
/// never enters the quarantine and so is never handed to `deallocate_frame`.
///
/// Returns `false` when the quarantine is full. The caller must still not free
/// the frames: they are leaked for the life of the boot, which is the safe
/// direction.
pub fn withhold_frames(base: usize, pages: usize, owner: usize) -> bool {
    let mut reg = REGISTRY.lock();
    hold_quarantined_frames(&mut reg, base, pages, ReleaseKey::Owner(owner))
}

pub fn withhold_vfs_frames(
    base: usize,
    pages: usize,
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> bool {
    let mut reg = REGISTRY.lock();
    hold_quarantined_frames(
        &mut reg,
        base,
        pages,
        ReleaseKey::VfsLease {
            holder_tid,
            grant_owner,
            request_generation,
        },
    )
}

/// Driver acknowledgement for `tid`: drop its pins and hand back the
/// `(base, pages)` frame ranges the reaper placed in quarantine on its behalf.
///
/// The caller returns those frames to the allocator. Safe to call before the
/// reaper as well as after — an acknowledgement that arrives first simply drops
/// the pins, and the reaper then frees the frames itself.
pub fn acknowledge(tid: usize) -> Vec<(usize, usize)> {
    let mut reg = REGISTRY.lock();
    for e in reg.pins.iter_mut() {
        if e.pages != 0 && e.owner == tid {
            *e = EMPTY_PIN;
        }
    }
    release_quarantined_frames(&mut reg, |release| release == ReleaseKey::Owner(tid))
}

pub fn pin_vfs_lease(
    base: usize,
    len: usize,
    grant_owner: usize,
    holder_tid: usize,
    grant_id: usize,
    request_generation: u64,
) -> Result<(), VfsLeaseError> {
    let (start, pages) = span(base, len).ok_or(VfsLeaseError::InvalidRange)?;
    let mut reg = REGISTRY.lock();
    let mut owned = 0usize;
    let mut free_slot = None;
    for (i, e) in reg.vfs_leases.iter().enumerate() {
        if e.pages == 0 {
            if free_slot.is_none() {
                free_slot = Some(i);
            }
            continue;
        }
        if e.holder_tid == holder_tid {
            owned += 1;
        }
        if same_vfs_lease(e, holder_tid, grant_owner, request_generation) {
            return Err(VfsLeaseError::DuplicateLease);
        }
    }
    if owned >= MAX_PINS_PER_TASK {
        return Err(VfsLeaseError::TaskLimit);
    }
    let slot = free_slot.ok_or(VfsLeaseError::TableFull)?;
    reg.vfs_leases[slot] = VfsLeaseEntry {
        base: start,
        pages,
        grant_owner,
        holder_tid,
        grant_id,
        request_generation,
        quarantined: false,
    };
    Ok(())
}

pub fn release_vfs_lease(
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> Vec<(usize, usize)> {
    let mut reg = REGISTRY.lock();
    for e in reg.vfs_leases.iter_mut() {
        if same_vfs_lease(e, holder_tid, grant_owner, request_generation) {
            *e = EMPTY_VFS_LEASE;
        }
    }
    release_quarantined_frames(&mut reg, |release| {
        release
            == ReleaseKey::VfsLease {
                holder_tid,
                grant_owner,
                request_generation,
            }
    })
}

pub fn release_vfs_holder_leases(holder_tid: usize) -> Vec<(usize, usize)> {
    let mut reg = REGISTRY.lock();
    for e in reg.vfs_leases.iter_mut() {
        if e.pages != 0 && e.holder_tid == holder_tid {
            *e = EMPTY_VFS_LEASE;
        }
    }
    release_quarantined_frames(&mut reg, |release| {
        matches!(
            release,
            ReleaseKey::VfsLease {
                holder_tid: lease_holder,
                ..
            } if lease_holder == holder_tid
        )
    })
}

pub fn find_vfs_lease(
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> Option<(usize, usize, usize)> {
    let reg = REGISTRY.lock();
    reg.vfs_leases
        .iter()
        .find(|e| same_vfs_lease(e, holder_tid, grant_owner, request_generation))
        .map(|e| (e.base, e.pages, e.grant_id))
}

/// Pages currently withheld from the frame allocator.
pub fn quarantined_pages() -> usize {
    let reg = REGISTRY.lock();
    reg.quarantine.iter().map(|q| q.pages).sum()
}

/// Release the registry lock unconditionally during fault teardown.
///
/// # Safety
/// Only valid from `force_unlock_all_kernel_locks`, with interrupts disabled and
/// no other context able to observe a half-written table.
pub unsafe fn force_unlock() {
    REGISTRY.force_unlock();
}

#[cfg(test)]
#[path = "pin_tests.rs"]
mod tests;
