//! RISC-V IOMMU driver — per-Cell DMA isolation via 3-level DDT + Sv39 first-stage.
//!
//! Phase 1 `init_hw()`:             probe PCIe IOMMU device, allocate L1 DDT + CQ. Stays BARE.
//! Phase 2 `map_range_for_cell()`:  register DMA range in a per-Cell Sv39 domain.
//! Phase 3 `activate()`:            fill kernel-domain DCs, switch DDTP to 3LVL enforcement.
//!
//! Each Cell gets its own `Sv39IommuPt` and a unique PSCID. Devices are isolated at the
//! Device Context (DC) level — a device can only DMA within its owning Cell's page table.
//!
//! 3LVL DDT: device_id[23:15]=DDI[2], [14:6]=DDI[1], [5:0]=DDI[0].
//! For PCIe BDF (16-bit), DDI[2]=bit15, DDI[1]=bits[14:6], DDI[0]=bits[5:0].
//! Eliminates bus-collision bug of 1LVL DDT (which indexed by device_id[5:0] only).

use alloc::{
    alloc::{alloc_zeroed, Layout},
    collections::BTreeMap,
    vec::Vec,
};
// RV32 lacks native 64-bit atomics; portable-atomic polyfills AtomicU64 there
// via the critical-section impl hal/arch/riscv registers.
use super::iommu::{classify_dma_publication, DmaMapResult};
use super::iommu_pt::Sv39IommuPt;
use super::iommu_riscv_cmd::{
    encode_cqb, encode_iodir_inval_ddt, encode_iofence_c, encode_iotinval_vma,
};
use crate::sync::Spinlock;
use crate::task::drivers::pcie_ecam;
#[cfg(not(target_arch = "riscv32"))]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
#[cfg(target_arch = "riscv32")]
use portable_atomic::AtomicU64;

const CLASS: u8 = 0x08;
const SUB: u8 = 0x06;
const PROGIF: u8 = 0x00;

// BAR0 register offsets (RISC-V IOMMU spec v1.0.1, register layout)
const REG_CAPS: usize = 0x00;
const REG_FCTL: usize = 0x08;
const REG_DDTP: usize = 0x10;
const REG_CQB: usize = 0x18;
const REG_CQH: usize = 0x20;
const REG_CQT: usize = 0x24;
const REG_CQCSR: usize = 0x48;
const REG_IPSR: usize = 0x54;

const DDTP_MODE_BARE: u64 = 1;
const DDTP_MODE_3LVL: u64 = 4;
const DDTP_BUSY: u64 = 1 << 4;

const CQCSR_CQEN: u32 = 1;
const CQCSR_CQON: u32 = 1 << 16;
const CQCSR_BUSY: u32 = 1 << 17;
const CQ_DEPTH: usize = 64;
const CQ_LOG2: u8 = 6;
const CQ_ENTRY: usize = 16;

const DC_TC_V: u64 = 1;
const SATP_MODE_SV39: u64 = 8u64 << 60;

const POLL_MAX: u64 = 1_000_000;

// ── Module-level state ────────────────────────────────────────────────────────

static BAR0: AtomicUsize = AtomicUsize::new(0);
static DDT_VIRT: AtomicUsize = AtomicUsize::new(0); // L1 table virtual address
static DDT_PHYS: AtomicU64 = AtomicU64::new(0); // L1 table physical address (identity)
static CQ_VIRT: AtomicUsize = AtomicUsize::new(0);
/// Serializes each invalidation batch through its acknowledging IOFENCE.
static CQ_TRANSACTION: Spinlock<()> = Spinlock::new(());

struct RiscvDomain {
    pt: Sv39IommuPt,
    pscid: u16,
    bdfs: Vec<u32>, // BDFs registered for this domain (for DC fill at activate + cleanup)
}

// Key = Tid as u64; kernel domain = tid 0.
static RISCV_DOMAINS: Spinlock<BTreeMap<u64, RiscvDomain>> = Spinlock::new(BTreeMap::new());

// Free-list prevents 16-bit PSCID exhaustion on long-running servers with Cell restarts.
// PSCID 0 is reserved (invalid); first real PSCID = 1.
static PSCID_FREE_LIST: Spinlock<Vec<u16>> = Spinlock::new(Vec::new());
static PSCID_NEXT: AtomicU16 = AtomicU16::new(1);

// ── MMIO helpers ─────────────────────────────────────────────────────────────

#[inline]
unsafe fn read32(base: usize, off: usize) -> u32 {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}
#[inline]
unsafe fn write32(base: usize, off: usize, val: u32) {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}
#[inline]
unsafe fn read64(base: usize, off: usize) -> u64 {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u64) }
}
#[inline]
unsafe fn write64(base: usize, off: usize, val: u64) {
    // SAFETY: caller ensures base is valid identity-mapped MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, val) }
}

// ── PSCID management ─────────────────────────────────────────────────────────

fn alloc_pscid() -> Option<u16> {
    if let Some(id) = PSCID_FREE_LIST.lock().pop() {
        return Some(id);
    }
    let id = PSCID_NEXT.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
        None
    } else {
        Some(id)
    } // id==0 means wrapped → exhausted
}

/// Return a PSCID to the free-list for reuse on Cell restart.
pub(super) fn free_pscid(id: u16) {
    if id != 0 {
        PSCID_FREE_LIST.lock().push(id);
    }
}

// ── Command queue ─────────────────────────────────────────────────────────────

fn wait_ddtp_ready(bar0: usize) -> bool {
    for _ in 0..POLL_MAX {
        if unsafe { read64(bar0, REG_DDTP) } & DDTP_BUSY == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    log::warn!("[iommu_riscv] DDTP busy timeout");
    false
}

fn wait_cq_state(bar0: usize, enabled: bool) -> bool {
    for _ in 0..POLL_MAX {
        let state = unsafe { read32(bar0, REG_CQCSR) };
        if state & CQCSR_BUSY == 0 && (state & CQCSR_CQON != 0) == enabled {
            return true;
        }
        core::hint::spin_loop();
    }
    log::warn!("[iommu_riscv] command queue state transition timeout");
    false
}

fn cq_head(bar0: usize) -> u32 {
    unsafe { read32(bar0, REG_CQH) }
}

/// Enqueue one 16-byte CQ entry. Returns `false` if the queue never frees a slot.
///
/// Caller holds `CQ_TRANSACTION`, preventing another producer from racing the
/// CQT read/slot write/tail publication sequence.
fn enqueue_cmd(bar0: usize, cq_virt: usize, w0: u64, w1: u64) -> bool {
    let mut spin = 0u64;
    loop {
        let tail = unsafe { read32(bar0, REG_CQT) };
        let head = cq_head(bar0);
        if (tail + 1) % CQ_DEPTH as u32 != head {
            break;
        }
        spin += 1;
        if spin > POLL_MAX {
            log::warn!("[iommu_riscv] CQ full — command was not published");
            return false;
        }
        core::hint::spin_loop();
    }
    let tail = unsafe { read32(bar0, REG_CQT) };
    let slot = cq_virt + (tail as usize) * CQ_ENTRY;
    unsafe {
        core::ptr::write_volatile(slot as *mut u64, w0);
        core::ptr::write_volatile((slot + 8) as *mut u64, w1);
        core::sync::atomic::fence(Ordering::Release);
        write32(bar0, REG_CQT, (tail + 1) % CQ_DEPTH as u32);
    }
    true
}

/// Issue IOFENCE.C and return only after the IOMMU drains the queue.
///
/// Frame quarantine: `true` is the acknowledgement required before DMA frames
/// or BDF ownership may be reused.
fn issue_iofence(bar0: usize, cq_virt: usize) -> bool {
    let (w0, w1) = encode_iofence_c();
    if !enqueue_cmd(bar0, cq_virt, w0, w1) {
        return false;
    }
    let expected = unsafe { read32(bar0, REG_CQT) };
    let mut spin = 0u64;
    loop {
        if cq_head(bar0) == expected {
            return true;
        }
        spin += 1;
        if spin > POLL_MAX {
            log::warn!("[iommu_riscv] IOFENCE timeout — retaining DMA quarantine");
            return false;
        }
        core::hint::spin_loop();
    }
}

/// Invalidate all first-stage IOTLB entries for a specific PSCID.
fn invalidate_pscid_tlb(bar0: usize, cq_virt: usize, pscid: u16) -> bool {
    let (w0, w1) = encode_iotinval_vma(pscid as u32);
    enqueue_cmd(bar0, cq_virt, w0, w1)
}

/// Invalidate the IOMMU's cached Device Context for a device_id.
fn invalidate_dc(bar0: usize, cq_virt: usize, device_id: u32) -> bool {
    let (w0, w1) = encode_iodir_inval_ddt(device_id);
    enqueue_cmd(bar0, cq_virt, w0, w1)
}

// ── 3LVL DDT tree management ──────────────────────────────────────────────────

/// Allocate a zeroed 4 KiB page for a DDT non-leaf or leaf table.
fn alloc_ddt_page() -> usize {
    let layout = Layout::from_size_align(4096, 4096).expect("iommu: ddt page");
    // SAFETY: layout is non-zero and 4096-aligned.
    let ptr = unsafe { alloc_zeroed(layout) } as usize;
    assert!(ptr != 0, "[iommu_riscv] OOM: DDT child page");
    ptr
}

/// Return the virtual address of the child table at `table_virt[idx]`.
///
/// Non-leaf entry format: bits[63:10] = PPN, bit[0] = V.
/// Allocates a new child page if the entry is not yet valid (V=0).
/// Identity-mapped: VA == PA for kernel-heap pages.
fn ensure_child_table(table_virt: usize, idx: usize) -> usize {
    let slot = table_virt + idx * 8;
    // SAFETY: slot is within a 4096-B DDT table page.
    let entry = unsafe { core::ptr::read_volatile(slot as *const u64) };
    if entry & 1 != 0 {
        // V=1 — child already exists; return its virtual address.
        let ppn = (entry >> 10) & 0x000F_FFFF_FFFF_FFFF;
        (ppn << 12) as usize // identity-mapped: VA == PA
    } else {
        // V=0 — allocate and link a new child table.
        let child_virt = alloc_ddt_page();
        let child_phys = child_virt as u64; // identity-mapped
        let non_leaf = ((child_phys >> 12) << 10) | 1u64; // PPN | V
                                                          // SAFETY: slot is within the parent DDT table.
        unsafe {
            core::ptr::write_volatile(slot as *mut u64, non_leaf);
        }
        child_virt
    }
}

/// Navigate or allocate the 3-level DDT tree for `bdf` and return the DC slot address.
///
/// Allocates intermediate pages on demand. Never modifies the DC itself.
fn get_dc_slot(l1_virt: usize, bdf: u32) -> usize {
    let ddi2 = ((bdf >> 15) & 0x1FF) as usize; // bits[23:15] of device_id (=bdf for PCIe)
    let ddi1 = ((bdf >> 6) & 0x1FF) as usize; // bits[14:6]
    let ddi0 = (bdf & 0x03F) as usize; // bits[5:0]

    let l2_virt = ensure_child_table(l1_virt, ddi2);
    let l3_virt = ensure_child_table(l2_virt, ddi1);
    l3_virt + ddi0 * 64 // each leaf DC is 64 bytes
}

/// Write a Device Context at `dc` with the given `fsc` (first-stage config) and `pscid`.
///
/// TC.V is written LAST after a Release fence so the IOMMU sees a consistent DC.
fn write_dc_fields(dc: usize, fsc: u64, pscid: u16) {
    let ta = (pscid as u64) << 12; // ta.PSCID in bits[31:12]
                                   // SAFETY: dc is the start of a 64-byte leaf DC; caller ensures valid alignment.
    unsafe {
        core::ptr::write_volatile((dc + 8) as *mut u64, 0u64); // iohgatp: G-stage bare
        core::ptr::write_volatile((dc + 16) as *mut u64, ta); // ta: PSCID
        core::ptr::write_volatile((dc + 24) as *mut u64, fsc); // fsc: Sv39 first-stage PT
        core::ptr::write_volatile((dc + 32) as *mut u64, 0u64); // msiptp
        core::ptr::write_volatile((dc + 40) as *mut u64, 0u64);
        core::ptr::write_volatile((dc + 48) as *mut u64, 0u64);
        core::ptr::write_volatile((dc + 56) as *mut u64, 0u64);
        core::sync::atomic::fence(Ordering::Release); // all fields visible before TC.V
        core::ptr::write_volatile(dc as *mut u64, DC_TC_V); // TC.V last — makes DC live
    }
}

/// Write a valid Device Context for `bdf` in the 3LVL DDT.
fn write_dc_3lvl(l1_virt: usize, bdf: u32, fsc: u64, pscid: u16) {
    let dc = get_dc_slot(l1_virt, bdf);
    write_dc_fields(dc, fsc, pscid);
}

/// Zero the Device Context for `bdf` (TC.V=0 → IOMMU treats device as not present).
fn zero_dc_3lvl(l1_virt: usize, bdf: u32) {
    let dc = get_dc_slot(l1_virt, bdf);
    // SAFETY: dc is the start of a 64-byte leaf DC.
    for i in 0usize..8 {
        unsafe {
            core::ptr::write_volatile((dc + i * 8) as *mut u64, 0);
        }
    }
}

// ── Phase 1: probe + allocate ─────────────────────────────────────────────────

/// Probe RISC-V IOMMU hardware, allocate L1 DDT and command queue.
/// Stays in BARE (passthrough) mode until `activate()` is called.
pub(super) fn init_hw() {
    let dev = match pcie_ecam::find_class(CLASS, SUB, PROGIF) {
        Some(d) => d,
        None => {
            log::warn!(
                "[iommu] RISC-V IOMMU not found \
                        (needs QEMU ≥8.2 + -device riscv-iommu-pci,bus=pcie.0)"
            );
            return;
        }
    };
    let bar0 = dev.bars[0].base_addr() as usize;
    if bar0 == 0 {
        log::warn!("[iommu] RISC-V IOMMU BAR0 == 0");
        return;
    }

    let _caps = unsafe { read64(bar0, REG_CAPS) };
    if !wait_ddtp_ready(bar0) {
        return;
    }
    unsafe {
        // Feature controls may change only while the IOMMU is Off and queues
        // are disabled. Keep memory structures little-endian.
        write64(bar0, REG_DDTP, 0);
    }
    if !wait_ddtp_ready(bar0) {
        return;
    }
    unsafe {
        write32(bar0, REG_CQCSR, 0);
    }
    if !wait_cq_state(bar0, false) {
        return;
    }
    unsafe {
        write32(bar0, REG_FCTL, 0);
        let ipsr = read32(bar0, REG_IPSR);
        if ipsr != 0 {
            write32(bar0, REG_IPSR, ipsr);
        }
        write64(bar0, REG_DDTP, DDTP_MODE_BARE);
    }
    if !wait_ddtp_ready(bar0) {
        return;
    }

    // Allocate L1 DDT: 512 × 8-byte non-leaf entries = 4096 bytes.
    // (Same size as 1LVL DDT; different internal structure.)
    let layout = Layout::from_size_align(4096, 4096).expect("iommu: DDT L1");
    let ddt_virt = unsafe { alloc_zeroed(layout) } as usize;
    assert!(ddt_virt != 0, "[iommu_riscv] OOM: DDT L1");

    // Allocate CQ: 64 entries × 16B = 1024B (use full page for alignment).
    let layout = Layout::from_size_align(4096, 4096).expect("iommu: CQ");
    let cq_virt = unsafe { alloc_zeroed(layout) } as usize;
    assert!(cq_virt != 0, "[iommu_riscv] OOM: CQ");

    unsafe {
        write64(bar0, REG_CQB, encode_cqb(cq_virt as u64, CQ_LOG2));
        write32(bar0, REG_CQT, 0);
        write32(bar0, REG_CQCSR, CQCSR_CQEN);
    }
    if !wait_cq_state(bar0, true) {
        return;
    }

    BAR0.store(bar0, Ordering::Relaxed);
    DDT_VIRT.store(ddt_virt, Ordering::Relaxed);
    DDT_PHYS.store(ddt_virt as u64, Ordering::Relaxed); // identity-mapped: VA == PA
    CQ_VIRT.store(cq_virt, Ordering::Relaxed);

    log::info!(
        "[iommu] RISC-V IOMMU HW ready (vendor={:04x} dev={:04x}) \
                — isolation pending",
        dev.vendor_id,
        dev.device_id
    );
}

// ── Phase 2: register DMA ranges ─────────────────────────────────────────────

/// Register `[phys, phys+size)` for Cell `tid` owning device `bdf`.
///
/// Creates a per-Cell `Sv39IommuPt` + PSCID on first call. Writes a DDT entry
/// for `bdf` immediately (even before `activate()`). IODIR.INVAL_DDT and
/// IOTINVAL.VMA are acknowledged together by IOFENCE.C after each update.
pub(super) fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    let bar0 = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let cq_virt = CQ_VIRT.load(Ordering::Relaxed);
    if bar0 == 0 || ddt_virt == 0 {
        return DmaMapResult::Rejected;
    }

    let mut domains = RISCV_DOMAINS.lock();
    let domain = domains.entry(tid).or_insert_with(|| {
        let pscid = alloc_pscid().expect("[iommu_riscv] PSCID exhausted (max 65535 active Cells)");
        RiscvDomain {
            pt: Sv39IommuPt::new(),
            pscid,
            bdfs: Vec::new(),
        }
    });

    domain.pt.map_range(phys, size);

    if bdf != 0 {
        let fsc = SATP_MODE_SV39 | (domain.pt.root_phys() >> 12);
        let pscid = domain.pscid;
        write_dc_3lvl(ddt_virt, bdf, fsc, pscid);
        if !domain.bdfs.contains(&bdf) {
            domain.bdfs.push(bdf);
        }

        log::info!(
            "[iommu] Cell {} BDF {:02x}:{:02x}.{} → PSCID={}",
            tid,
            (bdf >> 8) & 0xFF,
            (bdf >> 3) & 0x1F,
            bdf & 0x7,
            pscid
        );

        // The DC is already visible in memory. Missing CQ publication or
        // IOFENCE acknowledgement is therefore published-but-unconfirmed and
        // must retain the caller's DMA pin. Serialize the complete batch so
        // another hart cannot overwrite a command before this fence drains.
        let (translations_published, fence_acknowledged) = {
            let _transaction = CQ_TRANSACTION.lock();
            let directory_published = cq_virt != 0 && invalidate_dc(bar0, cq_virt, bdf);
            let translations_published =
                directory_published && invalidate_pscid_tlb(bar0, cq_virt, pscid);
            let fence_acknowledged = translations_published && issue_iofence(bar0, cq_virt);
            (translations_published, fence_acknowledged)
        };
        return classify_dma_publication(phys, translations_published, fence_acknowledged);
    }
    DmaMapResult::Mapped(phys)
}

/// Backward-compat: register a DMA range for the kernel domain (tid=0) without a BDF.
#[allow(dead_code)] // reason: kept for API parity with iommu_x86; no caller wired up yet
pub(super) fn map_range(phys: u64, size: usize) {
    let _ = map_range_for_cell(0, 0, phys, size);
}

// ── Cell exit DMA cleanup ─────────────────────────────────────────────────────

/// Flush IOTLB and invalidate DDT contexts for `tid`.
///
/// Returns `true` only after hardware acknowledges every teardown command.
/// Failure keeps the domain so ownership and pinned frames remain quarantined.
pub(super) fn unmap_cell(tid: u64) -> bool {
    let bar0 = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let cq_virt = CQ_VIRT.load(Ordering::Relaxed);

    // The dead task cannot issue new mappings. Temporarily remove its domain so
    // teardown can run without holding RISCV_DOMAINS across bounded MMIO polls.
    let Some(domain) = RISCV_DOMAINS.lock().remove(&tid) else {
        return true;
    };

    if bar0 != 0 {
        if ddt_virt == 0 || cq_virt == 0 {
            RISCV_DOMAINS.lock().insert(tid, domain);
            return false;
        }

        // Publish every DDT removal, invalidate the domain's first-stage
        // translations, then use one IOFENCE as acknowledgement for the batch.
        // The transaction guard drops before a failed domain is reinserted,
        // preserving the map path's RISCV_DOMAINS -> CQ_TRANSACTION lock order.
        let acknowledged = {
            let _transaction = CQ_TRANSACTION.lock();
            let directory_published = domain.bdfs.iter().copied().all(|bdf| {
                zero_dc_3lvl(ddt_virt, bdf);
                invalidate_dc(bar0, cq_virt, bdf)
            });
            let translations_published =
                directory_published && invalidate_pscid_tlb(bar0, cq_virt, domain.pscid);
            translations_published && issue_iofence(bar0, cq_virt)
        };
        if !acknowledged {
            RISCV_DOMAINS.lock().insert(tid, domain);
            return false;
        }
    }

    free_pscid(domain.pscid);
    log::info!(
        "[iommu] Cell {} domain cleaned up (PSCID={})",
        tid,
        domain.pscid
    );
    true
}

// ── Phase 3: activate enforcement ────────────────────────────────────────────

/// Switch DDTP from BARE to 3LVL. Eagerly fills DCs for all registered kernel-domain BDFs.
///
/// After this call, DMA from any unregistered device triggers an IOMMU fault.
pub(super) fn activate() {
    let bar0 = BAR0.load(Ordering::Relaxed);
    let ddt_virt = DDT_VIRT.load(Ordering::Relaxed);
    let ddt_phys = DDT_PHYS.load(Ordering::Relaxed);
    if bar0 == 0 || ddt_virt == 0 {
        return;
    }

    // Eagerly fill DC entries for all registered domains (typically kernel domain, tid=0).
    // Cell domains (tid>0) are lazy-filled at first DMA via map_range_for_cell.
    {
        let domains = RISCV_DOMAINS.lock();
        for domain in domains.values() {
            let fsc = SATP_MODE_SV39 | (domain.pt.root_phys() >> 12);
            for &bdf in &domain.bdfs {
                write_dc_3lvl(ddt_virt, bdf, fsc, domain.pscid);
            }
        }
    }

    // DDTP: PPN of L1 table | MODE=3LVL.
    let ddtp = ((ddt_phys >> 12) << 10) | DDTP_MODE_3LVL;
    unsafe {
        write64(bar0, REG_DDTP, ddtp);
    }
    if !wait_ddtp_ready(bar0) || unsafe { read64(bar0, REG_DDTP) } & 0xF != DDTP_MODE_3LVL {
        log::warn!("[iommu_riscv] 3LVL activation was not acknowledged");
        return;
    }

    super::iommu::set_active();
    log::info!("[iommu] RISC-V IOMMU: DMA isolation ACTIVE (Sv39 first-stage, 3LVL DDT)");
}
