//! Client-memory DMA export leases: `ExportDmaRange = 437`, `MapDmaExport =
//! 438`, `RevokeDmaExport = 439` (ADR-0033, CP1 `cp1-gpu-kernel-abi.md` §5-§6).
//!
//! A client Cell (the *owner*) exports a page-aligned range of memory it owns to
//! the live bound provider of a service in [`DMA_EXPORT_SERVICES`] (the
//! *holder*). The holder maps the export into its own VT-d domain at a
//! kernel-allocated IOVA window in `[2^38, 2^39)` and programs that IOVA into its
//! device. The kernel alone revokes: it clears the window's leaves and drains the
//! domain IOTLB, and only an acknowledged drain releases the owner's range. It
//! never waits on the holder (D-G5).
//!
//! # State machine
//!
//! ```text
//! 437 ──► Live{unmapped} ──438──► Live{mapped} ──439/owner death──► Revoking
//!            │                       │                                  │
//!            └──439/owner death──────┴──holder domain revoked──► dropped ◄─ ack
//! ```
//!
//! `Revoking` keeps the owner range locked (and, after owner death, its frames in
//! quarantine under `ReleaseKey::DmaExport`) until an invalidation is
//! acknowledged; [`retry_pending`] repeats it on every scheduler pass.
//!
//! # Locks
//!
//! `EXPORTS` is taken under no lock but `IOMMU_CLEANUP_SERIAL` (holder hook):
//! `IOMMU_CLEANUP_SERIAL → EXPORTS → grant table → SCHEDULER → pin REGISTRY` /
//! `EXPORTS → VT-d UNITS`. The rank table lists them in that order
//! (`sync/rank.rs`: `IOMMU_CLEANUP_SERIAL`, `DMA_EXPORTS`, `PAGE_GRANT_TABLE`,
//! `REG_GRANT_TABLE`, `SCHEDULER`, …), which closes the inversion the census
//! found (F7: `DMA_EXPORTS` used to rank after the grant tables it takes).
//! Every lease transition happens under it, so 437/438/439, owner death and
//! holder death are serialized. No frame is freed under it: an export's
//! released owner frames ride out in the [`KernelPlatform`] and are freed once
//! `EXPORTS` is dropped (`FRAME_ALLOCATOR`/`KERNEL_ROOT` used to be taken under
//! it).

use super::super::SCHEDULER;
use super::SyscallError;
use crate::memory::region::{Receiver, Region, RegionOwner};
use crate::sync::Spinlock;
use abi::dma_export::{ViDmaExportV1, EXPORT_DEVICE_WRITE, EXPORT_FLAGS_KNOWN, EXPORT_MAX_BYTES};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const PAGE: usize = 4096;
const LARGE_PAGE: u64 = 2 << 20;

/// Lowest IOVA of the export window region (D-G3): `2^38`.
pub(crate) const WINDOW_BASE: u64 = 1 << 38;
/// End (exclusive) of the window region: the 39-bit SLPT reach.
pub(crate) const WINDOW_END: u64 = 1 << 39;

/// Live leases across all owners (CP1 §5.1 placeholder, sized from G4 later).
pub(crate) const MAX_LEASES: usize = 4096;
/// Live leases one owner task may hold.
pub(crate) const MAX_LEASES_PER_OWNER: usize = 1024;

/// Test-only holder service id, admitted only in `test-hooks` images: the
/// QEMU `edu` lanes register their holder Cell under it (CP1 §7).
pub(crate) const TEST_EXPORT_SERVICE: u16 = 0x7AE0;

/// Services whose live provider may hold exports (D-G4). Each entry is an owner
/// decision; `test-hooks` images also admit [`TEST_EXPORT_SERVICE`].
pub(crate) const DMA_EXPORT_SERVICES: [u16; 1] = [abi::syscall::service::XE_GPU];

/// Whether `service` may name a holder in this image.
pub(crate) fn holder_service_allowed(service: u16) -> bool {
    DMA_EXPORT_SERVICES.contains(&service)
        || (cfg!(feature = "test-hooks") && service == TEST_EXPORT_SERVICE)
}

/// One Cell incarnation (identity plus generation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Incarnation {
    pub cell: u64,
    pub generation: u64,
}

impl Incarnation {
    fn charge(self) -> crate::memory::cell_quota::Charge {
        crate::memory::cell_quota::Charge::new(types::CellId(self.cell), self.generation)
    }
}

/// A bound provider resolved for an export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Provider {
    pub tid: usize,
    pub who: Incarnation,
    pub pcie_driver: bool,
}

/// Outcome of publishing a window in the holder's domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapOutcome {
    /// Leaves written and the invalidation acknowledged.
    Mapped,
    /// Nothing was published (no unit, DIDs/tables exhausted, window occupied).
    Rejected,
    /// Leaves are hardware-visible but the invalidation timed out. Only the
    /// VT-d backend produces it; other targets never publish a window.
    #[cfg_attr(not(any(test, target_arch = "x86_64")), allow(dead_code))]
    Unconfirmed,
}

/// Everything the lease state machine needs from the rest of the kernel.
///
/// The production implementation is [`KernelPlatform`]; host tests drive the
/// same state machine with a fake whose remapper acknowledgement can fail.
pub(crate) trait Platform {
    /// `(cell, generation)` of a live (not retiring) task.
    fn binding(&self, tid: usize) -> Option<Incarnation>;
    /// Whether any live task belongs to `who`.
    fn incarnation_live(&self, who: Incarnation) -> bool;
    /// A translating remapper is active and every RAM frame lies below
    /// [`WINDOW_BASE`], so no window can alias an identity leaf.
    fn exports_enabled(&self) -> bool;
    /// The live bound provider of `service`.
    fn provider(&self, service: u16) -> Option<Provider>;
    /// Atomically prove `[base, base + len)` is memory `tid` owns and may write
    /// (an owned SAS Grant always is) and lock it under `export_id`. No effect
    /// on refusal.
    fn lock_owned_range(&mut self, req: &RangeLock) -> Result<(), SyscallError>;
    /// Drop the range lock of `export_id` and release the frames nothing else
    /// protects (freed after `EXPORTS` is dropped); returns their pages.
    fn unlock_range(&mut self, export_id: u64) -> usize;
    /// `tid` holds `PcieDriverCap`.
    fn has_pcie_driver(&self, tid: usize) -> bool;
    /// `tid` owns `bdf` (418/424).
    fn owns_bdf(&self, tid: usize, bdf: u32) -> bool;
    /// The remapping unit covering `bdf` translates.
    fn translating(&self, bdf: u32) -> bool;
    /// Charge `bytes` to `holder`'s DMA quota.
    fn reserve_quota(&mut self, holder: Incarnation, bytes: usize) -> bool;
    /// Return `bytes` to `holder`'s DMA quota; a no-op once that incarnation's
    /// CellId names another.
    fn release_quota(&mut self, holder: Incarnation, bytes: usize);
    /// Publish `[iova, iova + len) → [phys, phys + len)` in `holder_tid`'s domain for `bdf`.
    fn map_window(&mut self, w: &WindowMap) -> MapOutcome;
    /// Clear the window's leaves and drain the domain IOTLB; `true` once acknowledged.
    fn unmap_window(&mut self, holder_tid: usize, iova: u64, len: usize) -> bool;
    /// Enable bus mastering for `bdf`.
    fn enable_bus_master(&mut self, bdf: u32);
}

/// The ownership proof and lock a 437 asks for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RangeLock {
    pub tid: usize,
    pub owner: Incarnation,
    pub base: usize,
    pub len: usize,
    pub holder_cell: u64,
    pub export_id: u64,
}

/// One window publication a 438 asks for. Read only by the VT-d backend
/// (and the host fake); other targets refuse before publishing.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(any(test, target_arch = "x86_64")), allow(dead_code))]
pub(crate) struct WindowMap {
    pub holder_tid: usize,
    pub bdf: u32,
    pub iova: u64,
    pub phys: u64,
    pub len: usize,
    pub writable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeaseState {
    Live,
    Revoking,
}

#[derive(Clone, Debug)]
struct Mapping {
    holder_tid: usize,
    iova: u64,
    quota_bytes: usize,
    confirmed: bool,
    bdfs: Vec<u32>,
}

/// An export's own state; the rest of a lease is its [`Region`]: the owner
/// task and incarnation, the holder incarnation (its receiver), the device
/// rights, the locked range of the owner's grant (borrowed, never owned) and
/// the range-lock key it holds in the pin registry.
#[derive(Debug)]
struct Export {
    state: LeaseState,
    mapping: Option<Mapping>,
}

/// One export lease.
type Lease = Region<Export>;

/// No incarnation: what [`Lease::holder`] reports for a region whose receiver
/// is not a holder, which [`Region::export`]'s callers never build. It matches
/// no live binding, so such a lease fails closed (refused, then dropped).
const NO_HOLDER: Incarnation = Incarnation {
    cell: u64::MAX,
    generation: u64::MAX,
};

// The device rights are the only known flag (`rights()` relies on it).
const _: () = assert!(EXPORT_FLAGS_KNOWN == EXPORT_DEVICE_WRITE);

impl Lease {
    /// The owner incarnation.
    fn owner_incarnation(&self) -> Incarnation {
        Incarnation {
            cell: self.owner.cell.0,
            generation: self.owner.generation,
        }
    }

    /// The holder incarnation.
    fn holder(&self) -> Incarnation {
        match self.receiver {
            Receiver::Holder { cell, generation } => Incarnation { cell, generation },
            _ => {
                debug_assert!(false, "an export's receiver is its holder");
                NO_HOLDER
            }
        }
    }

    /// The export flags the rights came from.
    fn flags(&self) -> u16 {
        if self.rights == types::GrantPerm::ReadWrite {
            EXPORT_DEVICE_WRITE
        } else {
            0
        }
    }
}

/// Free IOVA ranges of one holder task's window region, `start → len`.
#[derive(Debug)]
struct Windows {
    free: BTreeMap<u64, u64>,
}

impl Windows {
    fn new() -> Self {
        let mut free = BTreeMap::new();
        free.insert(WINDOW_BASE, WINDOW_END - WINDOW_BASE);
        Self { free }
    }

    /// First fit; 2 MiB aligned for windows of at least 2 MiB so the device
    /// side may use large pages where frames allow.
    fn allocate(&mut self, len: u64) -> Option<u64> {
        let align = if len >= LARGE_PAGE {
            LARGE_PAGE
        } else {
            PAGE as u64
        };
        let (start, span, base) = self.free.iter().find_map(|(&start, &span)| {
            let base = start.checked_add(align - 1)? & !(align - 1);
            (base.checked_add(len)? <= start + span).then_some((start, span, base))
        })?;
        self.free.remove(&start);
        if base > start {
            self.free.insert(start, base - start);
        }
        let end = base + len;
        if end < start + span {
            self.free.insert(end, start + span - end);
        }
        Some(base)
    }

    /// Return `[base, base + len)` and coalesce with its neighbours.
    fn release(&mut self, base: u64, len: u64) {
        let mut start = base;
        let mut end = base + len;
        if let Some((&prev, &span)) = self.free.range(..base).next_back() {
            if prev + span == base {
                self.free.remove(&prev);
                start = prev;
            }
        }
        if let Some(span) = self.free.remove(&end) {
            end += span;
        }
        self.free.insert(start, end - start);
    }
}

/// The lease table: every live or revoking export, plus per-holder windows.
pub(crate) struct ExportTable {
    leases: BTreeMap<u64, Lease>,
    windows: BTreeMap<usize, Windows>,
}

static EXPORTS: Spinlock<ExportTable> =
    Spinlock::ranked(ExportTable::new(), crate::sync::rank::DMA_EXPORTS);

// The documented order (module docs, census F7): the holder hook takes
// `EXPORTS` under `IOMMU_CLEANUP_SERIAL`, and every lease transition takes the
// grant tables and `SCHEDULER` under `EXPORTS`.
const _: () = {
    use crate::sync::rank::{
        DMA_EXPORTS, IOMMU_CLEANUP_SERIAL, PAGE_GRANT_TABLE, REG_GRANT_TABLE, SCHEDULER,
    };
    assert!(IOMMU_CLEANUP_SERIAL.0 < DMA_EXPORTS.0);
    assert!(DMA_EXPORTS.0 < PAGE_GRANT_TABLE.0);
    assert!(DMA_EXPORTS.0 < REG_GRANT_TABLE.0);
    assert!(DMA_EXPORTS.0 < SCHEDULER.0);
};
/// Next export id; boot-wide, never reused. An id taken by an export whose
/// range proof then fails is skipped, never reissued.
static NEXT_EXPORT_ID: AtomicU64 = AtomicU64::new(1);
/// Leases in `Revoking`, so the scheduler pass skips `EXPORTS` when zero.
static REVOKING: AtomicUsize = AtomicUsize::new(0);
/// Live leases of any state, so death hooks skip `EXPORTS` when zero.
static LEASES: AtomicUsize = AtomicUsize::new(0);

/// Upper bound on the SLPT pages one window needs (leaf tables plus their
/// parents), charged to the holder's DMA quota (CP1 §5.2).
fn table_bytes(len: usize) -> usize {
    let leaf_tables = len.div_ceil(1 << 21) + 1;
    let parents = len.div_ceil(1 << 30) + 1;
    (leaf_tables + parents) * PAGE
}

fn rights_name(flags: u16) -> &'static str {
    if flags & EXPORT_DEVICE_WRITE != 0 {
        "RW"
    } else {
        "R"
    }
}

impl ExportTable {
    pub(crate) const fn new() -> Self {
        Self {
            leases: BTreeMap::new(),
            windows: BTreeMap::new(),
        }
    }

    fn set_state(&mut self, id: u64, state: LeaseState) {
        if let Some(lease) = self.leases.get_mut(&id) {
            if lease.ext.state != state {
                if state == LeaseState::Revoking {
                    REVOKING.fetch_add(1, Ordering::Relaxed);
                } else {
                    REVOKING.fetch_sub(1, Ordering::Relaxed);
                }
                lease.ext.state = state;
            }
        }
    }

    /// Release a mapping's window and quota after its leaves are gone.
    fn retire_mapping(
        &mut self,
        platform: &mut impl Platform,
        holder: Incarnation,
        m: Mapping,
        len: usize,
    ) {
        if let Some(windows) = self.windows.get_mut(&m.holder_tid) {
            windows.release(m.iova, len as u64);
        }
        platform.release_quota(holder, m.quota_bytes);
    }

    /// Remove lease `id` entirely: its mapping is gone (or never existed).
    fn drop_lease(&mut self, platform: &mut impl Platform, id: u64, why: &str) {
        let Some(mut lease) = self.leases.remove(&id) else {
            return;
        };
        LEASES.fetch_sub(1, Ordering::Relaxed);
        if lease.ext.state == LeaseState::Revoking {
            REVOKING.fetch_sub(1, Ordering::Relaxed);
        }
        if let Some(m) = lease.ext.mapping.take() {
            self.retire_mapping(platform, lease.holder(), m, lease.size);
        }
        debug_assert!(matches!(
            lease.pin,
            crate::memory::region::PinState::Locked(crate::memory::pin::ReleaseKey::DmaExport {
                export_id,
                ..
            }) if export_id == id
        ));
        let released = platform.unlock_range(id);
        log::warn!(
            "[export] drop id={id} ({why}): owner range {:#x}+{} unlocked, {released} quarantined page(s) released",
            lease.base(),
            lease.size
        );
    }

    /// `Live → Revoking`, then clear and drain the window. Drops the lease on
    /// acknowledgement; otherwise leaves it `Revoking` for [`retry_pending`].
    fn revoke(
        &mut self,
        platform: &mut impl Platform,
        id: u64,
        why: &str,
    ) -> Result<(), SyscallError> {
        self.set_state(id, LeaseState::Revoking);
        let Some(lease) = self.leases.get(&id) else {
            return Err(SyscallError::FileNotFound);
        };
        if let Some(m) = &lease.ext.mapping {
            if !platform.unmap_window(m.holder_tid, m.iova, lease.size) {
                log::warn!(
                    "[export] revoke id={id} ({why}): iova={:#x} invalidation unacknowledged; lease Revoking, range locked",
                    m.iova
                );
                return Err(SyscallError::TryAgain);
            }
            log::warn!(
                "[export] revoke id={id} ({why}): iova={:#x} len={} cleared, IOTLB drain acknowledged",
                m.iova,
                lease.size
            );
        }
        self.drop_lease(platform, id, why);
        Ok(())
    }

    /// 437: export `[va, va + len)` of `caller` to the provider of `service`.
    pub(crate) fn export(
        &mut self,
        platform: &mut impl Platform,
        caller: usize,
        va: usize,
        len: usize,
        service: usize,
        flags: usize,
    ) -> Result<u64, SyscallError> {
        if !va.is_multiple_of(PAGE)
            || len == 0
            || !len.is_multiple_of(PAGE)
            || len as u64 > EXPORT_MAX_BYTES
            || va.checked_add(len).is_none()
            || service > u16::MAX as usize
            || flags & !(EXPORT_FLAGS_KNOWN as usize) != 0
        {
            return Err(SyscallError::InvalidInput);
        }
        let service = service as u16;
        let flags = flags as u16;
        if !holder_service_allowed(service) {
            log::warn!("[export] task {caller} refused: service {service:#x} cannot hold exports");
            return Err(SyscallError::PermissionDenied);
        }
        if !platform.exports_enabled() {
            log::warn!("[export] task {caller} refused: no translating remapper (exports never use identity DMA)");
            return Err(SyscallError::NotSupported);
        }
        let owner = platform
            .binding(caller)
            .ok_or(SyscallError::PermissionDenied)?;
        let provider = platform
            .provider(service)
            .ok_or(SyscallError::FileNotFound)?;
        if !provider.pcie_driver || provider.who == owner {
            log::warn!(
                "[export] task {caller} refused: holder of service {service:#x} is not eligible"
            );
            return Err(SyscallError::PermissionDenied);
        }
        if self.leases.len() >= MAX_LEASES
            || self
                .leases
                .values()
                .filter(|l| l.owner.tid == caller)
                .count()
                >= MAX_LEASES_PER_OWNER
        {
            return Err(SyscallError::OutOfMemory);
        }
        let id = NEXT_EXPORT_ID.fetch_add(1, Ordering::Relaxed);
        platform.lock_owned_range(&RangeLock {
            tid: caller,
            owner,
            base: va,
            len,
            holder_cell: provider.who.cell,
            export_id: id,
        })?;
        let rights = if flags & EXPORT_DEVICE_WRITE != 0 {
            types::GrantPerm::ReadWrite
        } else {
            types::GrantPerm::ReadOnly
        };
        self.leases.insert(
            id,
            Region::export(
                RegionOwner::task(caller, types::CellId(owner.cell), owner.generation),
                va,
                len,
                rights,
                Receiver::Holder {
                    cell: provider.who.cell,
                    generation: provider.who.generation,
                },
                crate::memory::pin::ReleaseKey::DmaExport {
                    owner: caller,
                    holder_cell: provider.who.cell,
                    export_id: id,
                },
                Export {
                    state: LeaseState::Live,
                    mapping: None,
                },
            ),
        );
        LEASES.fetch_add(1, Ordering::Relaxed);
        log::warn!(
            "[export] id={id} owner tid={caller} cell={}/{} holder cell={}/{} va={va:#x} len={len} rights={}",
            owner.cell,
            owner.generation,
            provider.who.cell,
            provider.who.generation,
            rights_name(flags)
        );
        Ok(id)
    }

    /// 438: map export `id` for `bdf` into `caller`'s domain; the record to copy out.
    pub(crate) fn map(
        &mut self,
        platform: &mut impl Platform,
        caller: usize,
        id: u64,
        bdf: usize,
    ) -> Result<ViDmaExportV1, SyscallError> {
        if bdf > 0xFFFF {
            return Err(SyscallError::InvalidInput);
        }
        let bdf = bdf as u32;
        if !platform.has_pcie_driver(caller) || !platform.owns_bdf(caller, bdf) {
            return Err(SyscallError::PermissionDenied);
        }
        let holder = platform
            .binding(caller)
            .ok_or(SyscallError::PermissionDenied)?;
        let lease = self.leases.get(&id).ok_or(SyscallError::FileNotFound)?;
        if lease.holder() != holder {
            log::warn!("[export] map id={id} refused: task {caller} is not the lease holder");
            return Err(SyscallError::PermissionDenied);
        }
        if lease.ext.state != LeaseState::Live {
            return Err(SyscallError::FileNotFound);
        }
        if !platform.translating(bdf) {
            return Err(SyscallError::NotSupported);
        }
        let flags = lease.flags();
        let writable = flags & EXPORT_DEVICE_WRITE != 0;
        let owner = lease.owner_incarnation();
        let record = |iova| {
            ViDmaExportV1::new(
                flags,
                id,
                owner.cell,
                owner.generation,
                iova,
                lease.size as u64,
            )
        };
        if let Some(m) = &lease.ext.mapping {
            if m.holder_tid != caller {
                return Err(SyscallError::PermissionDenied);
            }
            if !m.confirmed {
                return Err(SyscallError::Unknown);
            }
            let (iova, len, base, record) = (m.iova, lease.size, lease.base(), record(m.iova));
            if m.bdfs.contains(&bdf) {
                return Ok(record);
            }
            // Another function of the same holder: attach it to the same window.
            let outcome = platform.map_window(&WindowMap {
                holder_tid: caller,
                bdf,
                iova,
                phys: base as u64,
                len,
                writable,
            });
            let lease = self.leases.get_mut(&id).ok_or(SyscallError::FileNotFound)?;
            let m = lease
                .ext
                .mapping
                .as_mut()
                .ok_or(SyscallError::FileNotFound)?;
            return match outcome {
                MapOutcome::Mapped => {
                    m.bdfs.push(bdf);
                    platform.enable_bus_master(bdf);
                    Ok(record)
                }
                MapOutcome::Rejected => Err(SyscallError::OutOfMemory),
                MapOutcome::Unconfirmed => {
                    m.confirmed = false;
                    Err(SyscallError::Unknown)
                }
            };
        }

        let (len, base) = (lease.size, lease.base());
        let quota_bytes = table_bytes(len);
        let iova = self
            .windows
            .entry(caller)
            .or_insert_with(Windows::new)
            .allocate(len as u64)
            .ok_or(SyscallError::OutOfMemory)?;
        if !platform.reserve_quota(holder, quota_bytes) {
            if let Some(w) = self.windows.get_mut(&caller) {
                w.release(iova, len as u64);
            }
            return Err(SyscallError::OutOfMemory);
        }
        let outcome = platform.map_window(&WindowMap {
            holder_tid: caller,
            bdf,
            iova,
            phys: base as u64,
            len,
            writable,
        });
        if outcome == MapOutcome::Rejected {
            platform.release_quota(holder, quota_bytes);
            if let Some(w) = self.windows.get_mut(&caller) {
                w.release(iova, len as u64);
            }
            log::warn!("[export] map id={id} refused: remapper rejected the window (tables, DIDs or occupied)");
            return Err(SyscallError::OutOfMemory);
        }
        let confirmed = outcome == MapOutcome::Mapped;
        let lease = self.leases.get_mut(&id).ok_or(SyscallError::FileNotFound)?;
        lease.ext.mapping = Some(Mapping {
            holder_tid: caller,
            iova,
            quota_bytes,
            confirmed,
            bdfs: alloc::vec![bdf],
        });
        if !confirmed {
            // Hardware-visible but unacknowledged: like GrantDma, keep the
            // mapping (only revoke/death cleans it) and do not enable BME.
            log::warn!("[export] map id={id} iova={iova:#x} unconfirmed: invalidation timed out");
            return Err(SyscallError::Unknown);
        }
        platform.enable_bus_master(bdf);
        log::warn!(
            "[export] map id={id} holder tid={caller} bdf={:02x}:{:02x}.{} iova={iova:#x} len={len} rights={}",
            (bdf >> 8) & 0xFF,
            (bdf >> 3) & 0x1F,
            bdf & 7,
            rights_name(flags)
        );
        Ok(ViDmaExportV1::new(
            flags,
            id,
            owner.cell,
            owner.generation,
            iova,
            len as u64,
        ))
    }

    /// 439: revoke export `id`; only its owner or holder may ask.
    pub(crate) fn revoke_by(
        &mut self,
        platform: &mut impl Platform,
        caller: usize,
        id: u64,
    ) -> Result<(), SyscallError> {
        let who = platform.binding(caller).ok_or(SyscallError::FileNotFound)?;
        let lease = self.leases.get(&id).ok_or(SyscallError::FileNotFound)?;
        let why = if lease.owner_incarnation() == who {
            "owner"
        } else if lease.holder() == who {
            "holder"
        } else {
            // No existence oracle for third parties.
            return Err(SyscallError::FileNotFound);
        };
        self.revoke(platform, id, why)
    }

    /// Owner death (first step of retirement): revoke every lease `tid` owns.
    pub(crate) fn owner_died(&mut self, platform: &mut impl Platform, tid: usize) {
        let ids: Vec<u64> = self
            .leases
            .iter()
            .filter(|(_, l)| l.owner.tid == tid)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let _ = self.revoke(platform, id, "owner death");
        }
    }

    /// The holder task's whole domain was revoked and acknowledged
    /// (`teardown_dma_authority`): its windows no longer exist.
    pub(crate) fn holder_domain_revoked(&mut self, platform: &mut impl Platform, tid: usize) {
        let ids: Vec<u64> = self
            .leases
            .iter()
            .filter(|(_, l)| l.ext.mapping.as_ref().is_some_and(|m| m.holder_tid == tid))
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let Some(lease) = self.leases.get_mut(&id) else {
                continue;
            };
            let (holder, len) = (lease.holder(), lease.size);
            if let Some(m) = lease.ext.mapping.take() {
                platform.release_quota(holder, m.quota_bytes);
            }
            let revoking = lease.ext.state == LeaseState::Revoking;
            if revoking || !platform.incarnation_live(holder) {
                self.drop_lease(platform, id, "holder domain revoked");
            } else {
                log::warn!("[export] id={id}: holder tid {tid} lost its domain; lease stays Live unmapped ({len} bytes)");
            }
        }
        // The domain and every window in it are gone; a new domain starts empty.
        self.windows.remove(&tid);
        self.drop_orphans(platform);
    }

    /// Drop unmapped leases whose holder incarnation no longer has a live task.
    fn drop_orphans(&mut self, platform: &mut impl Platform) {
        let ids: Vec<u64> = self
            .leases
            .iter()
            .filter(|(_, l)| l.ext.mapping.is_none() && !platform.incarnation_live(l.holder()))
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.drop_lease(platform, id, "holder incarnation gone");
        }
    }

    /// Scheduler pass: retry every `Revoking` lease's invalidation.
    pub(crate) fn retry(&mut self, platform: &mut impl Platform) {
        let ids: Vec<u64> = self
            .leases
            .iter()
            .filter(|(_, l)| l.ext.state == LeaseState::Revoking)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let _ = self.revoke(platform, id, "deferred retry");
        }
    }

    #[cfg(test)]
    fn state(&self, id: u64) -> Option<(LeaseState, Option<u64>)> {
        self.leases
            .get(&id)
            .map(|l| (l.ext.state, l.ext.mapping.as_ref().map(|m| m.iova)))
    }
}

/// 437 entry.
/// Run `f` under `EXPORTS`, then free the owner frames it released only once
/// the lock is dropped (census F7).
fn with_exports<R>(f: impl FnOnce(&mut ExportTable, &mut KernelPlatform) -> R) -> R {
    let mut platform = KernelPlatform::new();
    let result = f(&mut EXPORTS.lock(), &mut platform);
    platform.free_released();
    result
}

pub(crate) fn export(
    caller: usize,
    va: usize,
    len: usize,
    service: usize,
    flags: usize,
) -> Result<usize, SyscallError> {
    with_exports(|exports, platform| {
        exports
            .export(platform, caller, va, len, service, flags)
            .map(|id| id as usize)
    })
}

/// 438 entry. `out_ptr`/`out_len` are validated by the caller before this.
pub(crate) fn map(caller: usize, id: usize, bdf: usize) -> Result<ViDmaExportV1, SyscallError> {
    with_exports(|exports, platform| exports.map(platform, caller, id as u64, bdf))
}

/// 439 entry.
pub(crate) fn revoke(caller: usize, id: usize) -> Result<usize, SyscallError> {
    with_exports(|exports, platform| exports.revoke_by(platform, caller, id as u64).map(|()| 0))
}

/// First retirement step for `tid` (CP1 §6): revoke every lease it owns before
/// any of its frames can be reclaimed.
pub(crate) fn reap_owner(tid: usize) {
    if LEASES.load(Ordering::Relaxed) == 0 {
        return;
    }
    with_exports(|exports, platform| exports.owner_died(platform, tid));
}

/// Holder hook: `tid`'s VT-d domain was torn down and acknowledged.
///
/// Always takes `EXPORTS`: the leases a dying holder affects were created by
/// other tasks, so no program order makes a lock-free zero check exact here.
pub(crate) fn holder_domain_revoked(tid: usize) {
    with_exports(|exports, platform| exports.holder_domain_revoked(platform, tid));
}

/// Scheduler pass: retry unacknowledged revokes (CP1 §5.3 deferred retry).
pub(crate) fn retry_pending() {
    if REVOKING.load(Ordering::Relaxed) == 0 {
        return;
    }
    with_exports(|exports, platform| exports.retry(platform));
}

/// Prove `req.base..+len` lies inside one live Grant `req.tid` owns, under the
/// owner's live binding, and lock it — all inside the grant-table transaction,
/// so neither `GrantFree`/`GrantUnregister` nor the reaper can remove the Grant
/// between the proof and the lock (same shape as `GrantDma`'s reservation).
///
/// SAS Grants are identity-mapped (VA == PA), physically contiguous, mapped
/// USER RW for their owner, and never MMIO, framebuffer, ELF or received
/// memory, so "inside a caller-owned Grant" is CP1 §5.1's whole ownership
/// gate. A range under a live VFS lease is refused. A private-root caller is
/// refused until Tier-2 anonymous memory exists (D-G0).
fn lock_exported_range(req: &RangeLock) -> Result<(), SyscallError> {
    use super::super::tcb::TaskState;
    if super::domain_grant_task(req.tid) {
        log::warn!(
            "[export] task {} refused: private-root exports need Tier-2 anonymous memory (D-G0)",
            req.tid
        );
        return Err(SyscallError::PermissionDenied);
    }
    let binding = (types::CellId(req.owner.cell), req.owner.generation);
    let live_binding = || {
        SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&req.tid))
            .filter(|task| !matches!(task.state, TaskState::Retiring | TaskState::Terminated))
            .map(|task| (task.cell_id, task.cell_generation))
    };
    let lock = || {
        if crate::memory::pin::vfs_holder_of_owner(req.base, req.len, req.tid).is_some() {
            log::warn!(
                "[export] task {} refused: range is under a live VFS lease",
                req.tid
            );
            return Err(SyscallError::PermissionDenied);
        }
        crate::memory::pin::lock_dma_export(
            req.base,
            req.len,
            req.tid,
            req.holder_cell,
            req.export_id,
        )
        .map_err(|_| SyscallError::OutOfMemory)
    };
    {
        let grants = super::grant_table_lock().lock();
        if live_binding() != Some(binding) {
            return Err(SyscallError::PermissionDenied);
        }
        let owned = grants.as_ref().is_some_and(|grants| {
            grants
                .values()
                .any(|g| super::grant_authorizes_dma(g, req.tid, binding, req.base, req.len))
        });
        if owned {
            return lock();
        }
    }
    {
        let grants = super::reg_grant_table_lock().lock();
        if live_binding() != Some(binding) {
            return Err(SyscallError::PermissionDenied);
        }
        let owned = grants.as_ref().is_some_and(|grants| {
            grants
                .values()
                .any(|g| super::grant_authorizes_dma(g, req.tid, binding, req.base, req.len))
        });
        if owned {
            return lock();
        }
    }
    log::warn!(
        "[export] task {} refused: {:#x}+{} is not inside a live Grant it owns",
        req.tid,
        req.base,
        req.len
    );
    Err(SyscallError::PermissionDenied)
}

/// The production [`Platform`]: scheduler, grant tables, pin registry, VT-d.
/// The production [`Platform`]. It carries the owner frames an unlock
/// released out from under `EXPORTS`, for [`Self::free_released`] once the
/// lock is dropped.
pub(crate) struct KernelPlatform {
    released: Vec<crate::memory::pin::ReleasedRange>,
}

impl KernelPlatform {
    const fn new() -> Self {
        Self {
            released: Vec::new(),
        }
    }

    /// Resume the release of every run an unlock handed back: unpublish,
    /// flush, free. Runs with `EXPORTS` released.
    fn free_released(self) {
        for range in self.released {
            super::free_released_range(range);
        }
    }
}

impl Platform for KernelPlatform {
    fn binding(&self, tid: usize) -> Option<Incarnation> {
        super::live_task_binding(tid).map(|(cell, generation)| Incarnation {
            cell: cell.0,
            generation,
        })
    }

    fn incarnation_live(&self, who: Incarnation) -> bool {
        use super::super::tcb::TaskState;
        SCHEDULER.lock().as_ref().is_some_and(|sched| {
            sched.tasks.values().any(|t| {
                t.cell_id.0 == who.cell
                    && t.cell_generation == who.generation
                    && !matches!(t.state, TaskState::Retiring | TaskState::Terminated)
            })
        })
    }

    fn exports_enabled(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            if !crate::dev::iommu::is_active() {
                return false;
            }
            let top = crate::memory::frame::with(|a| a.memory_end()).unwrap_or(usize::MAX);
            if top as u64 > WINDOW_BASE {
                log::warn!("[export] RAM reaches {top:#x} >= 2^38: export windows could alias identity leaves");
                return false;
            }
            true
        }
        #[cfg(not(target_arch = "x86_64"))]
        false
    }

    fn provider(&self, service: u16) -> Option<Provider> {
        let (tid, cell, generation) = crate::cell::service_registry::lookup_bound(service)?;
        Some(Provider {
            tid,
            who: Incarnation { cell, generation },
            pcie_driver: super::caller_has_pcie_driver(tid),
        })
    }

    fn lock_owned_range(&mut self, req: &RangeLock) -> Result<(), SyscallError> {
        lock_exported_range(req)
    }

    fn unlock_range(&mut self, export_id: u64) -> usize {
        let mut released = 0;
        for range in crate::memory::pin::unlock_dma_export(export_id) {
            released += range.pages();
            if self.released.try_reserve(1).is_ok() {
                self.released.push(range);
            } else {
                crate::memory::frame_run::PENDING
                    .leak(range.run, "no heap to defer an export's release");
            }
        }
        released
    }

    fn has_pcie_driver(&self, tid: usize) -> bool {
        super::caller_has_pcie_driver(tid)
    }

    fn owns_bdf(&self, tid: usize, bdf: u32) -> bool {
        crate::dev::ownership::owns_pci(tid, bdf)
    }

    fn translating(&self, bdf: u32) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            crate::arch::dev::iommu_x86::bdf_translating(bdf)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = bdf;
            false
        }
    }

    fn reserve_quota(&mut self, holder: Incarnation, bytes: usize) -> bool {
        crate::memory::cell_quota::Reservation::take(
            holder.charge(),
            crate::memory::cell_quota::Account::Dma,
            bytes,
        )
        .map(crate::memory::cell_quota::Reservation::keep)
        .is_ok()
    }

    fn release_quota(&mut self, holder: Incarnation, bytes: usize) {
        crate::memory::cell_quota::refund(
            holder.charge(),
            crate::memory::cell_quota::Account::Dma,
            bytes,
        );
    }

    fn map_window(&mut self, w: &WindowMap) -> MapOutcome {
        #[cfg(target_arch = "x86_64")]
        {
            use crate::dev::iommu::DmaMapResult;
            match crate::arch::dev::iommu_x86::map_window_for_cell(
                w.holder_tid as u64,
                w.bdf,
                w.iova,
                w.phys,
                w.len,
                w.writable,
            ) {
                DmaMapResult::Mapped(_) => MapOutcome::Mapped,
                DmaMapResult::PublishedUnconfirmed => MapOutcome::Unconfirmed,
                DmaMapResult::Rejected => MapOutcome::Rejected,
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = w;
            MapOutcome::Rejected
        }
    }

    fn unmap_window(&mut self, holder_tid: usize, iova: u64, len: usize) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            crate::arch::dev::iommu_x86::unmap_window_for_cell(holder_tid as u64, iova, len)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (holder_tid, iova, len);
            true
        }
    }

    fn enable_bus_master(&mut self, bdf: u32) {
        crate::dev::pcie_ecam::enable_bus_master(bdf);
    }
}

#[cfg(test)]
#[path = "dma_export_tests.rs"]
mod tests;
