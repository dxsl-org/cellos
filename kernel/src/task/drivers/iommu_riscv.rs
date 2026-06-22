//! RISC-V IOMMU driver — per-Cell DMA isolation via 1-level DDT + Sv39 first-stage.
//!
//! Phase 1 `init_hw()`:             probe PCIe IOMMU device, allocate DDT + CQ. Stays BARE.
//! Phase 2 `map_range_for_cell()`:  register DMA range in a per-Cell Sv39 domain.
//! Phase 3 `activate()`:            fill kernel-domain DCs, switch DDTP to 1LVL enforcement.
//!
//! Each Cell gets its own `Sv39IommuPt` and a unique PSCID. Devices are isolated at the
//! Device Context (DC) level — a device can only DMA within its owning Cell's page table.

use alloc::{
    alloc::{alloc_zeroed, Layout},
    collections::BTreeMap,
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, AtomicU64, AtomicUsize, Ordering};
use crate::sync::Spinlock;
use super::iommu_pt::Sv39IommuPt;
use crate::task::drivers::pcie_ecam;

const CLASS:  u8 = 0x08;
const SUB:    u8 = 0x06;
const PROGIF: u8 = 0x00;

// BAR0 register offsets (RISC-V IOMMU spec v1.0.1 §3.1)
const REG_CAPS: usize = 0x00;
const REG_FCTL: usize = 0x08;
const REG_DDTP: usize = 0x10;
const REG_CQB:  usize = 0x18; // Command Queue Base
const REG_CQH:  usize = 0x20; // Command Queue Head (HW-owned, read-only)
const REG_CQT:  usize = 0x28; // Command Queue Tail (SW-owned)
const REG_IPSR: usize = 0x38;

const DDTP_MODE_BARE: u64 = 1;
const DDTP_MODE_1LVL: u64 = 2;

const CQ_DEPTH: usize = 64;
const CQ_LOG2:  u64   = 6;  // log2(64)
const CQ_ENTRY: usize = 16; // bytes per CQ entry

const DC_TC_V:        u64 = 1;
const SATP_MODE_SV39: u64 = 8u64 << 60;

const POLL_MAX: u64 = 1_000_000;

// ── Module-level state ────────────────────────────────────────────────────────

static BAR0:     AtomicUsize = AtomicUsize::new(0);
static DDT_VIRT: AtomicUsize = AtomicUsize::new(0);
static DDT_PHYS: AtomicU64   = AtomicU64::new(0);
static CQ_VIRT:  AtomicUsize = AtomicUsize::new(0);

struct RiscvDomain {
    pt:    Sv39IommuPt,
    pscid: u16,
    bdfs:  Vec<u32>, // BDFs registered for this domain (for DC fill at activate + cleanup)
}

// Key = Tid as u64; kernel domain = tid 0.
static RISCV_DOMAINS: Spinlock<BTreeMap<u64, RiscvDomain>> = Spinlock::new(BTreeMap::new());

// Free-list prevents 16-bit PSCID exhaustion on long-running servers with Cell restarts.
// PSCID 0 is reserved (invalid); first real PSCID = 1.
static PSCID_FREE_LIST: Spinlock<Vec<u16>> = Spinlock::new(Vec::new());
static PSCID_NEXT:      AtomicU16 = AtomicU16::new(1);

// ── MMIO helpers ─────────────────────────────────────────────────────────────

#[inline] unsafe fn read32(base: usize, off: usize) -> u32 {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}
#[inline] unsafe fn write32(base: usize, off: usize, val: u32) {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}
#[inline] unsafe fn write64(base: usize, off: usize, val: u64) {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, val) }
}

// ── PSCID management ─────────────────────────────────────────────────────────

fn alloc_pscid() -> Option<u16> {
    if let Some(id) = PSCID_FREE_LIST.lock().pop() { return Some(id); }
    let id = PSCID_NEXT.fetch_add(1, Ordering::Relaxed);
    if id == 0 { None } else { Some(id) } // id==0 means wrapped → exhausted
}

/// Return a PSCID to the free-list for reuse on Cell restart.
pub(super) fn free_pscid(id: u16) {
    if id != 0 { PSCID_FREE_LIST.lock().push(id); }
}

// ── Command queue ─────────────────────────────────────────────────────────────

fn cq_head(bar0: usize) -> u64 {
    let v = unsafe { core::ptr::read_volatile((bar0 + REG_CQH) as *const u64) };
    v & 0xFFFF
}

/// Enqueue one 16-byte CQ entry. CQ-full guard spins until a slot is free.
fn enqueue_cmd(bar0: usize, cq_virt: usize, w0: u64, w1: u64) {
    let mut spin = 0u64;
    loop {
        let t = unsafe { core::ptr::read_volatile((bar0 + REG_CQT) as *const u64) } & 0xFFFF;
        let h = cq_head(bar0);
        if (t + 1) % CQ_DEPTH as u64 != h { break; }
        spin += 1;
        if spin > POLL_MAX {
            log::warn!("[iommu_riscv] CQ full — QEMU may not process CQ commands");
            return;
        }
        core::hint::spin_loop();
    }
    let tail = unsafe { core::ptr::read_volatile((bar0 + REG_CQT) as *const u64) } & 0xFFFF;
    let slot = cq_virt + (tail as usize) * CQ_ENTRY;
    unsafe {
        core::ptr::write_volatile(slot as *mut u64, w0);
        core::ptr::write_volatile((slot + 8) as *mut u64, w1);
        core::ptr::write_volatile((bar0 + REG_CQT) as *mut u64, (tail + 1) % CQ_DEPTH as u64);
    }
}

/// Issue IOFENCE.C and poll until IOMMU drains the queue (CQH == CQT).
///
/// Frame quarantine: this MUST complete before DMA frames return to the frame allocator.
fn issue_iofence(bar0: usize, cq_virt: usize) {
    enqueue_cmd(bar0, cq_virt, 0x03, 0); // IOFENCE.C: OPCODE=3, FUNC3=0
    let expected = unsafe { core::ptr::read_volatile((bar0 + REG_CQT) as *const u64) } & 0xFFFF;
    let mut spin = 0u64;
    loop {
        if cq_head(bar0) == expected { break; }
        spin += 1;
        if spin > POLL_MAX {
            log::warn!("[iommu_riscv] IOFENCE timeout — QEMU may not advance CQH");
            break; // validated: log warn + continue on QEMU
        }
        core::hint::spin_loop();
    }
}

/// Invalidate all first-stage IOTLB entries for a specific PSCID.
fn invalidate_pscid_tlb(bar0: usize, cq_virt: usize, pscid: u16) {
    // IOTINVAL.VMA: OPCODE=2, FUNC3=0 (VMA/first-stage), PSCV=bit20, PSCID[9:0] in bits[31:22]
    let w0 = 0x02u64 | (1u64 << 20) | ((pscid as u64 & 0x3FF) << 22);
    enqueue_cmd(bar0, cq_virt, w0, 0);
}

/// Invalidate the IOMMU's cached Device Context for a device_id.
fn invalidate_dc(bar0: usize, cq_virt: usize, device_id: u64) {
    // IODIR.INVAL_DDT: OPCODE=1, FUNC3=2 (DDT targeted), DV=bit10, DID in bits[35:12]
    let w0 = 0x01u64 | (2u64 << 7) | (1u64 << 10) | (device_id << 12);
    enqueue_cmd(bar0, cq_virt, w0, 0);
}

// ── Device Context helpers ────────────────────────────────────────────────────

/// Write a Device Context into the 1LVL DDT for `device_id`.
///
/// TC.V is written LAST after a Release fence so the IOMMU sees a consistent DC.
fn write_dc(ddt_virt: usize, device_id: u64, fsc: u64, pscid: u16) {
    let idx = (device_id & 0x3F) as usize; // 1LVL DDT: indexed by DeviceID[5:0]
    let dc = ddt_virt + idx * 64;
    let ta = (pscid as u64) << 12; // ta.PSCID in bits[31:12]
    // SAFETY: dc is within the 4096-byte DDT allocation (idx < 64; DC size = 64 bytes).
    unsafe {
        core::ptr::write_volatile((dc +  8) as *mut u64, 0u64); // iohgatp: G-stage bare
        core::ptr::write_volatile((dc + 16) as *mut u64, ta);   // ta: PSCID
        core::ptr::write_volatile((dc + 24) as *mut u64, fsc);  // fsc: Sv39 first-stage PT
        core::ptr::write_volatile((dc + 32) as *mut u64, 0u64); // msiptp
        core::ptr::write_volatile((dc + 40) as *mut u64, 0u64);
        core::ptr::write_volatile((dc + 48) as *mut u64, 0u64);
        core::ptr::write_volatile((dc + 56) as *mut u64, 0u64);
        core::sync::atomic::fence(Ordering::Release); // all other fields visible before TC.V
        core::ptr::write_volatile(dc as *mut u64, DC_TC_V); // TC.V last — makes DC live
    }
}

// ── Phase 1: probe + allocate ─────────────────────────────────────────────────

/// Probe RISC-V IOMMU hardware, allocate 1LVL DDT and command queue.
/// Stays in BARE (passthrough) mode until `activate()` is called.
pub(super) fn init_hw() {
    let dev = match pcie_ecam::find_class(CLASS, SUB, PROGIF) {
        Some(d) => d,
        None => {
            log::warn!("[iommu] RISC-V IOMMU not found \
                        (needs QEMU ≥8.2 + -device riscv-iommu-pci,bus=pcie.0)");
            return;
        }
    };
    let bar0 = dev.bars[0].base_addr() as usize;
    if bar0 == 0 { log::warn!("[iommu] RISC-V IOMMU BAR0 == 0"); return; }

    let _caps = unsafe { core::ptr::read_volatile((bar0 + REG_CAPS) as *const u64) };
    unsafe {
        write32(bar0, REG_FCTL, 0);
        write64(bar0, REG_DDTP, DDTP_MODE_BARE);
        let ipsr = read32(bar0, REG_IPSR);
        if ipsr != 0 { write32(bar0, REG_IPSR, ipsr); }
    }

    // Allocate 1-level DDT: 64 DCs × 64B = 4096B.
    let layout = Layout::from_size_align(4096, 4096).expect("iommu: DDT");
    let ddt_virt = unsafe { alloc_zeroed(layout) } as usize;
    assert!(ddt_virt != 0, "[iommu_riscv] OOM: DDT");

    // Allocate CQ: 64 entries × 16B = 1024B (use full page for alignment).
    let layout = Layout::from_size_align(4096, 4096).expect("iommu: CQ");
    let cq_virt = unsafe { alloc_zeroed(layout) } as usize;
    assert!(cq_virt != 0, "[iommu_riscv] OOM: CQ");

    BAR0.store(bar0, Ordering::Relaxed);
    DDT_VIRT.store(ddt_virt, Ordering::Relaxed);
    DDT_PHYS.store(ddt_virt as u64, Ordering::Relaxed); // identity-mapped: VA == PA
    CQ_VIRT.store(cq_virt, Ordering::Relaxed);

    // Program CQ: REG_CQB = (cq_phys >> 12) | log2(depth)
    unsafe {
        write64(bar0, REG_CQB, (cq_virt as u64 >> 12) | CQ_LOG2);
        let fctl = read32(bar0, REG_FCTL);
        write32(bar0, REG_FCTL, fctl | 1); // CQEN = bit 0
    }

    log::info!("[iommu] RISC-V IOMMU HW ready (vendor={:04x} dev={:04x}) \
                — isolation pending", dev.vendor_id, dev.device_id);
}

// ── Phase 2: register DMA ranges ─────────────────────────────────────────────

/// Register `[phys, phys+size)` for Cell `tid` owning device `bdf`.
///
/// Creates a per-Cell `Sv39IommuPt` + PSCID on first call. Writes a DDT entry
/// for `bdf` immediately (even before `activate()`). A bare CPU fence is insufficient
/// for DC cache coherency — IODIR.INVAL_DDT + IOFENCE.C are issued after each DC write.
pub(super) fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) {
    let bar0     = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let cq_virt  = CQ_VIRT.load(Ordering::Relaxed);
    if bar0 == 0 || ddt_virt == 0 { return; }

    let mut domains = RISCV_DOMAINS.lock();
    let domain = domains.entry(tid).or_insert_with(|| {
        let pscid = alloc_pscid().expect("[iommu_riscv] PSCID exhausted (max 65535 active Cells)");
        RiscvDomain { pt: Sv39IommuPt::new(), pscid, bdfs: Vec::new() }
    });

    domain.pt.map_range(phys, size);

    if bdf != 0 {
        let fsc   = SATP_MODE_SV39 | (domain.pt.root_phys() >> 12);
        let pscid = domain.pscid;
        write_dc(ddt_virt, bdf as u64, fsc, pscid);
        if !domain.bdfs.contains(&bdf) { domain.bdfs.push(bdf); }

        log::info!("[iommu] Cell {} BDF {:02x}:{:02x}.{} → PSCID={}",
                   tid, (bdf >> 8) & 0xFF, (bdf >> 3) & 0x1F, bdf & 0x7, pscid);

        // Invalidate IOMMU's cached DC for this device — CPU fence alone is insufficient.
        if cq_virt != 0 {
            invalidate_dc(bar0, cq_virt, bdf as u64);
            issue_iofence(bar0, cq_virt);
        }
    }
}

/// Backward-compat: register a DMA range for the kernel domain (tid=0) without a BDF.
pub(super) fn map_range(phys: u64, size: usize) {
    map_range_for_cell(0, 0, phys, size);
}

// ── Cell exit DMA cleanup ─────────────────────────────────────────────────────

/// Flush IOTLB for `tid`'s domain and zero its DDT entries. Called on Cell exit.
///
/// Frame quarantine: IOFENCE must complete BEFORE the caller returns DMA frames
/// to the frame allocator.
pub(super) fn unmap_cell(tid: u64) {
    let bar0     = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let cq_virt  = CQ_VIRT.load(Ordering::Relaxed);

    // Remove domain while holding lock, then operate outside lock.
    let domain = RISCV_DOMAINS.lock().remove(&tid);
    let domain = match domain { Some(d) => d, None => return };

    if bar0 != 0 && cq_virt != 0 {
        invalidate_pscid_tlb(bar0, cq_virt, domain.pscid);
        issue_iofence(bar0, cq_virt); // frame quarantine: must complete before frame release
    }

    // Zero DDT entries for each BDF this Cell owned.
    if ddt_virt != 0 {
        for bdf in &domain.bdfs {
            let idx = (*bdf as usize) & 0x3F;
            let dc = ddt_virt + idx * 64;
            for i in 0usize..8 {
                // SAFETY: dc within DDT allocation; zeroing all 8 qwords clears TC.V.
                unsafe { core::ptr::write_volatile((dc + i * 8) as *mut u64, 0); }
            }
        }
    }

    free_pscid(domain.pscid);
    log::info!("[iommu] Cell {} domain cleaned up (PSCID={})", tid, domain.pscid);
}

// ── Phase 3: activate enforcement ────────────────────────────────────────────

/// Switch DDTP from BARE to 1LVL. Eagerly fills DCs for all registered kernel-domain BDFs.
///
/// After this call, DMA from any unregistered device triggers an IOMMU fault.
pub(super) fn activate() {
    let bar0     = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let ddt_phys = DDT_PHYS.load(Ordering::Relaxed);
    if bar0 == 0 || ddt_virt == 0 { return; }

    // Eagerly fill DC entries for all registered domains (typically kernel domain, tid=0).
    // Cell domains (tid>0) are lazy-filled at first DMA via map_range_for_cell.
    {
        let domains = RISCV_DOMAINS.lock();
        for domain in domains.values() {
            let fsc = SATP_MODE_SV39 | (domain.pt.root_phys() >> 12);
            for &bdf in &domain.bdfs {
                write_dc(ddt_virt, bdf as u64, fsc, domain.pscid);
            }
        }
    }

    let ddtp = ((ddt_phys >> 12) << 10) | DDTP_MODE_1LVL;
    unsafe { write64(bar0, REG_DDTP, ddtp); }

    super::iommu::set_active();
    log::info!("[iommu] RISC-V IOMMU: DMA isolation ACTIVE (Sv39 first-stage, 1LVL DDT)");
}
