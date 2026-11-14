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
//! The withheld frames are a `FrameRun` held as `Quarantined(Pinned(key))` in
//! the frame-run quarantine (`memory::frame_run::PENDING`), still mapped: the
//! holder may still dereference them, so the hold is taken before anything is
//! unpublished (census F15). This registry decides *which* key protects a range
//! and when that key ends; the pending list keeps the frames and counts them
//! against the 1/64 quarantine cap. A released run comes back `Mapped`, and the
//! caller resumes its release (unpublish, flush, free) outside every lock here.
//!
//! # Lock order
//!
//! `REGISTRY` is a **leaf** except for the frame-run quarantine's own lock,
//! which ranks after it: holds are installed, re-keyed and ended in the same
//! transaction that finds their protector. It is never held across
//! `FRAME_ALLOCATOR`, `KERNEL_ROOT` or `SCHEDULER`, and nothing logs under it. The
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

use crate::memory::frame_run::{FrameRun, KernelAlias, Reason, RunKind, RunOwner, PENDING};
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

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

/// p01-trace provenance slots for withheld runs (one per grant a dying cell had
/// pinned); the runs themselves live in the frame-run quarantine, unbounded.
#[cfg(feature = "p01-trace")]
const P01_HOLD_SLOTS: usize = 64;
/// No epoch: an event hold ends on its event, never on a flush.
const NO_EPOCHS: crate::memory::frame_run::Epochs = [0; crate::sched::smp::MAX_HARTS];
pub(crate) const MAX_VFS_LEASES: usize = 32;
/// `GrantSlice` leases (`LEASE` flag) tracked at one time across all holders.
///
/// Sized so a full per-holder table (the compositor between two receives)
/// still leaves room for a second holder.
pub(crate) const MAX_GRANT_LEASES: usize = 128;
/// Distinct `(owner, grant)` leases one holder may keep at once. A re-lease of
/// the same key adds a hold instead of a slot, so this bounds the distinct
/// client buffers one blit pass may touch between two receives.
pub(crate) const MAX_GRANT_LEASES_PER_HOLDER: usize =
    abi::syscall::grant_slice::MAX_LEASES_PER_HOLDER;

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

/// Why a `GrantSlice` lease could not be published. Every variant surfaces to
/// the caller as the unknown-id sentinel; the split is for host tests and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantLeaseError {
    /// Zero length, or `base + len` overflows the address space.
    InvalidRange,
    /// The holder already keeps [`MAX_GRANT_LEASES_PER_HOLDER`] distinct
    /// leases, or one key's hold count would overflow.
    HolderFull,
    /// No global slot is left.
    TableFull,
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
    /// The grant owner died; only the active holder may complete this lease.
    pub pending_revoke: bool,
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

/// One exact cache-maintenance operation submitted by a device driver.
///
/// Unlike broad [`PinEntry`] acknowledgements, this entry is released only by
/// its opaque token. A driver that loses completion evidence leaves the entry
/// live so task teardown quarantines the underlying Grant frames.
#[derive(Clone, Copy)]
struct CacheSyncEntry {
    /// Exact byte range the device was submitted against.
    operation_base: usize,
    operation_len: usize,
    /// Page coverage used only for GrantFree/reaper protection.
    base: usize,
    pages: usize,
    owner: usize,
    token: usize,
    completing: bool,
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
    /// Owner death has revoked future slices but not the holder's existing raw
    /// pointer. The holder alone ends this exact lease.
    pending_revoke: bool,
}

/// One holder's lease on one Grant, published by `GrantSlice` with `LEASE`.
///
/// Keyed by `(holder_tid, grant_owner, grant_id)`. The holder ends it at its
/// next receive, its next send to `grant_owner`, or its death; until then the
/// owner cannot free the Grant and the owner's death quarantines the frames
/// under [`ReleaseKey::GrantLease`].
#[derive(Clone, Copy)]
struct GrantLeaseEntry {
    base: usize,
    /// Zero marks a free slot.
    pages: usize,
    grant_owner: usize,
    holder_tid: usize,
    grant_id: usize,
    /// `GrantSlice` calls folded into this key since the holder's last release.
    holds: u32,
    /// The owner died and the frames are withheld under this lease's key.
    quarantined: bool,
}

/// The owner-range lock of one client-memory DMA export (`ExportDmaRange =
/// 437`, ADR-0033 CP1 §5.1, §5.5).
///
/// Present from export until the lease is dropped after an acknowledged
/// revoke. While present the owner cannot free the range, and the owner's
/// death quarantines its frames under [`ReleaseKey::DmaExport`]: a device in
/// the holder's VT-d domain may still translate the export window until the
/// IOTLB drain is acknowledged.
#[derive(Clone, Copy)]
struct DmaExportLock {
    base: usize,
    pages: usize,
    owner: usize,
    holder_cell: u64,
}

/// p01-trace provenance of one withheld run, kept beside the run (which lives
/// in the frame-run quarantine) until its key ends.
#[cfg(feature = "p01-trace")]
#[derive(Clone, Copy)]
struct P01Hold {
    base: usize,
    /// Zero marks a free slot.
    pages: usize,
    p01_obligation: Option<crate::p01_resources::Obligation>,
    p01_range: Option<crate::p01_retained::Token>,
}

/// Withheld frames whose key ended: the run is `Mapped` again, and the caller
/// resumes its release (unpublish, flush, free) outside `REGISTRY`. Dropping
/// one unreleased leaks the frames (counted), never frees them. Private
/// provenance survives removal from REGISTRY and is retired by the caller only
/// after actual allocator release.
#[derive(Debug)]
pub struct ReleasedRange {
    pub run: FrameRun,
    #[cfg(feature = "p01-trace")]
    pub(crate) p01_range: Option<crate::p01_retained::Token>,
    #[cfg(feature = "p01-trace")]
    pub(crate) p01_obligation: Option<crate::p01_resources::Obligation>,
}

impl ReleasedRange {
    pub fn base(&self) -> usize {
        self.run.base()
    }

    pub fn pages(&self) -> usize {
        self.run.pages()
    }
}

impl PartialEq for ReleasedRange {
    fn eq(&self, other: &Self) -> bool {
        (self.base(), self.pages()) == (other.base(), other.pages())
    }
}
impl Eq for ReleasedRange {}

// Existing source selftests compare physical custody, not private provenance.
impl PartialEq<(usize, usize)> for ReleasedRange {
    fn eq(&self, other: &(usize, usize)) -> bool {
        (self.base(), self.pages()) == *other
    }
}

/// What ends a pin hold; also the key of a `frame_run::Reason::Pinned` hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseKey {
    Owner(usize),
    /// A VideoCore mailbox request may complete after its cell dies. It has no
    /// IOMMU completion acknowledgement, so its frames are boot-lifetime.
    CacheSyncBootLifetime,
    VfsLease {
        holder_tid: usize,
        grant_owner: usize,
        request_generation: u64,
    },
    /// A `GrantSlice` lease whose owner died while the holder still used it.
    GrantLease {
        holder_tid: usize,
        grant_owner: usize,
        grant_id: usize,
    },
    /// A DMA export whose window invalidation was not yet acknowledged when the
    /// owner's frames were reclaimed; released only by that export's revoke.
    DmaExport {
        owner: usize,
        holder_cell: u64,
        export_id: u64,
    },
}
/// Result of the reaper's single-transaction pin-to-quarantine transfer.
///
/// `Free` means no live pin protected the range at the transfer linearization
/// point, so the reaper may return the frames to the allocator. `Full` means a
/// live pin did protect them but the bounded quarantine has no slot; callers
/// must leak rather than free in that case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameTransfer {
    Free,
    Withheld,
    Full,
}

const EMPTY_PIN: PinEntry = PinEntry {
    base: 0,
    pages: 0,
    owner: 0,
    holds: 0,
    quarantined: false,
};

const EMPTY_CACHE_SYNC: CacheSyncEntry = CacheSyncEntry {
    operation_base: 0,
    operation_len: 0,
    base: 0,
    pages: 0,
    owner: 0,
    token: 0,
    completing: false,
    quarantined: false,
};

#[cfg(feature = "p01-trace")]
const EMPTY_P01_HOLD: P01Hold = P01Hold {
    base: 0,
    pages: 0,
    p01_obligation: None,
    p01_range: None,
};

const EMPTY_VFS_LEASE: VfsLeaseEntry = VfsLeaseEntry {
    base: 0,
    pages: 0,
    grant_owner: 0,
    holder_tid: 0,
    grant_id: 0,
    request_generation: 0,
    quarantined: false,
    pending_revoke: false,
};

const EMPTY_GRANT_LEASE: GrantLeaseEntry = GrantLeaseEntry {
    base: 0,
    pages: 0,
    grant_owner: 0,
    holder_tid: 0,
    grant_id: 0,
    holds: 0,
    quarantined: false,
};

struct Registry {
    pins: [PinEntry; MAX_PINNED_REGIONS],
    cache_sync: [CacheSyncEntry; MAX_PINNED_REGIONS],
    next_cache_token: usize,
    vfs_leases: [VfsLeaseEntry; MAX_VFS_LEASES],
    grant_leases: [GrantLeaseEntry; MAX_GRANT_LEASES],
    /// Export range locks keyed by export id; bounded by the lease table in
    /// `task::dma_export`, which is the only writer.
    dma_exports: BTreeMap<u64, DmaExportLock>,
    #[cfg(feature = "p01-trace")]
    p01_holds: [P01Hold; P01_HOLD_SLOTS],
}

/// One lock covers both tables: they are only ever consulted together, and a
/// single leaf lock removes any ordering question between them.
static REGISTRY: Spinlock<Registry> = Spinlock::ranked(
    Registry {
        pins: [EMPTY_PIN; MAX_PINNED_REGIONS],
        cache_sync: [EMPTY_CACHE_SYNC; MAX_PINNED_REGIONS],
        next_cache_token: 1,
        vfs_leases: [EMPTY_VFS_LEASE; MAX_VFS_LEASES],
        grant_leases: [EMPTY_GRANT_LEASE; MAX_GRANT_LEASES],
        dma_exports: BTreeMap::new(),
        #[cfg(feature = "p01-trace")]
        p01_holds: [EMPTY_P01_HOLD; P01_HOLD_SLOTS],
    },
    crate::sync::rank::PIN_REGISTRY,
);

/// Live entries in `Registry::grant_leases`.
///
/// Written only while `REGISTRY` is held, so it always equals the table's
/// occupancy at the last unlock. Every receive and send runs the lease-end
/// hook; this lock-free read lets those hot paths skip `REGISTRY` entirely
/// while no holder anywhere has a lease (the common case).
static LIVE_GRANT_LEASES: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "p01-trace")]
pub(crate) fn p01_static_bytes() -> usize {
    // The provenance slots beside the frame-run quarantine's runs.
    P01_HOLD_SLOTS
        * (core::mem::size_of::<Option<crate::p01_resources::Obligation>>()
            + core::mem::size_of::<Option<crate::p01_retained::Token>>())
}

#[cfg(feature = "p01-trace")]
pub(crate) fn p01_retirement_check(owner: crate::p01_measurement::Owner) {
    let tid = owner.tid as usize;
    let registry = REGISTRY.lock();
    let pending = registry.pins.iter().any(|e| e.pages != 0 && e.owner == tid)
        || registry
            .cache_sync
            .iter()
            .any(|e| e.pages != 0 && e.owner == tid)
        || registry
            .vfs_leases
            .iter()
            .any(|e| e.pages != 0 && (e.grant_owner == tid || e.holder_tid == tid))
        || registry
            .grant_leases
            .iter()
            .any(|e| e.pages != 0 && (e.grant_owner == tid || e.holder_tid == tid))
        || held_pages(|key| key_names_any(key, &[tid])) > 0;
    if pending {
        crate::p01_resources::unknown(owner, 3);
        crate::p01_resources::unknown(owner, 5);
    } else {
        crate::p01_resources::checked(owner, 3);
        crate::p01_resources::checked(owner, 5);
    }
}

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
        pending_revoke: false,
        request_generation: 0,
        holder_tid: 0,
    }
}

fn pin_holder_from_cache_sync(entry: &CacheSyncEntry) -> PinHolder {
    PinHolder {
        base: entry.base,
        pages: entry.pages,
        owner: entry.owner,
        holds: 1,
        quarantined: entry.quarantined,
        pending_revoke: false,
        request_generation: entry.token as u64,
        holder_tid: entry.owner,
    }
}

fn pin_holder_from_vfs_lease(entry: &VfsLeaseEntry) -> PinHolder {
    PinHolder {
        base: entry.base,
        pages: entry.pages,
        owner: entry.grant_owner,
        holds: 1,
        quarantined: entry.quarantined,
        pending_revoke: entry.pending_revoke,
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

fn pin_holder_from_grant_lease(entry: &GrantLeaseEntry) -> PinHolder {
    PinHolder {
        base: entry.base,
        pages: entry.pages,
        owner: entry.grant_owner,
        holds: entry.holds,
        quarantined: entry.quarantined,
        // A quarantined lease outlived its owner: only the holder ends it.
        pending_revoke: entry.quarantined,
        request_generation: 0,
        holder_tid: entry.holder_tid,
    }
}

fn same_grant_lease(
    entry: &GrantLeaseEntry,
    holder_tid: usize,
    grant_owner: usize,
    grant_id: usize,
) -> bool {
    entry.pages != 0
        && entry.holder_tid == holder_tid
        && entry.grant_owner == grant_owner
        && entry.grant_id == grant_id
}

/// Withhold `pages` frames at `base`, still mapped, under `release`: a run in
/// the frame-run quarantine as `Quarantined(Pinned(release))`. `false` when no
/// entry could be recorded (no heap): the frames are then leaked (counted),
/// never freed, and the caller logs it once `REGISTRY` is released.
fn hold_quarantined_frames(
    reg: &mut Registry,
    base: usize,
    pages: usize,
    release: ReleaseKey,
) -> bool {
    if pages == 0 {
        return false;
    }
    #[cfg(feature = "p01-trace")]
    let p01_owner = match release {
        ReleaseKey::Owner(tid) => crate::p01_resources::owner_for_tid(tid),
        ReleaseKey::VfsLease { grant_owner, .. } => {
            crate::p01_resources::owner_for_tid(grant_owner)
        }
        ReleaseKey::GrantLease { grant_owner, .. } => {
            crate::p01_resources::owner_for_tid(grant_owner)
        }
        ReleaseKey::DmaExport { owner, .. } => crate::p01_resources::owner_for_tid(owner),
        ReleaseKey::CacheSyncBootLifetime => reg
            .cache_sync
            .iter()
            .find(|entry| entry.pages != 0 && overlaps((base, pages), (entry.base, entry.pages)))
            .and_then(|entry| crate::p01_resources::owner_for_tid(entry.owner)),
    };
    #[cfg(feature = "p01-trace")]
    let p01_obligation = p01_owner
        .and_then(|owner| crate::p01_resources::obligation(owner, 3, (pages * PAGE_SIZE) as u64));
    #[cfg(feature = "p01-trace")]
    let p01_range = crate::p01_retained::begin(
        p01_owner,
        base,
        pages,
        if release == ReleaseKey::CacheSyncBootLifetime {
            crate::p01_retained::Reason::CacheSync
        } else {
            crate::p01_retained::Reason::PinWithheld
        },
    );
    if !hold_run(grant_run(base, pages), release) {
        #[cfg(feature = "p01-trace")]
        {
            // Observe Full before frames lose their only structured release key.
            crate::p01_retained::mark(p01_range, crate::p01_retained::Reason::PinFull);
            crate::p01_resources::finish(p01_obligation, 5);
            if let Some(owner) = p01_owner {
                crate::p01_resources::retained(owner, 3);
            }
        }
        return false;
    }
    #[cfg(feature = "p01-trace")]
    match reg.p01_holds.iter_mut().find(|entry| entry.pages == 0) {
        Some(slot) => {
            *slot = P01Hold {
                base,
                pages,
                p01_obligation,
                p01_range,
            }
        }
        // The frames are held; only their provenance is lost.
        None => crate::p01_retained::fault(),
    }
    #[cfg(not(feature = "p01-trace"))]
    let _ = reg;
    true
}

/// A withheld grant range as one run, still mapped (its holder may still use
/// it). The alias is the one the grant release normalises.
fn grant_run(base: usize, pages: usize) -> FrameRun {
    FrameRun::mapped(
        base,
        pages,
        RunOwner::Kernel,
        RunKind::Leaf,
        KernelAlias::of_identity_remap(base),
    )
}

/// Hold a `Mapped` run under `key` in the frame-run quarantine. `false` when it
/// could not be recorded: it is leaked (counted, never freed).
fn hold_run(mut run: FrameRun, key: ReleaseKey) -> bool {
    // A `Mapped` run takes any event hold; anything else is leaked below.
    let _ = run.hold(Reason::Pinned(key));
    match PENDING.try_push(run, &NO_EPOCHS) {
        Ok(()) => true,
        Err((run, _)) => {
            PENDING.forget_unlogged(run);
            false
        }
    }
}

/// End every hold whose key `matches` selects and hand its run back.
fn release_quarantined_frames<F>(reg: &mut Registry, matches: F) -> Vec<ReleasedRange>
where
    F: Fn(ReleaseKey) -> bool,
{
    PENDING
        .release_event(|reason| matches!(reason, Reason::Pinned(key) if matches(*key)))
        .into_iter()
        .map(|run| released_range(reg, run))
        .collect()
}

/// A released run with its p01 provenance, which leaves `REGISTRY` with it.
fn released_range(reg: &mut Registry, run: FrameRun) -> ReleasedRange {
    #[cfg(feature = "p01-trace")]
    let hold = reg
        .p01_holds
        .iter_mut()
        .find(|entry| entry.pages != 0 && entry.base == run.base() && entry.pages == run.pages())
        .map(|entry| core::mem::replace(entry, EMPTY_P01_HOLD))
        .unwrap_or(EMPTY_P01_HOLD);
    #[cfg(not(feature = "p01-trace"))]
    let _ = reg;
    ReleasedRange {
        run,
        #[cfg(feature = "p01-trace")]
        p01_range: hold.p01_range,
        #[cfg(feature = "p01-trace")]
        p01_obligation: hold.p01_obligation,
    }
}

/// Re-key a run whose key just ended to the next live protector of its frames,
/// or hand it back when nothing protects them any more.
fn rekey_or_release(reg: &mut Registry, run: FrameRun, released: &mut Vec<ReleasedRange>) {
    match live_protector_key(reg, (run.base(), run.pages())) {
        // A run that cannot be recorded is leaked (counted), never freed.
        Some(key) => {
            let _ = hold_run(run, key);
        }
        None => {
            let range = released_range(reg, run);
            released.push(range);
        }
    }
}

/// Pages held under the keys `matches` selects.
fn held_pages(matches: impl Fn(&ReleaseKey) -> bool) -> usize {
    PENDING.event_pages(|reason| matches!(reason, Reason::Pinned(key) if matches(key)))
}

/// Does `key` name one of `tids`: an owner-wide acknowledgement by one of them,
/// a VFS or `GrantSlice` lease one granted or held, or an export one owns?
/// Boot-lifetime cache-sync holds carry no task.
fn key_names_any(key: &ReleaseKey, tids: &[usize]) -> bool {
    match *key {
        ReleaseKey::Owner(owner) => tids.contains(&owner),
        ReleaseKey::VfsLease {
            holder_tid,
            grant_owner,
            ..
        } => tids.contains(&holder_tid) || tids.contains(&grant_owner),
        ReleaseKey::GrantLease {
            holder_tid,
            grant_owner,
            ..
        } => tids.contains(&holder_tid) || tids.contains(&grant_owner),
        ReleaseKey::DmaExport { owner, .. } => tids.contains(&owner),
        ReleaseKey::CacheSyncBootLifetime => false,
    }
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
/// Roll back one ordinary pin when setup fails before DMA is enabled.
///
/// The range and owner must match the preceding [`pin`] call exactly.
pub fn rollback_pin(base: usize, len: usize, owner: usize) -> bool {
    let Some((start, pages)) = span(base, len) else {
        return false;
    };
    let mut reg = REGISTRY.lock();
    let Some(entry) = reg.pins.iter_mut().find(|entry| {
        entry.pages == pages && entry.base == start && entry.owner == owner && !entry.quarantined
    }) else {
        return false;
    };
    if entry.holds > 1 {
        entry.holds -= 1;
    } else {
        *entry = EMPTY_PIN;
    }
    true
}

/// Reserve one exact cache synchronization pin and return its non-zero token.
///
/// The caller must prove Grant ownership and logical byte bounds before calling
/// this leaf function. `complete_cache_sync` is the only normal release path.
pub fn begin_cache_sync(base: usize, len: usize, owner: usize) -> Result<usize, PinError> {
    let (start, pages) = span(base, len).ok_or(PinError::InvalidRange)?;
    let mut reg = REGISTRY.lock();
    let owned = reg
        .pins
        .iter()
        .filter(|entry| entry.pages != 0 && entry.owner == owner)
        .count()
        + reg
            .cache_sync
            .iter()
            .filter(|entry| entry.pages != 0 && entry.owner == owner)
            .count();
    if owned >= MAX_PINS_PER_TASK {
        return Err(PinError::TaskLimit);
    }
    let slot = reg
        .cache_sync
        .iter()
        .position(|entry| entry.pages == 0)
        .ok_or(PinError::TableFull)?;
    let mut token = reg.next_cache_token;
    if token == 0 {
        token = 1;
    }
    reg.next_cache_token = token.wrapping_add(1);
    if reg.next_cache_token == 0 {
        reg.next_cache_token = 1;
    }
    reg.cache_sync[slot] = CacheSyncEntry {
        operation_base: base,
        operation_len: len,
        base: start,
        pages,
        owner,
        token,
        completing: false,
        quarantined: false,
    };
    Ok(token)
}

/// Return the exact owned cache-sync span for `token` without releasing it.
pub fn begin_cache_sync_completion(token: usize, owner: usize) -> Option<(usize, usize)> {
    if token == 0 {
        return None;
    }
    let mut reg = REGISTRY.lock();
    reg.cache_sync
        .iter_mut()
        .find(|entry| {
            entry.pages != 0 && entry.owner == owner && entry.token == token && !entry.completing
        })
        .map(|entry| {
            entry.completing = true;
            (entry.operation_base, entry.operation_len)
        })
}

/// Drop only the cache-sync operation authenticated by `(owner, token)`.
pub fn complete_cache_sync(token: usize, owner: usize) -> bool {
    if token == 0 {
        return false;
    }
    let mut reg = REGISTRY.lock();
    let Some(entry) = reg.cache_sync.iter_mut().find(|entry| {
        entry.pages != 0 && entry.owner == owner && entry.token == token && entry.completing
    }) else {
        return false;
    };
    *entry = EMPTY_CACHE_SYNC;
    true
}

/// Roll back a pre-submit cache pin when the platform has no cache primitive.
pub fn cancel_cache_sync(token: usize, owner: usize) -> bool {
    if token == 0 {
        return false;
    }
    let mut reg = REGISTRY.lock();
    let Some(entry) = reg
        .cache_sync
        .iter_mut()
        .find(|entry| entry.pages != 0 && entry.owner == owner && entry.token == token)
    else {
        return false;
    };
    *entry = EMPTY_CACHE_SYNC;
    true
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
            reg.cache_sync
                .iter()
                .find(|entry| entry.pages != 0 && overlaps(probe, (entry.base, entry.pages)))
                .map(pin_holder_from_cache_sync)
        })
        .or_else(|| {
            reg.vfs_leases
                .iter()
                .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
                .map(pin_holder_from_vfs_lease)
        })
        .or_else(|| {
            reg.grant_leases
                .iter()
                .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
                .map(pin_holder_from_grant_lease)
        })
        .or_else(|| {
            reg.dma_exports
                .iter()
                .find(|(_, e)| overlaps(probe, (e.base, e.pages)))
                .map(|(&export_id, e)| PinHolder {
                    base: e.base,
                    pages: e.pages,
                    owner: e.owner,
                    holds: 1,
                    quarantined: false,
                    pending_revoke: false,
                    request_generation: export_id,
                    holder_tid: 0,
                })
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

/// The overlapping `GrantSlice` lease another task holds on `grant_owner`'s
/// Grant, if any.
///
/// The registered-grant reaper calls this under `REG_GRANT_TABLE` (order grant
/// table → pin leaf): a leased row must not pass to its grantee at owner death,
/// because the frames then belong to the quarantine until the lease ends. A
/// dying owner's lease on its own Grant protects nothing and is ignored.
pub(crate) fn grant_lease_holder_of_owner(
    base: usize,
    len: usize,
    grant_owner: usize,
) -> Option<PinHolder> {
    let probe = span(base, len)?;
    let reg = REGISTRY.lock();
    reg.grant_leases
        .iter()
        .find(|e| {
            e.pages != 0
                && e.grant_owner == grant_owner
                && e.holder_tid != grant_owner
                && overlaps(probe, (e.base, e.pages))
        })
        .map(pin_holder_from_grant_lease)
}

/// Mark every non-VFS pin owned by `tid` as quarantined and report how many.
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
    for entry in reg.cache_sync.iter_mut() {
        if entry.pages != 0 && entry.owner == tid {
            entry.quarantined = true;
            marked += 1;
        }
    }
    // VFS leases are per-request and held by another task. Their owner death
    // is marked by `mark_vfs_lease_pending_revoke` from the scheduler while it
    // still has the holder+owner+generation context; broad owner-wide marking
    // would let an unrelated holder release a live raw pointer.
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

/// Take custody of `pages` VFS-lease frames that the reaper declined to free.
///
/// The exact lease is checked while holding the registry lock before the
/// quarantine entry is installed. A missing lease therefore cannot leave an
/// orphaned [`ReleaseKey::VfsLease`] that nobody can release.
pub fn withhold_vfs_frames(
    base: usize,
    pages: usize,
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> bool {
    let mut reg = REGISTRY.lock();
    if !reg
        .vfs_leases
        .iter()
        .any(|entry| same_vfs_lease(entry, holder_tid, grant_owner, request_generation))
    {
        return false;
    }
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

/// Atomically decide whether a dying owner's frames must be quarantined.
///
/// Lookup and custody transfer share the `REGISTRY` transaction. In
/// particular, a VFS holder that completes its exact lease first removes that
/// lease before this function can install its release key, and the reaper then
/// receives [`FrameTransfer::Free`]. If this function wins, that same exact
/// release key owns the quarantine entry until the holder releases it.
///
/// A `GrantSlice` lease is consulted first: it is the only protector whose
/// release re-checks for a remaining protector before freeing (see
/// [`release_orphaned_grant_lease_holds`]), so preferring it never hands the
/// frames to a key that could free them under a live lease holder.
pub(crate) fn withhold_pinned_frames(base: usize, pages: usize) -> FrameTransfer {
    let Some(len) = pages.checked_mul(PAGE_SIZE) else {
        return FrameTransfer::Free;
    };
    let Some(probe) = span(base, len) else {
        return FrameTransfer::Free;
    };
    let mut reg = REGISTRY.lock();

    if let Some(entry) = reg
        .grant_leases
        .iter_mut()
        .find(|entry| entry.pages != 0 && overlaps(probe, (entry.base, entry.pages)))
    {
        entry.quarantined = true;
        let release = ReleaseKey::GrantLease {
            holder_tid: entry.holder_tid,
            grant_owner: entry.grant_owner,
            grant_id: entry.grant_id,
        };
        return if hold_quarantined_frames(&mut reg, base, pages, release) {
            FrameTransfer::Withheld
        } else {
            FrameTransfer::Full
        };
    }

    // Export locks re-check for a remaining protector on release
    // (`unlock_dma_export`), so they too precede the keys that do not.
    if let Some((&export_id, entry)) = reg
        .dma_exports
        .iter()
        .find(|(_, entry)| overlaps(probe, (entry.base, entry.pages)))
    {
        let release = ReleaseKey::DmaExport {
            owner: entry.owner,
            holder_cell: entry.holder_cell,
            export_id,
        };
        return if hold_quarantined_frames(&mut reg, base, pages, release) {
            FrameTransfer::Withheld
        } else {
            FrameTransfer::Full
        };
    }

    if let Some(entry) = reg
        .pins
        .iter()
        .find(|entry| entry.pages != 0 && overlaps(probe, (entry.base, entry.pages)))
    {
        let owner = entry.owner;
        return if hold_quarantined_frames(&mut reg, base, pages, ReleaseKey::Owner(owner)) {
            FrameTransfer::Withheld
        } else {
            FrameTransfer::Full
        };
    }

    #[cfg_attr(not(feature = "p01-trace"), allow(unused_variables))]
    if let Some(entry) = reg
        .cache_sync
        .iter()
        .find(|entry| entry.pages != 0 && overlaps(probe, (entry.base, entry.pages)))
    {
        #[cfg(feature = "p01-trace")]
        if let Some(owner) = crate::p01_resources::owner_for_tid(entry.owner) {
            // CacheSyncBootLifetime intentionally has no release owner. Save
            // attribution before replacing it with that permanent retention key.
            let token = crate::p01_resources::obligation(owner, 3, (pages * PAGE_SIZE) as u64);
            crate::p01_resources::finish(token, 5);
            crate::p01_resources::retained(owner, 3);
        }
        return if hold_quarantined_frames(&mut reg, base, pages, ReleaseKey::CacheSyncBootLifetime)
        {
            FrameTransfer::Withheld
        } else {
            FrameTransfer::Full
        };
    }

    let Some(entry) = reg
        .vfs_leases
        .iter()
        .find(|entry| entry.pages != 0 && overlaps(probe, (entry.base, entry.pages)))
    else {
        return FrameTransfer::Free;
    };
    let release = ReleaseKey::VfsLease {
        holder_tid: entry.holder_tid,
        grant_owner: entry.grant_owner,
        request_generation: entry.request_generation,
    };
    if hold_quarantined_frames(&mut reg, base, pages, release) {
        FrameTransfer::Withheld
    } else {
        FrameTransfer::Full
    }
}

/// IOMMU-driver acknowledgement for `tid`: drop its ordinary pins and hand back the
/// `(base, pages)` frame ranges the reaper placed in quarantine on its behalf.
///
/// The caller returns those frames to the allocator. Safe to call before the
/// reaper as well as after — an acknowledgement that arrives first simply drops
/// ordinary pins, and the reaper then frees the frames itself. Cache-sync pins
/// deliberately do not participate: a VideoCore response has no trustworthy
/// driver acknowledgement and remains boot-lifetime after task death.
pub fn acknowledge(tid: usize) -> Vec<ReleasedRange> {
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
        pending_revoke: false,
    };
    Ok(())
}

/// Mark one live holder's lease revoked because its grant owner has died.
///
/// This preserves the pin and caller context until the holder completes or
/// drops that exact request, while preventing another `GrantSlice` from being
/// issued from the stale context.
pub fn mark_vfs_lease_pending_revoke(
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> bool {
    let mut reg = REGISTRY.lock();
    let Some(entry) = reg
        .vfs_leases
        .iter_mut()
        .find(|entry| same_vfs_lease(entry, holder_tid, grant_owner, request_generation))
    else {
        return false;
    };
    entry.quarantined = true;
    entry.pending_revoke = true;
    true
}

/// Whether the exact live VFS lease has been revoked by owner death.
pub fn vfs_lease_pending_revoke(
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> bool {
    let reg = REGISTRY.lock();
    reg.vfs_leases
        .iter()
        .find(|entry| same_vfs_lease(entry, holder_tid, grant_owner, request_generation))
        .is_some_and(|entry| entry.pending_revoke)
}

pub fn release_vfs_lease(
    holder_tid: usize,
    grant_owner: usize,
    request_generation: u64,
) -> Vec<ReleasedRange> {
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

pub fn release_vfs_holder_leases(holder_tid: usize) -> Vec<ReleasedRange> {
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

/// Publish one `GrantSlice` lease for `holder_tid` on `grant_owner`'s Grant.
///
/// The caller holds the grant-table lock that resolved the row (order grant
/// table → this leaf), so the owner's `GrantFree`/`GrantUnregister` and the
/// owner-death reaper observe the lease before they can remove the row.
/// Re-leasing the same `(holder, owner, grant)` key adds a hold rather than a
/// slot; every hold ends together at the holder's next release boundary.
///
/// # Errors
/// [`GrantLeaseError::InvalidRange`] for an empty or overflowing range,
/// [`GrantLeaseError::HolderFull`] once the holder keeps
/// [`MAX_GRANT_LEASES_PER_HOLDER`] distinct keys (or one key's holds would
/// overflow), [`GrantLeaseError::TableFull`] when no global slot is left.
pub fn lease_grant(
    base: usize,
    len: usize,
    grant_owner: usize,
    holder_tid: usize,
    grant_id: usize,
) -> Result<(), GrantLeaseError> {
    let (start, pages) = span(base, len).ok_or(GrantLeaseError::InvalidRange)?;
    let mut reg = REGISTRY.lock();
    let mut held = 0usize;
    let mut free_slot = None;
    for (i, e) in reg.grant_leases.iter_mut().enumerate() {
        if e.pages == 0 {
            free_slot.get_or_insert(i);
            continue;
        }
        if e.holder_tid != holder_tid {
            continue;
        }
        if e.grant_owner == grant_owner && e.grant_id == grant_id {
            e.holds = e.holds.checked_add(1).ok_or(GrantLeaseError::HolderFull)?;
            return Ok(());
        }
        held += 1;
    }
    if held >= MAX_GRANT_LEASES_PER_HOLDER {
        return Err(GrantLeaseError::HolderFull);
    }
    let slot = free_slot.ok_or(GrantLeaseError::TableFull)?;
    reg.grant_leases[slot] = GrantLeaseEntry {
        base: start,
        pages,
        grant_owner,
        holder_tid,
        grant_id,
        holds: 1,
        quarantined: false,
    };
    LIVE_GRANT_LEASES.fetch_add(1, Ordering::Release);
    Ok(())
}

/// Take back the hold a failed `GrantSlice` added with [`lease_grant`].
///
/// Only for rollback inside the same grant-table transaction: the row is
/// still present, so the owner is alive and no quarantine hold can name it.
pub fn unlease_grant_hold(holder_tid: usize, grant_owner: usize, grant_id: usize) -> bool {
    let mut reg = REGISTRY.lock();
    let Some(entry) = reg
        .grant_leases
        .iter_mut()
        .find(|e| same_grant_lease(e, holder_tid, grant_owner, grant_id))
    else {
        return false;
    };
    entry.holds -= 1;
    if entry.holds == 0 {
        *entry = EMPTY_GRANT_LEASE;
        LIVE_GRANT_LEASES.fetch_sub(1, Ordering::Release);
    }
    true
}

/// Whether any task currently holds a `GrantSlice` lease.
///
/// The receive and send hooks call this before [`release_grant_holder_leases`]
/// or [`release_grant_leases_to_owner`] so a task without leases never takes
/// `REGISTRY`. A holder only asks about its own leases, which it published in
/// an earlier syscall of its own, so a `false` here cannot hide one of them.
pub fn grant_leases_live() -> bool {
    LIVE_GRANT_LEASES.load(Ordering::Acquire) != 0
}

/// End every `GrantSlice` lease `holder_tid` holds: its next receive, or its
/// death. Returns the owner-dead frames that no other protector still covers.
pub fn release_grant_holder_leases(holder_tid: usize) -> Vec<ReleasedRange> {
    let mut reg = REGISTRY.lock();
    clear_grant_leases(&mut reg, |e| e.holder_tid == holder_tid);
    release_orphaned_grant_lease_holds(&mut reg)
}

/// End `holder_tid`'s leases on Grants `grant_owner` owns: the holder's next
/// send to that owner. Leases on other owners' Grants are untouched.
pub fn release_grant_leases_to_owner(holder_tid: usize, grant_owner: usize) -> Vec<ReleasedRange> {
    let mut reg = REGISTRY.lock();
    clear_grant_leases(&mut reg, |e| {
        e.holder_tid == holder_tid && e.grant_owner == grant_owner
    });
    release_orphaned_grant_lease_holds(&mut reg)
}

fn clear_grant_leases(reg: &mut Registry, matches: impl Fn(&GrantLeaseEntry) -> bool) {
    for e in reg.grant_leases.iter_mut() {
        if e.pages != 0 && matches(e) {
            *e = EMPTY_GRANT_LEASE;
            LIVE_GRANT_LEASES.fetch_sub(1, Ordering::Release);
        }
    }
}

/// Settle quarantine holds whose `GrantLease` key no longer names a live lease.
///
/// Several tasks can protect the same owner-dead frames (two holders, or a
/// holder plus a DMA pin), but a hold carries one key. When that key's lease
/// ends, the hold is re-keyed to the next live protector found by
/// [`live_protector_key`]; only frames nothing protects any more are returned
/// for the caller to free outside `REGISTRY`.
fn release_orphaned_grant_lease_holds(reg: &mut Registry) -> Vec<ReleasedRange> {
    let orphaned = {
        let leases = &reg.grant_leases;
        PENDING.release_event(|reason| match *reason {
            Reason::Pinned(ReleaseKey::GrantLease {
                holder_tid,
                grant_owner,
                grant_id,
            }) => !leases
                .iter()
                .any(|e| same_grant_lease(e, holder_tid, grant_owner, grant_id)),
            _ => false,
        })
    };
    let mut released = Vec::new();
    for run in orphaned {
        rekey_or_release(reg, run, &mut released);
    }
    released
}

/// The release key of a live protector overlapping `probe`, marking it
/// quarantined, in [`withhold_pinned_frames`] precedence.
fn live_protector_key(reg: &mut Registry, probe: (usize, usize)) -> Option<ReleaseKey> {
    if let Some(e) = reg
        .grant_leases
        .iter_mut()
        .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
    {
        e.quarantined = true;
        return Some(ReleaseKey::GrantLease {
            holder_tid: e.holder_tid,
            grant_owner: e.grant_owner,
            grant_id: e.grant_id,
        });
    }
    if let Some((&export_id, e)) = reg
        .dma_exports
        .iter()
        .find(|(_, e)| overlaps(probe, (e.base, e.pages)))
    {
        return Some(ReleaseKey::DmaExport {
            owner: e.owner,
            holder_cell: e.holder_cell,
            export_id,
        });
    }
    if let Some(e) = reg
        .pins
        .iter()
        .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
    {
        return Some(ReleaseKey::Owner(e.owner));
    }
    if reg
        .cache_sync
        .iter()
        .any(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))
    {
        return Some(ReleaseKey::CacheSyncBootLifetime);
    }
    let e = reg
        .vfs_leases
        .iter_mut()
        .find(|e| e.pages != 0 && overlaps(probe, (e.base, e.pages)))?;
    e.quarantined = true;
    Some(ReleaseKey::VfsLease {
        holder_tid: e.holder_tid,
        grant_owner: e.grant_owner,
        request_generation: e.request_generation,
    })
}

/// Lock `[base, base + len)` of task `owner` for DMA export `export_id`, held by
/// the Cell `holder_cell` (ADR-0033 CP1 §5.1).
///
/// The caller proves ownership of the range while holding the grant-table lock
/// that resolved it (order grant table → this leaf), so the owner cannot free
/// the range between the check and the lock. Export ids are unique for the boot;
/// the lease table in `task::dma_export` bounds how many locks exist.
///
/// # Errors
/// [`PinError::InvalidRange`] for an empty or overflowing range;
/// [`PinError::TableFull`] if `export_id` is already locked (never issued twice).
pub(crate) fn lock_dma_export(
    base: usize,
    len: usize,
    owner: usize,
    holder_cell: u64,
    export_id: u64,
) -> Result<(), PinError> {
    let (start, pages) = span(base, len).ok_or(PinError::InvalidRange)?;
    let mut reg = REGISTRY.lock();
    if reg.dma_exports.contains_key(&export_id) {
        return Err(PinError::TableFull);
    }
    reg.dma_exports.insert(
        export_id,
        DmaExportLock {
            base: start,
            pages,
            owner,
            holder_cell,
        },
    );
    Ok(())
}

/// Drop export `export_id`'s range lock after its window invalidation was
/// acknowledged, and settle every quarantine hold keyed by it.
///
/// A hold whose frames another protector still covers (a second export of the
/// same pages, a grant lease, a DMA pin) is re-keyed to that protector; only
/// frames nothing protects are returned, for the caller to free outside
/// `REGISTRY`.
pub(crate) fn unlock_dma_export(export_id: u64) -> Vec<ReleasedRange> {
    let mut reg = REGISTRY.lock();
    reg.dma_exports.remove(&export_id);
    let ended = PENDING.release_event(|reason| {
        matches!(reason, Reason::Pinned(ReleaseKey::DmaExport { export_id: id, .. }) if *id == export_id)
    });
    let mut released = Vec::new();
    for run in ended {
        rekey_or_release(&mut reg, run, &mut released);
    }
    released
}

/// Pages withheld under export `export_id`'s release key.
#[cfg(test)]
pub(crate) fn dma_export_quarantined_pages(export_id: u64) -> usize {
    held_pages(|key| matches!(key, ReleaseKey::DmaExport { export_id: id, .. } if *id == export_id))
}

/// Pages currently withheld from the frame allocator under a pin key.
pub fn quarantined_pages() -> usize {
    held_pages(|_| true)
}

/// VFS leases on grants owned by `grant_owner` that its death pending-revoked
/// and whose holder has not released yet.
pub fn vfs_leases_pending_for_owner(grant_owner: usize) -> usize {
    let reg = REGISTRY.lock();
    reg.vfs_leases
        .iter()
        .filter(|e| e.pages != 0 && e.grant_owner == grant_owner && e.pending_revoke)
        .count()
}

/// Quarantined pages whose release key names one of `tids`: an owner-wide
/// acknowledgement by a member, or a VFS or `GrantSlice` lease a member granted
/// or held.
/// Boot-lifetime cache-sync holds carry no task and are never attributed.
pub fn quarantined_pages_attributed(tids: &[usize]) -> usize {
    held_pages(|key| key_names_any(key, tids))
}

#[cfg(test)]
#[path = "pin_tests.rs"]
mod tests;
