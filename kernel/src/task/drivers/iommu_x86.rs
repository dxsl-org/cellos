//! Intel VT-d DMA isolation driver for x86_64 — per-Cell domain isolation.
//!
//! Phase 1 `init_hw()`:             probe VT-d (GCAP), allocate the root table.
//! Phase 2 `map_range_for_cell()`:  create per-Cell SLPT + DID; write context entry.
//! Phase 3 `activate()`:            enable VT-d translation (TE).
//!
//! Per-Cell domains: each tid with DMA capability gets its own `VtdSlpt` and unique
//! DID (Domain ID). Context entries point to the owning Cell's SLPT; DMA outside that
//! SLPT triggers a VT-d fault.

use super::iommu_pt::VtdSlpt;
use crate::sync::Spinlock;
use alloc::alloc::{alloc_zeroed, Layout};
use alloc::collections::{BTreeMap, BTreeSet};
use core::sync::atomic::{fence, AtomicU16, AtomicUsize, Ordering};

// ── VT-d MMIO register offsets (Intel VT-d spec §10.4) ───────────────────────

const VTD_GCAP: usize = 0x00; // 64-bit capabilities (read-only)
const VTD_ECAP: usize = 0x10; // 64-bit extended capabilities (read-only)
const VTD_GCMD: usize = 0x18; // 32-bit global command (write-only)
const VTD_GSTS: usize = 0x1C; // 32-bit global status (read-only)
const VTD_RTADDR: usize = 0x20; // 64-bit root table address
const VTD_CCMD: usize = 0x28; // 64-bit context command

// GCMD / GSTS bit masks
const TE: u32 = 1 << 31; // Translation Enable
const SRTP: u32 = 1 << 30; // Set Root Table Pointer

// Register-based invalidation encodings (Intel VT-d spec §§10.4.3, 10.4.8).
const CCMD_ICC: u64 = 1u64 << 63; // Invalidate Context-Cache (trigger + status)
const CCMD_GLOBAL: u64 = 1u64 << 61;
const CCMD_DOMAIN: u64 = 2u64 << 61;

// IOTLB invalidation command bits (written to IOTLB register = IOTLB_BASE + 8).
const IOTLB_IVT: u64 = 1u64 << 63; // trigger; hardware clears it on completion
const IOTLB_GLOBAL: u64 = 1u64 << 60;
const IOTLB_DOMAIN: u64 = 2u64 << 60;
#[allow(dead_code)] // reason: page-selective flush path awaits its Phase 02 caller
const IOTLB_PAGE: u64 = 3u64 << 60;
const IOTLB_DRD: u64 = 1u64 << 49;
const IOTLB_DWD: u64 = 1u64 << 48;

// Context entry encoding (VT-d spec §9.3, 128-bit entry):
//   lo[0]    = Present
//   lo[3:2]  = TT (00 = untranslated requests walk the SLPT)
//   lo[11:4] = RESERVED — QEMU faults the walk if any bit is set
//   lo[63:12]= SLPT pointer
//   hi[2:0]  = AW (001 = 39-bit / 3-level AGAW)
//   hi[23:8] = Domain ID
// The first version OR'ed the AW value into lo bits 5:4 (reserved!) and left
// hi AW = 000 (30-bit, unsupported by QEMU SAGAW) — every translation faulted
// with context-entry-invalid and Driver-Cell DMA timed out under intel-iommu.
const CTX_AW_39BIT_HI: u64 = 0b001; // hi[2:0]
const CTX_PRESENT: u64 = 1;

// QEMU q35 hardcoded VT-d MMIO base (identity-mapped by init_kernel_paging_x86).
const VTD_BASE: usize = 0xFED9_0000;
const POLL_MAX: u64 = 1_000_000;

// ── Module-level state ────────────────────────────────────────────────────────

static VTD_ROOT_VIRT: AtomicUsize = AtomicUsize::new(0);
static VTD_ROOT_PHYS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
struct ContextTable {
    virt: usize,
    phys: u64,
    publication_flushed: bool,
}

/// Lazily allocated context tables, one 4 KiB page per populated PCI bus.
static VTD_CONTEXTS: Spinlock<BTreeMap<u8, ContextTable>> = Spinlock::new(BTreeMap::new());

/// Offset of IVA register within VT-d MMIO (computed from ECAP.IRO at init time).
static VTD_IVA_OFF: AtomicUsize = AtomicUsize::new(0);

/// Per-Cell VT-d domain (SLPT + DID + exact requester IDs).
struct VtdDomain {
    slpt: VtdSlpt,
    did: u16,
    bdfs: BTreeSet<u16>,
}

static VTD_DOMAINS: Spinlock<BTreeMap<u64, VtdDomain>> = Spinlock::new(BTreeMap::new());

/// Monotonically incrementing DID allocator (1-based; 0 = invalid).
static DID_COUNTER: AtomicU16 = AtomicU16::new(1);
/// Serializes register-based invalidations. A timed-out command may complete
/// later, so every successor must observe an idle register before publishing.
static VTD_INVALIDATION_LOCK: Spinlock<()> = Spinlock::new(());

#[inline]
pub(super) fn is_present() -> bool {
    VTD_ROOT_VIRT.load(Ordering::Relaxed) != 0
}

// ── MMIO helpers ─────────────────────────────────────────────────────────────

#[inline]
unsafe fn read64(base: usize, off: usize) -> u64 {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u64) }
}
#[inline]
unsafe fn read32(base: usize, off: usize) -> u32 {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}
#[inline]
unsafe fn write32(base: usize, off: usize, val: u32) {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}
#[inline]
unsafe fn write64(base: usize, off: usize, val: u64) {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, val) }
}

/// Convert a kernel heap virtual address to its physical address (x86_64 HHDM).
#[inline]
fn heap_to_phys(virt: usize) -> u64 {
    (virt - crate::memory::frame::phys_to_virt(0)) as u64
}

/// Allocate a zeroed 4 KiB page for an IOMMU table. Panics on OOM.
fn alloc_table() -> (usize, u64) {
    let layout = Layout::from_size_align(4096, 4096).expect("VT-d table layout");
    // SAFETY: layout is non-zero and 4096-aligned.
    let ptr = unsafe { alloc_zeroed(layout) } as usize;
    assert!(ptr != 0, "[vtd] OOM allocating IOMMU table");
    (ptr, heap_to_phys(ptr))
}

// ── Register invalidation helpers ────────────────────────────────────────────

#[inline]
const fn ctx_global_command() -> u64 {
    CCMD_ICC | CCMD_GLOBAL
}

#[inline]
const fn ctx_domain_command(did: u16) -> u64 {
    CCMD_ICC | CCMD_DOMAIN | did as u64
}

#[inline]
const fn iotlb_global_command() -> u64 {
    IOTLB_IVT | IOTLB_GLOBAL | IOTLB_DRD | IOTLB_DWD
}

#[inline]
const fn iotlb_domain_command(did: u16) -> u64 {
    IOTLB_IVT | IOTLB_DOMAIN | IOTLB_DRD | IOTLB_DWD | ((did as u64) << 32)
}

#[inline]
const fn iotlb_page_command(did: u16) -> u64 {
    IOTLB_IVT | IOTLB_PAGE | IOTLB_DRD | IOTLB_DWD | ((did as u64) << 32)
}

fn wait_for_invalidation(off: usize, busy: u64) -> bool {
    for _ in 0..POLL_MAX {
        // SAFETY: the caller has initialized the identity-mapped VT-d MMIO page.
        if unsafe { read64(VTD_BASE, off) } & busy == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn issue_context_invalidation(cmd: u64) -> bool {
    let _guard = VTD_INVALIDATION_LOCK.lock();
    if !wait_for_invalidation(VTD_CCMD, CCMD_ICC) {
        return false;
    }
    // SAFETY: VTD_BASE + VTD_CCMD is identity-mapped VT-d MMIO and the
    // serialized precheck proved no earlier command remains in flight.
    unsafe { write64(VTD_BASE, VTD_CCMD, cmd) };
    wait_for_invalidation(VTD_CCMD, CCMD_ICC)
}

fn issue_iotlb_invalidation(cmd: u64, iva: Option<u64>) -> bool {
    let iva_off = VTD_IVA_OFF.load(Ordering::Relaxed);
    if iva_off == 0 {
        return false;
    }
    let iotlb_off = iva_off + 8;
    let _guard = VTD_INVALIDATION_LOCK.lock();
    if !wait_for_invalidation(iotlb_off, IOTLB_IVT) {
        return false;
    }
    if let Some(iva) = iva {
        // SAFETY: IVA belongs to the same serialized IOTLB register bank.
        unsafe { write64(VTD_BASE, iva_off, iva) };
    }
    // SAFETY: VTD_BASE + iotlb_off is identity-mapped VT-d MMIO and the
    // serialized precheck proved no earlier command remains in flight.
    unsafe { write64(VTD_BASE, iotlb_off, cmd) };
    wait_for_invalidation(iotlb_off, IOTLB_IVT)
}

fn ctx_flush_global() -> bool {
    let completed = issue_context_invalidation(ctx_global_command());
    if !completed {
        log::warn!("[vtd] global context-cache flush timed out");
    }
    completed
}

fn ctx_flush_domain(did: u16) -> bool {
    let completed = issue_context_invalidation(ctx_domain_command(did));
    if !completed {
        log::warn!("[vtd] context-cache flush DID={} timed out", did);
    }
    completed
}

fn iotlb_flush_global() -> bool {
    let completed = issue_iotlb_invalidation(iotlb_global_command(), None);
    if !completed {
        log::warn!("[vtd] global IOTLB flush timed out");
    }
    completed
}

fn iotlb_flush_domain(did: u16) -> bool {
    let completed = issue_iotlb_invalidation(iotlb_domain_command(did), None);
    if !completed {
        log::warn!("[vtd] IOTLB flush DID={} timed out", did);
    }
    completed
}

/// Issue a page-selective IOTLB flush for a single page `iova` in domain `did`.
#[allow(dead_code)] // reason: awaits its Phase 02 caller (unmap_range_for_cell)
fn iotlb_flush_page(did: u16, iova: u64) -> bool {
    // IVA: bits[63:12] = page address, bits[5:0] = AM (0 = one page).
    let completed = issue_iotlb_invalidation(iotlb_page_command(did), Some(iova & !0xFFF));
    if !completed {
        log::warn!(
            "[vtd] IOTLB page flush DID={} iova={:#x} timed out",
            did,
            iova
        );
    }
    completed
}

// ── Root and context table helpers ───────────────────────────────────────────

#[inline]
const fn canonical_bdf(bus: u8, dev: u8, func: u8) -> u16 {
    ((bus as u16) << 8) | (((dev as u16) & 0x1F) << 3) | ((func as u16) & 0x07)
}

#[inline]
const fn bdf_parts(bdf: u16) -> (u8, u8, u8) {
    (
        (bdf >> 8) as u8,
        ((bdf >> 3) & 0x1F) as u8,
        (bdf & 0x07) as u8,
    )
}

#[inline]
const fn context_slot_offset(dev: u8, func: u8) -> usize {
    (((dev as usize) * 8) + (func as usize)) * 16
}

#[inline]
const fn context_slot(ctx_virt: usize, bdf: u16) -> usize {
    let (_, dev, func) = bdf_parts(bdf);
    ctx_virt + context_slot_offset(dev, func)
}

fn context_virt_for_bdf(contexts: &BTreeMap<u8, ContextTable>, bdf: u16) -> Option<usize> {
    let (bus, _, _) = bdf_parts(bdf);
    contexts.get(&bus).map(|table| table.virt)
}

fn for_each_domain_context_slot<F>(
    contexts: &BTreeMap<u8, ContextTable>,
    bdfs: &BTreeSet<u16>,
    mut visit: F,
) where
    F: FnMut(u16, usize),
{
    for &bdf in bdfs {
        if let Some(ctx_virt) = context_virt_for_bdf(contexts, bdf) {
            visit(bdf, context_slot(ctx_virt, bdf));
        }
    }
}

/// Write order mirrors context publication: high half first, present bit last.
unsafe fn write_root_entry(root_virt: usize, bus: u8, ctx_phys: u64) {
    let slot = root_virt + (bus as usize) * 16;
    // SAFETY: bus indexes one of the 256 entries in the 4 KiB root table.
    unsafe {
        core::ptr::write_volatile((slot + 8) as *mut u64, 0);
        fence(Ordering::Release);
        core::ptr::write_volatile(slot as *mut u64, ctx_phys | CTX_PRESENT);
    }
}

/// Return the bus's table only after a newly published root entry has been
/// globally invalidated. Keeping `publication_flushed=false` makes a timeout
/// retryable rather than letting a later mapping report a false success.
fn ensure_context_table(bus: u8) -> Option<usize> {
    let root_virt = VTD_ROOT_VIRT.load(Ordering::Relaxed);
    if root_virt == 0 {
        return None;
    }

    let mut contexts = VTD_CONTEXTS.lock();
    let table = contexts.entry(bus).or_insert_with(|| {
        let (virt, phys) = alloc_table();
        ContextTable {
            virt,
            phys,
            publication_flushed: false,
        }
    });

    if !table.publication_flushed {
        // SAFETY: both tables are 4 KiB pages allocated by alloc_table().
        unsafe { write_root_entry(root_virt, bus, table.phys) };
        let context_done = ctx_flush_global();
        let iotlb_done = iotlb_flush_global();
        if !(context_done && iotlb_done) {
            return None;
        }
        table.publication_flushed = true;
    }

    Some(table.virt)
}

/// Write a single VT-d context entry for one canonical requester ID.
///
/// Write order per VT-d spec §6.2.3.1: hi (DID) first, fence, lo (P=1) last.
unsafe fn write_ctx_entry(ctx_virt: usize, bdf: u16, slpt_phys: u64, did: u16) {
    let slot = context_slot(ctx_virt, bdf);
    let hi = ((did as u64) << 8) | CTX_AW_39BIT_HI;
    let lo = (slpt_phys & !0xFFF) | CTX_PRESENT;
    // SAFETY: slot is within the selected bus's 4 KiB context table.
    unsafe {
        core::ptr::write_volatile((slot + 8) as *mut u64, hi);
        fence(Ordering::Release);
        core::ptr::write_volatile(slot as *mut u64, lo);
    }
}

/// Zero an exact context slot, clearing P=0 before clearing its metadata.
unsafe fn clear_ctx_slot(slot: usize) {
    // SAFETY: the caller selected a slot in an allocated context table.
    unsafe {
        core::ptr::write_volatile(slot as *mut u64, 0);
        fence(Ordering::Release);
        core::ptr::write_volatile((slot + 8) as *mut u64, 0);
    }
}

// ── Phase 1: probe + allocate ─────────────────────────────────────────────────

/// Probe Intel VT-d; allocate the root table; compute the IOTLB register offset.
/// Context tables are allocated per bus on demand. Translation stays disabled.
pub(super) fn init_hw() {
    if crate::board::selected().soc != cellos_boards::SocId::QemuX86Q35 {
        log::warn!("[vtd] no DMAR-discovered register base; refusing q35 fallback");
        return;
    }

    // q35 exposes a single 4 KiB VT-d register page at this fallback address.
    // Map it only after validating the compiled board identity; other x86 boards
    // must discover their register base from DMAR before reaching this driver.
    crate::memory::paging::map_mmio_x86(VTD_BASE, 0x1000);
    // SAFETY: the q35 VT-d register page was identity-mapped immediately above.
    let gcap = unsafe { read64(VTD_BASE, VTD_GCAP) };
    if gcap == 0 || gcap == u64::MAX {
        log::info!("[vtd] Intel VT-d not present (GCAP={:#x})", gcap);
        return;
    }

    // GCAP.ND = bits[22:16]: number of supported domain IDs (ND+1 bits → 2^(ND+1) IDs).
    let nd = ((gcap >> 16) & 0x7F) as u32;
    let max_did: u32 = 1u32 << (nd + 1);
    log::info!(
        "[vtd] Intel VT-d found GCAP={:#x} ND={} max_did={}",
        gcap,
        nd,
        max_did
    );
    if max_did < 2 {
        log::warn!("[vtd] VT-d supports < 2 domains — per-Cell isolation disabled");
        return;
    }

    // Compute IOTLB register base from ECAP.IRO (bits[17:8]).
    // IOTLB_BASE = VTD_BASE + IRO * 16 (spec §10.4.8 IOTLB Invalidate Register).
    let ecap = unsafe { read64(VTD_BASE, VTD_ECAP) };
    let iro = ((ecap >> 8) & 0x3FF) as usize;
    let iva_off = iro * 16;
    VTD_IVA_OFF.store(iva_off, Ordering::Relaxed);
    log::info!("[vtd] ECAP={:#x} IRO={} IVA_OFF={:#x}", ecap, iro, iva_off);

    let (root_virt, root_phys) = alloc_table();
    VTD_ROOT_VIRT.store(root_virt, Ordering::Relaxed);
    VTD_ROOT_PHYS.store(root_phys as usize, Ordering::Relaxed);

    log::info!("[vtd] VT-d root allocated — DMA isolation pending activation");
}

// ── Phase 2: register DMA range (per-Cell) ───────────────────────────────────

/// Add [phys, phys+size) to the VT-d SLPT for Cell `tid` owning device `bdf`.
///
/// Creates a per-Cell domain on first call. Writes context entry for (bus, dev, func).
pub(super) fn map_range_for_cell(
    tid: u64,
    bdf: u32,
    phys: u64,
    size: usize,
) -> super::iommu::DmaMapResult {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;
    let bdf = canonical_bdf(bus, dev, func);
    let Some(ctx_virt) = ensure_context_table(bus) else {
        return super::iommu::DmaMapResult::Rejected;
    };

    let mut domains = VTD_DOMAINS.lock();
    // A requester ID can name only one domain. Remove stale ownership before
    // publishing a replacement entry so later teardown remains exact.
    for (&owner_tid, domain) in domains.iter_mut() {
        if owner_tid != tid {
            domain.bdfs.remove(&bdf);
        }
    }

    let entry = domains.entry(tid).or_insert_with(|| {
        let did = DID_COUNTER.fetch_add(1, Ordering::Relaxed);
        log::info!("[vtd] Cell {} allocated DID={}", tid, did);
        VtdDomain {
            slpt: VtdSlpt::new(),
            did,
            bdfs: BTreeSet::new(),
        }
    });

    entry.slpt.map_range(phys, size);
    entry.bdfs.insert(bdf);
    let did = entry.did;
    let slpt_phys = entry.slpt.root_phys();

    // SAFETY: ctx_virt is the 4 KiB context page selected by bdf.bus.
    unsafe { write_ctx_entry(ctx_virt, bdf, slpt_phys, did) };

    // Context ownership may replace an older DID for the same requester ID, so
    // invalidate the context cache globally before flushing the new domain's
    // translations. CM hardware can cache not-present results as well.
    let context_done = ctx_flush_global();
    let iotlb_done = iotlb_flush_domain(did);
    if !(context_done && iotlb_done) {
        return super::iommu::DmaMapResult::PublishedUnconfirmed;
    }

    log::info!(
        "[vtd] Cell {} BDF {:02x}:{:02x}.{} DID={} SLPT={:#x}",
        tid,
        bus,
        dev,
        func,
        did,
        slpt_phys
    );
    super::iommu::DmaMapResult::Mapped(phys)
}

/// Backward-compat wrapper: kernel domain (tid=0, bdf=0) → map in tid=0 domain.
#[allow(dead_code)] // reason: kept for API parity with iommu_riscv; no caller wired up yet
pub(super) fn map_range(phys: u64, size: usize) {
    map_range_for_cell(0, 0, phys, size);
}

// ── Phase 3: activate enforcement ────────────────────────────────────────────

/// Install the lazily populated root table, invalidate stale walks, then enable TE.
///
/// Unpopulated bus root entries remain not-present.
pub(super) fn activate() {
    let root_virt = VTD_ROOT_VIRT.load(Ordering::Relaxed);
    let root_phys = VTD_ROOT_PHYS.load(Ordering::Relaxed) as u64;
    if root_virt == 0 {
        return;
    } // VT-d not present

    // Serialize activation with late per-bus root publication.
    let mut contexts = VTD_CONTEXTS.lock();
    for (&bus, table) in contexts.iter_mut() {
        if !table.publication_flushed {
            // SAFETY: root_virt and table.phys name allocated 4 KiB pages.
            unsafe { write_root_entry(root_virt, bus, table.phys) };
            let context_done = ctx_flush_global();
            let iotlb_done = iotlb_flush_global();
            if !(context_done && iotlb_done) {
                return;
            }
            table.publication_flushed = true;
        }
    }

    // Step 1: programme root table address.
    // SAFETY: VTD_BASE is identity-mapped; root_phys is 4096-aligned.
    unsafe {
        write64(VTD_BASE, VTD_RTADDR, root_phys);
    }

    // Step 2: GCMD.SRTP → poll GSTS.RTPS.
    unsafe {
        write32(VTD_BASE, VTD_GCMD, SRTP);
    }
    let mut n = 0u64;
    loop {
        if unsafe { read32(VTD_BASE, VTD_GSTS) } & SRTP != 0 {
            break;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] GSTS.RTPS never set — aborting");
            return;
        }
        core::hint::spin_loop();
    }

    // Step 3: discard any context/root misses and stale translations associated
    // with the newly installed root pointer before enabling translation.
    let context_done = ctx_flush_global();
    let iotlb_done = iotlb_flush_global();
    if !(context_done && iotlb_done) {
        log::warn!("[vtd] root-table invalidation incomplete — aborting");
        return;
    }

    // Step 4: GCMD.(TE|SRTP) → poll GSTS.TES.
    unsafe {
        write32(VTD_BASE, VTD_GCMD, TE | SRTP);
    }
    let mut n = 0u64;
    loop {
        if unsafe { read32(VTD_BASE, VTD_GSTS) } & TE != 0 {
            break;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] GSTS.TES never set — translation NOT active");
            return;
        }
        core::hint::spin_loop();
    }

    super::iommu::set_active();
    // `warn!` — activation happens post-scheduler (deferred init fires from the
    // Platform Cell's RegisterPciDevice), after the kernel log level drops to
    // Warn. One-time boot-integrity event + the nic_x86_vtd_enabled test oracle.
    log::warn!("[vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT)");
}

// ── Cell exit: exact context cleanup, then cache drains ───────────────────────

fn clear_domain_contexts(bdfs: &BTreeSet<u16>, did: u16) {
    let contexts = VTD_CONTEXTS.lock();
    for_each_domain_context_slot(&contexts, bdfs, |_bdf, slot| {
        // Guard against an ownership invariant violation: never clear a slot
        // that has since been published for another domain.
        // SAFETY: slot was selected from an allocated 4 KiB context table.
        let lo = unsafe { core::ptr::read_volatile(slot as *const u64) };
        // SAFETY: the high half belongs to the same 16-byte context entry.
        let hi = unsafe { core::ptr::read_volatile((slot + 8) as *const u64) };
        let entry_did = ((hi >> 8) & 0xFFFF) as u16;
        if lo & CTX_PRESENT != 0 && entry_did == did {
            // SAFETY: slot is the exact tracked requester entry for this domain.
            unsafe { clear_ctx_slot(slot) };
        }
    });
}

fn run_teardown_sequence<C, X, I>(mut clear: C, mut flush_context: X, mut flush_iotlb: I) -> bool
where
    C: FnMut(),
    X: FnMut() -> bool,
    I: FnMut() -> bool,
{
    clear();
    let context_done = flush_context();
    // Always drain the IOTLB even if context invalidation timed out.
    let iotlb_done = flush_iotlb();
    context_done && iotlb_done
}

/// Clear Cell `tid`'s exact requester entries, then invalidate its domain.
///
/// Call on Cell exit BEFORE DMA frames are returned to the frame allocator.
pub(super) fn unmap_cell_domain(tid: u64) -> bool {
    let mut domains = VTD_DOMAINS.lock();
    let Some(domain) = domains.remove(&tid) else {
        return true;
    };
    let did = domain.did;

    let completed = run_teardown_sequence(
        || clear_domain_contexts(&domain.bdfs, did),
        || ctx_flush_domain(did),
        || iotlb_flush_domain(did),
    );
    if !completed {
        // Cached translations may still reference the SLPT after a timeout.
        // Keep the domain and its page tables alive so teardown can be retried.
        domains.insert(tid, domain);
        log::warn!(
            "[vtd] Cell {} DID={} teardown invalidation incomplete; domain retained",
            tid,
            did
        );
        false
    } else {
        log::info!(
            "[vtd] Cell {} DID={} contexts cleared + caches drained",
            tid,
            did
        );
        true
    }
}

/// Issue a page-selective IOTLB flush for a specific IOVA owned by `tid`.
#[allow(dead_code)] // reason: finer-grained per-IOVA unmap; iommu.rs currently only wires full-cell unmap_cell_domain (Phase 02)
pub(super) fn unmap_range_for_cell(tid: u64, iova: u64, _size: usize) {
    let domains = VTD_DOMAINS.lock();
    if let Some(domain) = domains.get(&tid) {
        iotlb_flush_page(domain.did, iova);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    fn fake_context(virt: usize) -> ContextTable {
        ContextTable {
            virt,
            phys: virt as u64,
            publication_flushed: true,
        }
    }

    #[test]
    fn same_devfn_on_distinct_buses_selects_distinct_context_pages() {
        let bdf0 = canonical_bdf(0, 7, 3);
        let bdf1 = canonical_bdf(1, 7, 3);
        let mut contexts = BTreeMap::new();
        contexts.insert(0, fake_context(0x10_0000));
        contexts.insert(1, fake_context(0x20_0000));

        let ctx0 = context_virt_for_bdf(&contexts, bdf0).unwrap();
        let ctx1 = context_virt_for_bdf(&contexts, bdf1).unwrap();
        assert_eq!(bdf0, (7 << 3) | 3);
        assert_eq!(bdf1, (1 << 8) | (7 << 3) | 3);
        assert_ne!(ctx0, ctx1);
        assert_eq!(
            context_slot(ctx0, bdf0) - ctx0,
            context_slot(ctx1, bdf1) - ctx1
        );
        assert_ne!(context_slot(ctx0, bdf0), context_slot(ctx1, bdf1));
        assert_eq!(context_slot_offset(0, 0), 0);
        assert_eq!(context_slot_offset(7, 3), (7 * 8 + 3) * 16);
        assert_eq!(context_slot_offset(31, 7), 4096 - 16);
    }

    #[test]
    fn invalidation_commands_use_register_granularity_and_did_fields() {
        let did = 0xA55A;
        assert_eq!(ctx_global_command(), (1u64 << 63) | (1u64 << 61));
        assert_eq!(
            ctx_domain_command(did),
            (1u64 << 63) | (2u64 << 61) | did as u64
        );
        assert_eq!(
            iotlb_global_command(),
            (1u64 << 63) | (1u64 << 60) | (1u64 << 49) | (1u64 << 48)
        );
        assert_eq!(
            iotlb_domain_command(did),
            (1u64 << 63) | (2u64 << 60) | (1u64 << 49) | (1u64 << 48) | ((did as u64) << 32)
        );
    }

    #[test]
    fn exact_bdf_selection_does_not_touch_same_devfn_on_another_bus() {
        let bus0 = canonical_bdf(0, 4, 1);
        let bus1 = canonical_bdf(1, 4, 1);
        let mut contexts = BTreeMap::new();
        contexts.insert(0, fake_context(0x30_0000));
        contexts.insert(1, fake_context(0x40_0000));
        let mut owned = BTreeSet::new();
        owned.insert(bus1);

        let selected = Cell::new(0usize);
        let count = Cell::new(0usize);
        for_each_domain_context_slot(&contexts, &owned, |bdf, slot| {
            assert_eq!(bdf, bus1);
            selected.set(slot);
            count.set(count.get() + 1);
        });

        assert_eq!(count.get(), 1);
        assert_eq!(selected.get(), context_slot(0x40_0000, bus1));
        assert_ne!(selected.get(), context_slot(0x30_0000, bus0));
    }

    #[test]
    fn teardown_clears_then_invalidates_context_then_drains_iotlb() {
        let stage = Cell::new(0u8);
        let completed = run_teardown_sequence(
            || {
                assert_eq!(stage.get(), 0);
                stage.set(1);
            },
            || {
                assert_eq!(stage.get(), 1);
                stage.set(2);
                false
            },
            || {
                assert_eq!(stage.get(), 2);
                stage.set(3);
                true
            },
        );

        assert!(!completed);
        assert_eq!(stage.get(), 3);
    }
}
