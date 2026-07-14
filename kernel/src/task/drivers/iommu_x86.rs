//! Intel VT-d DMA isolation driver for x86_64 — per-Cell domain isolation.
//!
//! Phase 1 `init_hw()`:             probe VT-d (GCAP), allocate root/context tables.
//! Phase 2 `map_range_for_cell()`:  create per-Cell SLPT + DID; write context entry.
//! Phase 3 `activate()`:            enable VT-d translation (TE).
//!
//! Per-Cell domains: each tid with DMA capability gets its own `VtdSlpt` and unique
//! DID (Domain ID). Context entries point to the owning Cell's SLPT; DMA outside that
//! SLPT triggers a VT-d fault.

use super::iommu_pt::VtdSlpt;
use crate::sync::Spinlock;
use alloc::alloc::{alloc_zeroed, Layout};
use alloc::collections::BTreeMap;
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

// CCMD bits
const CCMD_ICC: u64 = 1u64 << 63; // Invalidate Context-Cache (trigger + status)
const CCMD_DSI: u64 = 0b01 << 61; // Domain-selective invalidation (CIRG)

// IOTLB invalidation command bits (written to IOTLB register = IOTLB_BASE + 8)
// Bit 63: IVT (Invalidate IOTLB; 1=trigger, cleared when done)
// Bits [49:48]: DRD=1 (drain reads), DWD=1 (drain writes)
// Bits [47:32]: DID
// Bits [5:4]: IIRG (00=global, 01=domain, 10=page-selective)
const IOTLB_IVT: u64 = 1u64 << 63;
const IOTLB_DRD: u64 = 1u64 << 49;
const IOTLB_DWD: u64 = 1u64 << 48;
const IOTLB_DSI: u64 = 0b01 << 4; // domain-selective
#[allow(dead_code)] // reason: page-selective flush path (iotlb_flush_page) awaits its Phase 02 caller
const IOTLB_PSI: u64 = 0b10 << 4; // page-selective

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
static VTD_CTX_VIRT: AtomicUsize = AtomicUsize::new(0);

/// Offset of IVA register within VT-d MMIO (computed from ECAP.IRO at init time).
static VTD_IVA_OFF: AtomicUsize = AtomicUsize::new(0);

/// Per-Cell VT-d domain (SLPT + DID).
struct VtdDomain {
    slpt: VtdSlpt,
    did: u16,
}

static VTD_DOMAINS: Spinlock<BTreeMap<u64, VtdDomain>> = Spinlock::new(BTreeMap::new());

/// Monotonically incrementing DID allocator (1-based; 0 = invalid).
static DID_COUNTER: AtomicU16 = AtomicU16::new(1);

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

// ── IOTLB invalidation helpers ────────────────────────────────────────────────

/// Issue a domain-selective IOTLB flush for `did`.
fn iotlb_flush_domain(did: u16) {
    let iva_off = VTD_IVA_OFF.load(Ordering::Relaxed);
    if iva_off == 0 {
        return;
    } // VT-d not present

    let iotlb_off = iva_off + 8;
    let cmd = IOTLB_IVT | IOTLB_DSI | IOTLB_DRD | IOTLB_DWD | ((did as u64) << 32);
    // SAFETY: VTD_BASE + iotlb_off is identity-mapped VT-d MMIO.
    unsafe { write64(VTD_BASE, iotlb_off, cmd) };
    let mut n = 0u64;
    loop {
        // SAFETY: read VT-d IOTLB register.
        if unsafe { read64(VTD_BASE, iotlb_off) } & IOTLB_IVT == 0 {
            break;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] IOTLB DSI flush DID={} timed out", did);
            break;
        }
        core::hint::spin_loop();
    }
}

/// Issue a page-selective IOTLB flush for a single page `iova` in domain `did`.
#[allow(dead_code)] // reason: awaits its Phase 02 caller (unmap_range_for_cell)
fn iotlb_flush_page(did: u16, iova: u64) {
    let iva_off = VTD_IVA_OFF.load(Ordering::Relaxed);
    if iva_off == 0 {
        return;
    }

    let iotlb_off = iva_off + 8;
    // IVA: bits[63:12] = page addr, bits[5:0] = AM (0 = single page)
    let iva_val = iova & !0xFFF; // AM=0
                                 // SAFETY: VTD_BASE + iva_off is identity-mapped VT-d MMIO.
    unsafe { write64(VTD_BASE, iva_off, iva_val) };
    let cmd = IOTLB_IVT | IOTLB_PSI | IOTLB_DRD | IOTLB_DWD | ((did as u64) << 32);
    // SAFETY: VTD_BASE + iotlb_off is identity-mapped VT-d MMIO.
    unsafe { write64(VTD_BASE, iotlb_off, cmd) };
    let mut n = 0u64;
    loop {
        if unsafe { read64(VTD_BASE, iotlb_off) } & IOTLB_IVT == 0 {
            break;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!(
                "[vtd] IOTLB PSI flush DID={} iova={:#x} timed out",
                did,
                iova
            );
            break;
        }
        core::hint::spin_loop();
    }
}

/// Issue a domain-selective context-cache flush for `did`, then drain.
fn ctx_flush_domain(did: u16) {
    let cmd = CCMD_ICC | CCMD_DSI | ((did as u64) << 32);
    // SAFETY: VTD_BASE + VTD_CCMD is identity-mapped VT-d MMIO.
    unsafe { write64(VTD_BASE, VTD_CCMD, cmd) };
    let mut n = 0u64;
    loop {
        // SAFETY: read back ICC bit.
        if unsafe { read64(VTD_BASE, VTD_CCMD) } & CCMD_ICC == 0 {
            break;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] context-cache flush DID={} timed out", did);
            break;
        }
        core::hint::spin_loop();
    }
}

// ── Context entry write ───────────────────────────────────────────────────────

/// Write a single VT-d context entry for (bus, dev, func) → (slpt_phys, did).
///
/// Write order per VT-d spec §6.2.3.1: hi (DID) first, fence, lo (P=1) last.
unsafe fn write_ctx_entry(ctx_virt: usize, _bus: u8, dev: u8, func: u8, slpt_phys: u64, did: u16) {
    // Context table layout: 256 bus×32 devices×8 funcs, each entry = 16 bytes.
    // With one shared context table for all buses (Phase 01 limitation), index by dev+func only.
    let i = (dev as usize) * 8 + (func as usize);
    let slot = ctx_virt + i * 16;
    let hi = ((did as u64) << 8) | CTX_AW_39BIT_HI;
    let lo = (slpt_phys & !0xFFF) | CTX_PRESENT;
    // SAFETY: slot is within the 4096-B context table page.
    unsafe {
        core::ptr::write_volatile((slot + 8) as *mut u64, hi); // hi first (DID)
        fence(Ordering::Release);
        core::ptr::write_volatile(slot as *mut u64, lo); // lo last (P=1)
    }
}

/// Zero a context entry, clearing P=0 so VT-d treats it as invalid.
unsafe fn clear_ctx_entry(ctx_virt: usize, dev: u8, func: u8) {
    let i = (dev as usize) * 8 + (func as usize);
    let slot = ctx_virt + i * 16;
    // SAFETY: slot is within the 4096-B context table page.
    unsafe {
        core::ptr::write_volatile(slot as *mut u64, 0u64);
        core::ptr::write_volatile((slot + 8) as *mut u64, 0u64);
    }
}

// ── Phase 1: probe + allocate ─────────────────────────────────────────────────

/// Probe Intel VT-d; allocate root + context tables; compute IOTLB register offset.
/// Does NOT enable VT-d translation — stays silent until `activate()`.
pub(super) fn init_hw() {
    // SAFETY: VTD_BASE (0xFED90000) is identity-mapped by init_kernel_paging_x86.
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
    let (ctx_virt, _ctx_phys) = alloc_table();

    VTD_ROOT_VIRT.store(root_virt, Ordering::Relaxed);
    VTD_ROOT_PHYS.store(root_phys as usize, Ordering::Relaxed);
    VTD_CTX_VIRT.store(ctx_virt, Ordering::Relaxed);

    log::info!("[vtd] VT-d structures allocated — DMA isolation pending activation");
}

// ── Phase 2: register DMA range (per-Cell) ───────────────────────────────────

/// Add [phys, phys+size) to the VT-d SLPT for Cell `tid` owning device `bdf`.
///
/// Creates a per-Cell domain on first call. Writes context entry for (bus, dev, func).
pub(super) fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) {
    let ctx_virt = VTD_CTX_VIRT.load(Ordering::Relaxed);
    if ctx_virt == 0 {
        return;
    } // VT-d not present

    let mut domains = VTD_DOMAINS.lock();
    let entry = domains.entry(tid).or_insert_with(|| {
        let did = DID_COUNTER.fetch_add(1, Ordering::Relaxed);
        log::info!("[vtd] Cell {} allocated DID={}", tid, did);
        VtdDomain {
            slpt: VtdSlpt::new(),
            did,
        }
    });

    entry.slpt.map_range(phys, size);

    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;
    let did = entry.did;
    let slpt_phys = entry.slpt.root_phys();

    // SAFETY: ctx_virt is a 4 KiB-aligned page allocated in init_hw().
    unsafe {
        write_ctx_entry(ctx_virt, bus, dev, func, slpt_phys, did);
    }

    // Context-cache flush so hardware sees the new entry before the first DMA.
    ctx_flush_domain(did);
    // IOTLB domain flush: QEMU intel-iommu runs with Caching Mode (CM=1), which
    // caches not-present/faulting walks too — SLPT entries added after TE=1 are
    // invisible to the device until the domain IOTLB is invalidated.
    iotlb_flush_domain(did);
    log::info!(
        "[vtd] Cell {} BDF {:02x}:{:02x}.{} DID={} SLPT={:#x}",
        tid,
        bus,
        dev,
        func,
        did,
        slpt_phys
    );
}

/// Backward-compat wrapper: kernel domain (tid=0, bdf=0) → map in tid=0 domain.
#[allow(dead_code)] // reason: kept for API parity with iommu_riscv; no caller wired up yet
pub(super) fn map_range(phys: u64, size: usize) {
    map_range_for_cell(0, 0, phys, size);
}

// ── Phase 3: activate enforcement ────────────────────────────────────────────

/// Fill VT-d root table (all bus entries → shared context table), then enable TE.
///
/// Context entries start all-zero (P=0 = invalid). Only DMA-active Cells have
/// entries filled via `map_range_for_cell`.
pub(super) fn activate() {
    let root_virt = VTD_ROOT_VIRT.load(Ordering::Relaxed);
    let root_phys = VTD_ROOT_PHYS.load(Ordering::Relaxed) as u64;
    let ctx_virt = VTD_CTX_VIRT.load(Ordering::Relaxed);
    if root_virt == 0 {
        return;
    } // VT-d not present

    // Root table: all 256 bus entries point to the shared context table.
    // Context entries are P=0 by default; only DMA-active entries are P=1.
    let ctx_phys = heap_to_phys(ctx_virt);
    for i in 0usize..256 {
        let slot = root_virt + i * 16;
        // SAFETY: root_virt is a 4096-B page; i*16 < 4096.
        unsafe {
            core::ptr::write_volatile(slot as *mut u64, ctx_phys | CTX_PRESENT);
            core::ptr::write_volatile((slot + 8) as *mut u64, 0u64);
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

    // Step 3: GCMD.(TE|SRTP) → poll GSTS.TES.
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

// ── Cell exit: DSI flush + context entry cleanup ──────────────────────────────

/// Flush IOTLB for Cell `tid`'s domain and zero its context entry.
///
/// Call on Cell exit BEFORE DMA frames are returned to the frame allocator.
pub(super) fn unmap_cell_domain(tid: u64) {
    let ctx_virt = VTD_CTX_VIRT.load(Ordering::Relaxed);
    let mut domains = VTD_DOMAINS.lock();
    let Some(domain) = domains.remove(&tid) else {
        return;
    };
    let did = domain.did;

    // DSI IOTLB flush so hardware stops accepting DMA for this domain.
    iotlb_flush_domain(did);

    // Zero all context entries with this DID (scan the 256-entry table).
    // Simplification: scan all entries (O(256)); G2 would use a BDF→DID reverse map.
    if ctx_virt != 0 {
        for dev in 0u8..32 {
            for func in 0u8..8 {
                let i = (dev as usize) * 8 + (func as usize);
                let slot = ctx_virt + i * 16;
                // SAFETY: slot is within the 4 KiB context table page.
                let entry_hi = unsafe { core::ptr::read_volatile((slot + 8) as *const u64) };
                let entry_did = ((entry_hi >> 8) & 0xFFFF) as u16;
                if entry_did == did {
                    // SAFETY: slot is within the 4 KiB context table page.
                    unsafe {
                        clear_ctx_entry(ctx_virt, dev, func);
                    }
                }
            }
        }
        // Context-cache flush to propagate zeroed entries.
        ctx_flush_domain(did);
    }

    log::info!(
        "[vtd] Cell {} DID={} IOTLB flushed + context zeroed",
        tid,
        did
    );
}

/// Issue a page-selective IOTLB flush for a specific IOVA owned by `tid`.
#[allow(dead_code)] // reason: finer-grained per-IOVA unmap; iommu.rs currently only wires full-cell unmap_cell_domain (Phase 02)
pub(super) fn unmap_range_for_cell(tid: u64, iova: u64, _size: usize) {
    let domains = VTD_DOMAINS.lock();
    if let Some(domain) = domains.get(&tid) {
        iotlb_flush_page(domain.did, iova);
    }
}
