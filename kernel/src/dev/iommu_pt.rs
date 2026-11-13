//! Identity-mapping IOMMU page tables for DMA isolation.
//!
//! Both table types map IOVA == PA for explicitly registered DMA ranges.
//! Any other IOVA causes an IOMMU fault — preventing arbitrary RAM access via DMA.
//!
//! `Sv39IommuPt` — RISC-V IOMMU second-stage (Sv39, 3 levels)
//! `VtdSlpt`     — Intel VT-d second-level page table (3 levels, AW=39-bit)

use alloc::alloc::{alloc_zeroed, Layout};

fn alloc_page() -> (usize, u64) {
    let layout = Layout::from_size_align(4096, 4096).expect("iommu_pt: page");
    // SAFETY: layout is non-zero and 4096-aligned.
    // The witness is the heap table allocation, including hardware-independent
    // boot probes, not the physical DMA frames later named by its leaf entries.
    let ptr = {
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::DRIVER_SHARED,
        );
        (unsafe { alloc_zeroed(layout) }) as usize
    };
    assert!(ptr != 0, "[iommu_pt] OOM allocating IOMMU page");
    (ptr, virt_to_phys(ptr))
}

/// Physical address of a kernel-heap pointer (HHDM on x86_64, identity elsewhere).
#[inline]
pub(crate) fn virt_to_phys(virt: usize) -> u64 {
    crate::arch::iommu::virt_to_phys(virt)
}

#[inline]
pub(super) fn phys_to_virt_inner(phys: u64) -> usize {
    crate::arch::iommu::phys_to_virt(phys)
}

// ── RISC-V Sv39 second-stage page table ──────────────────────────────────────

const SV39_V: u64 = 1 << 0; // Valid
const SV39_R: u64 = 1 << 1; // Read
const SV39_W: u64 = 1 << 2; // Write
const SV39_A: u64 = 1 << 6; // Accessed (pre-set to avoid IOMMU A-fault on first DMA)
const SV39_D: u64 = 1 << 7; // Dirty   (pre-set to avoid IOMMU D-fault on first DMA)
/// Leaf DMA PTE: V|R|W|A|D — readable+writable, no execute.
const PTE_DMA: u64 = SV39_V | SV39_R | SV39_W | SV39_A | SV39_D;

/// Sv39 3-level identity-mapping page table for RISC-V IOMMU second-stage.
pub struct Sv39IommuPt {
    root_phys: u64,
    root_virt: usize,
}

// SAFETY: pointers are into kernel-owned 4 KiB pages never aliased externally.
unsafe impl Send for Sv39IommuPt {}

impl Default for Sv39IommuPt {
    fn default() -> Self {
        Self::new()
    }
}

impl Sv39IommuPt {
    pub fn new() -> Self {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        let (v, p) = alloc_page();
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::created_heap(p, 3, v);
        Self {
            root_phys: p,
            root_virt: v,
        }
    }

    /// Add an identity mapping (IOVA == PA) for [phys, phys+size). Idempotent.
    pub fn map_range(&self, phys: u64, size: usize) {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut pa = start;
        while pa < end {
            self.map_page(pa);
            pa += 0x1000;
        }
    }

    fn map_page(&self, pa: u64) {
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let l1_phys = ensure_sv39_child(self.root_virt, vpn2);
        let l0_phys = ensure_sv39_child(phys_to_virt_inner(l1_phys), vpn1);
        let leaf = ((pa >> 12) << 10) | PTE_DMA;
        // SAFETY: l0 page is a valid 4 KiB allocation; vpn0 < 512.
        unsafe {
            let ptr = (phys_to_virt_inner(l0_phys) + vpn0 * 8) as *mut u64;
            ptr.write_volatile(leaf);
        }
    }

    /// Remove the identity mapping for `[phys, phys+size)`. Idempotent.
    ///
    /// Lookup-only: a missing child table is skipped rather than allocated, and
    /// no intermediate table is ever freed — a sibling range sharing the same
    /// 4 KiB table may still be mapped. Returns the number of leaf entries
    /// cleared, so a caller can tell "nothing was mapped" from "unmapped".
    ///
    /// Zeroing a leaf is not enough on its own: the caller must invalidate the
    /// IOMMU's cached translation for the cleared page before the frames behind
    /// it are reused (see `iommu_riscv::unmap_range_for_cell`).
    pub fn unmap_range(&self, phys: u64, size: usize) -> usize {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut cleared = 0;
        let mut pa = start;
        while pa < end {
            if self.unmap_page(pa) {
                cleared += 1;
            }
            pa += 0x1000;
        }
        cleared
    }

    /// Zero the leaf entry mapping `pa`. Returns whether one was present.
    fn unmap_page(&self, pa: u64) -> bool {
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_sv39_child(self.root_virt, vpn2) else {
            return false;
        };
        let Some(l0_phys) = lookup_sv39_child(phys_to_virt_inner(l1_phys), vpn1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; vpn0 < 512.
        let slot = (phys_to_virt_inner(l0_phys) + vpn0 * 8) as *mut u64;
        unsafe {
            if slot.read_volatile() == 0 {
                return false;
            }
            slot.write_volatile(0);
        }
        true
    }

    /// Physical address of the root page (program into Device Context satp.PPN).
    #[inline]
    pub fn root_phys(&self) -> u64 {
        self.root_phys
    }
}

/// Get or allocate a child table at `table_virt[idx]`. Returns child phys.
fn ensure_sv39_child(table_virt: usize, idx: usize) -> u64 {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let slot = (table_virt + idx * 8) as *mut u64;
    let e = unsafe { slot.read_volatile() };
    if e & SV39_V != 0 {
        return (e >> 10) << 12; // extract PPN → phys
    }
    let (_, child_phys) = alloc_page();
    let ptr_pte = ((child_phys >> 12) << 10) | SV39_V; // V=1, no R/W/X = non-leaf
    unsafe {
        slot.write_volatile(ptr_pte);
    }
    child_phys
}

/// Child table at `table_virt[idx]`, or `None` when the entry is not valid.
///
/// The teardown counterpart of [`ensure_sv39_child`]: never allocates.
fn lookup_sv39_child(table_virt: usize, idx: usize) -> Option<u64> {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let e = unsafe { ((table_virt + idx * 8) as *const u64).read_volatile() };
    if e & SV39_V != 0 {
        Some((e >> 10) << 12)
    } else {
        None
    }
}

// ── Intel VT-d 3-level SLPT (AW=39-bit) ──────────────────────────────────────

/// VT-d SLPT entry flag: R=1|W=1 required for all valid entries (leaf + non-leaf).
const VTD_RW: u64 = 0b11;
/// VT-d SLPT leaf permission for a device-read-only export window (`R` only).
const VTD_R: u64 = 0b01;

/// Intel VT-d second-level page table (3 levels, 39-bit address width).
pub struct VtdSlpt {
    root_phys: u64,
    root_virt: usize,
    /// The owning unit snoops CPU caches (`ECAP.C`). When it does not, every
    /// table store must be written back before the unit can see it.
    coherent: bool,
}

// SAFETY: pointers are into kernel-owned 4 KiB pages never aliased externally.
unsafe impl Send for VtdSlpt {}

impl VtdSlpt {
    /// An empty table for a unit whose walks do (`coherent`) or do not snoop
    /// the CPU caches.
    pub fn new(coherent: bool) -> Self {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        let (v, p) = alloc_page();
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::created_heap(p, 4, v);
        let slpt = Self {
            root_phys: p,
            root_virt: v,
            coherent,
        };
        slpt.sync(v, 4096);
        slpt
    }

    /// Write `[virt, virt + len)` back to memory for a non-snooping unit.
    fn sync(&self, virt: usize, len: usize) {
        crate::arch::iommu::sync_table(virt, len, self.coherent);
    }

    /// Add an identity mapping (IOVA == PA) for [phys, phys+size). Idempotent.
    pub fn map_range(&self, phys: u64, size: usize) {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut pa = start;
        while pa < end {
            self.map_page(pa);
            pa += 0x1000;
        }
    }

    fn map_page(&self, pa: u64) {
        self.map_leaf(pa, pa, VTD_RW);
    }

    /// Whether no leaf is present in `[iova, iova + size)`.
    ///
    /// Lookup-only: a missing child table means the whole span under it is
    /// free, and nothing is allocated. Export windows call this before any
    /// change so a window never overwrites an identity or RMRR leaf.
    pub fn window_free(&self, iova: u64, size: usize) -> bool {
        let end = iova + size as u64;
        let mut va = iova & !0xFFF;
        while va < end {
            if self.leaf_at(va).is_some_and(|leaf| leaf != 0) {
                return false;
            }
            va += 0x1000;
        }
        true
    }

    /// Map the physically contiguous `[phys, phys + size)` at the non-identity
    /// IOVA window `[iova, iova + size)` (ADR-0033 CP1 D-G3).
    ///
    /// `writable` clear publishes `R`-only leaves (D-G10), so the remapper
    /// itself refuses device writes. Both bounds must be 4 KiB aligned; the
    /// caller has checked [`Self::window_free`] and invalidates afterwards.
    pub fn map_window(&self, iova: u64, phys: u64, size: usize, writable: bool) {
        let rights = if writable { VTD_RW } else { VTD_R };
        let mut offset = 0u64;
        while offset < size as u64 {
            self.map_leaf(iova + offset, phys + offset, rights);
            offset += 0x1000;
        }
    }

    /// Write one leaf `iova → pa` with `rights`, allocating child tables.
    fn map_leaf(&self, iova: u64, pa: u64, rights: u64) {
        let i2 = ((iova >> 30) & 0x1FF) as usize;
        let i1 = ((iova >> 21) & 0x1FF) as usize;
        let i0 = ((iova >> 12) & 0x1FF) as usize;
        let l1_phys = self.ensure_child(self.root_virt, i2);
        let l0_phys = self.ensure_child(phys_to_virt_inner(l1_phys), i1);
        let ptr = (phys_to_virt_inner(l0_phys) + i0 * 8) as *mut u64;
        // SAFETY: l0 page is a valid 4 KiB allocation; i0 < 512.
        unsafe { ptr.write_volatile((pa & !0xFFF) | rights) };
        self.sync(ptr as usize, 8);
    }

    /// The raw leaf for `iova`, or `None` when a child table is missing.
    fn leaf_at(&self, iova: u64) -> Option<u64> {
        let i2 = ((iova >> 30) & 0x1FF) as usize;
        let i1 = ((iova >> 21) & 0x1FF) as usize;
        let i0 = ((iova >> 12) & 0x1FF) as usize;
        let l1_phys = lookup_vtd_child(self.root_virt, i2)?;
        let l0_phys = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1)?;
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        Some(unsafe { ((phys_to_virt_inner(l0_phys) + i0 * 8) as *const u64).read_volatile() })
    }

    /// Get or allocate a child table at `table_virt[idx]`. Returns child phys.
    /// A new child is written back (zeroed) before the entry naming it.
    fn ensure_child(&self, table_virt: usize, idx: usize) -> u64 {
        // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
        let slot = (table_virt + idx * 8) as *mut u64;
        let e = unsafe { slot.read_volatile() };
        if e & VTD_RW != 0 {
            return e & !0xFFF; // extract physical address from non-leaf entry
        }
        let (child_virt, child_phys) = alloc_page();
        self.sync(child_virt, 4096);
        // SAFETY: as above.
        unsafe { slot.write_volatile((child_phys & !0xFFF) | VTD_RW) };
        self.sync(slot as usize, 8);
        child_phys
    }

    /// Remove the identity mapping for `[phys, phys+size)`. Idempotent.
    ///
    /// Lookup-only and leaf-only: a missing child table is skipped rather than
    /// allocated, and no intermediate SLPT page is ever freed — a sibling range
    /// sharing the same table may still be mapped. Returns the number of leaf
    /// entries cleared.
    ///
    /// Zeroing a leaf is not enough on its own: the caller must invalidate the
    /// IOTLB entry for the cleared page before the frames behind it are reused
    /// (see `iommu_x86::unmap_range_for_cell`).
    pub fn unmap_range(&self, phys: u64, size: usize) -> usize {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut cleared = 0;
        let mut pa = start;
        while pa < end {
            if self.unmap_page(pa) {
                cleared += 1;
            }
            pa += 0x1000;
        }
        cleared
    }

    /// Zero the leaf entry mapping `pa`. Returns whether one was present.
    fn unmap_page(&self, pa: u64) -> bool {
        let i2 = ((pa >> 30) & 0x1FF) as usize;
        let i1 = ((pa >> 21) & 0x1FF) as usize;
        let i0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_vtd_child(self.root_virt, i2) else {
            return false;
        };
        let Some(l0_phys) = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        let slot = (phys_to_virt_inner(l0_phys) + i0 * 8) as *mut u64;
        unsafe {
            if slot.read_volatile() == 0 {
                return false;
            }
            slot.write_volatile(0);
        }
        self.sync(slot as usize, 8);
        true
    }

    /// Physical address of the SLPT root page.
    #[inline]
    pub fn root_phys(&self) -> u64 {
        self.root_phys
    }
}

/// Child table at `table_virt[idx]`, or `None` when the entry is not present.
///
/// The teardown counterpart of [`ensure_vtd_child`]: never allocates.
fn lookup_vtd_child(table_virt: usize, idx: usize) -> Option<u64> {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let e = unsafe { ((table_virt + idx * 8) as *const u64).read_volatile() };
    if e & VTD_RW != 0 {
        Some(e & !0xFFF)
    } else {
        None
    }
}

// ── Teardown self-test ────────────────────────────────────────────────────────

/// The exact leaf an IOMMU walk would consult for `pa`.
#[cfg(feature = "test-hooks")]
impl Sv39IommuPt {
    pub(crate) fn leaf_present(&self, pa: u64) -> bool {
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_sv39_child(self.root_virt, vpn2) else {
            return false;
        };
        let Some(l0_phys) = lookup_sv39_child(phys_to_virt_inner(l1_phys), vpn1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; vpn0 < 512.
        unsafe { ((phys_to_virt_inner(l0_phys) + vpn0 * 8) as *const u64).read_volatile() != 0 }
    }

    /// Whether the level-2 table covering `pa` exists at all.
    pub(crate) fn level2_present(&self, pa: u64) -> bool {
        lookup_sv39_child(self.root_virt, ((pa >> 30) & 0x1FF) as usize).is_some()
    }
}

/// The exact leaf an IOMMU walk would consult for `pa`.
#[cfg(feature = "test-hooks")]
impl VtdSlpt {
    pub(crate) fn leaf_present(&self, pa: u64) -> bool {
        let i2 = ((pa >> 30) & 0x1FF) as usize;
        let i1 = ((pa >> 21) & 0x1FF) as usize;
        let i0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_vtd_child(self.root_virt, i2) else {
            return false;
        };
        let Some(l0_phys) = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        unsafe { ((phys_to_virt_inner(l0_phys) + i0 * 8) as *const u64).read_volatile() != 0 }
    }

    /// Whether the level-2 table covering `pa` exists at all.
    pub(crate) fn level2_present(&self, pa: u64) -> bool {
        lookup_vtd_child(self.root_virt, ((pa >> 30) & 0x1FF) as usize).is_some()
    }
}

/// What one page table's teardown probe observed.
#[cfg(feature = "test-hooks")]
struct TeardownProbe {
    /// A cleared leaf reads absent while its two neighbours in the same table stay.
    leaf: bool,
    /// Clearing the same page twice reports nothing cleared the second time.
    idempotent: bool,
    /// Unmapping a range that was never mapped allocates no table and clears nothing.
    lookup_only: bool,
    /// The cleared page can be mapped again (no table was freed by the unmap).
    remappable: bool,
}

/// Exercise `map_range` / `unmap_range` on one page table.
///
/// The three pages sit in one 4 KiB leaf table, so the neighbours prove the walk
/// clears the exact entry rather than the table, and the re-map proves the
/// intermediate tables survived.
#[cfg(feature = "test-hooks")]
fn probe_teardown<M, U, P, L>(map: M, unmap: U, present: P, level2: L) -> TeardownProbe
where
    M: Fn(u64, usize),
    U: Fn(u64, usize) -> usize,
    P: Fn(u64) -> bool,
    L: Fn(u64) -> bool,
{
    const BASE: u64 = 0x1234_5000;
    const PAGE: u64 = 0x1000;
    // Never mapped, and far outside the range above: any table it allocates would
    // be visible as a level-2 entry of its own.
    const UNMAPPED: u64 = 0x7FFF_0000;

    map(BASE, 3 * PAGE as usize);
    let mapped = (0..3).all(|i| present(BASE + i as u64 * PAGE));

    let cleared = unmap(BASE + PAGE, PAGE as usize);
    let leaf = mapped
        && cleared == 1
        && !present(BASE + PAGE)
        && present(BASE)
        && present(BASE + 2 * PAGE);

    let idempotent = unmap(BASE + PAGE, PAGE as usize) == 0 && !present(BASE + PAGE);

    let lookup_only =
        !level2(UNMAPPED) && unmap(UNMAPPED, 3 * PAGE as usize) == 0 && !level2(UNMAPPED);

    map(BASE + PAGE, PAGE as usize);
    let remappable = present(BASE + PAGE) && present(BASE) && present(BASE + 2 * PAGE);

    TeardownProbe {
        leaf,
        idempotent,
        lookup_only,
        remappable,
    }
}

/// Boot-time assertions for the teardown primitives.
///
/// The page tables are plain memory, so this runs without IOMMU hardware: it is
/// the page-table half of a revoke (leaf exactly cleared, siblings untouched,
/// no allocation on the unmap path). The hardware half — that the invalidation
/// is acknowledged — is proven by the DMA-isolation QEMU lanes.
#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    let sv39 = Sv39IommuPt::new();
    let sv39 = probe_teardown(
        |phys, size| sv39.map_range(phys, size),
        |phys, size| sv39.unmap_range(phys, size),
        |pa| sv39.leaf_present(pa),
        |pa| sv39.level2_present(pa),
    );

    // No unit walks this table; coherent skips the write-back.
    let vtd = VtdSlpt::new(true);
    let vtd = probe_teardown(
        |phys, size| vtd.map_range(phys, size),
        |phys, size| vtd.unmap_range(phys, size),
        |pa| vtd.leaf_present(pa),
        |pa| vtd.level2_present(pa),
    );

    report("IOMMU-TEARDOWN-LEAF", sv39.leaf && vtd.leaf);
    report(
        "IOMMU-TEARDOWN-IDEMPOTENT",
        sv39.idempotent && vtd.idempotent,
    );
    report(
        "IOMMU-TEARDOWN-LOOKUP-ONLY",
        sv39.lookup_only && vtd.lookup_only,
    );
    report(
        "IOMMU-TEARDOWN-REMAPPABLE",
        sv39.remappable && vtd.remappable,
    );
}

/// One PASS/FAIL line per teardown property; the markers are the QEMU evidence.
#[cfg(feature = "test-hooks")]
fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}
// ── Apple DART (Device Address Resolution Table, Phase 05) ─────────────────────

pub const DART_PAGE_SIZE: usize = 16384;
pub const DART_PAGE_MASK: u64 = 0x3FFF;
pub const DART_PTE_VALID: u64 = 1 << 0;
/// DART1 (t8103-format) leaf: subpage protection disabled. Read/write access
/// is granted by leaving the negative NO_WRITE (bit 7) / NO_READ (bit 8) clear.
pub const DART1_PTE_SP_DIS: u64 = 1 << 1;
/// Subpage window `[0, 0xFFF]`: the whole 16 KiB page, as Linux programs it.
pub const DART_PTE_SUBPAGE_FULL: u64 = 0xFFF << 40;
pub const DART_PTE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_C000;
pub const DART_ENTRIES_PER_TABLE: usize = 2048; // 16 KiB / 8 bytes
/// Two 11-bit levels over 16 KiB pages reach 2^36; a wider IOVA would alias
/// into a lower L1 slot.
pub const DART_IOVA_LIMIT: u64 = 1 << 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartError {
    BypassForbidden,
    /// The range ends beyond [`DART_IOVA_LIMIT`] (or overflows).
    OutOfRange,
}

/// 16 KiB-padded `[start, end)` of `[phys, phys+size)`, or `None` on overflow.
fn dart_span(phys: u64, size: usize) -> Option<(u64, u64)> {
    let end = phys.checked_add(size as u64)?.checked_add(DART_PAGE_MASK)? & !DART_PAGE_MASK;
    Some((phys & !DART_PAGE_MASK, end))
}

fn alloc_dart_page() -> (usize, u64) {
    let layout = Layout::from_size_align(16384, 16384).expect("iommu_pt: dart page");
    let ptr = {
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::DRIVER_SHARED,
        );
        (unsafe { alloc_zeroed(layout) }) as usize
    };
    assert!(ptr != 0, "[iommu_pt] OOM allocating DART 16 KiB page");
    (ptr, virt_to_phys(ptr))
}

/// 2-level identity-mapping page table for Apple DART IOMMU (16 KiB pages).
pub struct AppleDartPt {
    root_phys: u64,
    root_virt: usize,
}

unsafe impl Send for AppleDartPt {}

impl Default for AppleDartPt {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleDartPt {
    pub fn new() -> Self {
        let (v, p) = alloc_dart_page();
        Self {
            root_phys: p,
            root_virt: v,
        }
    }

    pub fn root_phys(&self) -> u64 {
        self.root_phys
    }

    /// Add an identity mapping (IOVA == PA) for [phys, phys+size), padded to 16 KiB pages. Idempotent.
    ///
    /// Refused as a whole when any page lies at or beyond [`DART_IOVA_LIMIT`].
    pub fn map_range(&self, phys: u64, size: usize) -> Result<(), DartError> {
        let (start, end) = dart_span(phys, size).ok_or(DartError::OutOfRange)?;
        if end > DART_IOVA_LIMIT {
            return Err(DartError::OutOfRange);
        }
        let mut pa = start;
        while pa < end {
            self.map_page(pa);
            pa += DART_PAGE_SIZE as u64;
        }
        Ok(())
    }

    fn map_page(&self, pa: u64) {
        let l1_idx = ((pa >> 25) & 0x7FF) as usize;
        let l2_idx = ((pa >> 14) & 0x7FF) as usize;
        let l2_table_phys = self.get_or_alloc_l2(l1_idx);
        let l2_virt = phys_to_virt_inner(l2_table_phys);
        let pte_addr = (l2_virt + l2_idx * 8) as *mut u64;
        let pte = pa | DART_PTE_VALID | DART1_PTE_SP_DIS | DART_PTE_SUBPAGE_FULL;
        unsafe {
            pte_addr.write_volatile(pte);
        }
    }

    fn get_or_alloc_l2(&self, l1_idx: usize) -> u64 {
        let entry_ptr = (self.root_virt + l1_idx * 8) as *mut u64;
        let current = unsafe { entry_ptr.read_volatile() };
        if current & DART_PTE_VALID != 0 {
            current & DART_PTE_ADDR_MASK
        } else {
            let (_, p) = alloc_dart_page();
            let entry = (p & DART_PTE_ADDR_MASK) | DART_PTE_VALID;
            unsafe {
                entry_ptr.write_volatile(entry);
            }
            p
        }
    }

    /// Clear identity mappings for [phys, phys+size), returning the number of 16 KiB pages unmapped.
    pub fn unmap_range(&self, phys: u64, size: usize) -> usize {
        let Some((start, end)) = dart_span(phys, size) else {
            return 0;
        };
        // Pages at or beyond the limit were never mappable.
        let end = end.min(DART_IOVA_LIMIT);
        let mut pa = start;
        let mut count = 0;
        while pa < end {
            if self.unmap_page(pa) {
                count += 1;
            }
            pa += DART_PAGE_SIZE as u64;
        }
        count
    }

    fn unmap_page(&self, pa: u64) -> bool {
        let l1_idx = ((pa >> 25) & 0x7FF) as usize;
        let l2_idx = ((pa >> 14) & 0x7FF) as usize;
        let l1_entry_ptr = (self.root_virt + l1_idx * 8) as *const u64;
        let l1_entry = unsafe { l1_entry_ptr.read_volatile() };
        if l1_entry & DART_PTE_VALID == 0 {
            return false;
        }
        let l2_table_phys = l1_entry & DART_PTE_ADDR_MASK;
        let l2_virt = phys_to_virt_inner(l2_table_phys);
        let pte_addr = (l2_virt + l2_idx * 8) as *mut u64;
        let old = unsafe { pte_addr.read_volatile() };
        if old & DART_PTE_VALID != 0 {
            unsafe {
                pte_addr.write_volatile(0);
            }
            true
        } else {
            false
        }
    }

    /// Apple DART stream bypass is forbidden under DmaIsolation::Required.
    pub fn set_bypass(&self, enable: bool) -> Result<(), DartError> {
        if enable {
            Err(DartError::BypassForbidden)
        } else {
            Ok(())
        }
    }

    pub fn is_bypass_enabled(&self) -> bool {
        false
    }
    #[cfg(any(test, feature = "test-hooks"))]
    #[allow(dead_code)]
    pub(crate) fn leaf_present(&self, pa: u64) -> bool {
        let l1_idx = ((pa >> 25) & 0x7FF) as usize;
        let l2_idx = ((pa >> 14) & 0x7FF) as usize;
        let l1_entry_ptr = (self.root_virt + l1_idx * 8) as *const u64;
        let l1_entry = unsafe { l1_entry_ptr.read_volatile() };
        if l1_entry & DART_PTE_VALID == 0 {
            return false;
        }
        let l2_table_phys = l1_entry & DART_PTE_ADDR_MASK;
        let l2_virt = phys_to_virt_inner(l2_table_phys);
        let pte_addr = (l2_virt + l2_idx * 8) as *const u64;
        unsafe { pte_addr.read_volatile() & DART_PTE_VALID != 0 }
    }
    #[cfg(any(test, feature = "test-hooks"))]
    #[allow(dead_code)]
    pub(crate) fn level1_present(&self, pa: u64) -> bool {
        let l1_idx = ((pa >> 25) & 0x7FF) as usize;
        let l1_entry_ptr = (self.root_virt + l1_idx * 8) as *const u64;
        unsafe { l1_entry_ptr.read_volatile() & DART_PTE_VALID != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dart_16k_mapping_and_bypass_refusal() {
        let pt = AppleDartPt::new();
        assert_eq!(pt.set_bypass(true), Err(DartError::BypassForbidden));
        assert!(!pt.is_bypass_enabled());

        const BASE: u64 = 0x8_0000_0000; // 32 GiB base on Apple Silicon M1
        const PAGE_16K: usize = 16384;

        // Map 3 pages (48 KiB)
        assert_eq!(pt.map_range(BASE, 3 * PAGE_16K), Ok(()));
        assert!(pt.leaf_present(BASE));
        assert!(pt.leaf_present(BASE + PAGE_16K as u64));
        assert!(pt.leaf_present(BASE + 2 * PAGE_16K as u64));
        assert!(!pt.leaf_present(BASE + 3 * PAGE_16K as u64));

        // Unmap middle page
        let unmapped = pt.unmap_range(BASE + PAGE_16K as u64, PAGE_16K);
        assert_eq!(unmapped, 1);
        assert!(pt.leaf_present(BASE));
        assert!(!pt.leaf_present(BASE + PAGE_16K as u64));
        assert!(pt.leaf_present(BASE + 2 * PAGE_16K as u64));

        // Unmapping again is idempotent
        assert_eq!(pt.unmap_range(BASE + PAGE_16K as u64, PAGE_16K), 0);

        // Remap middle page
        assert_eq!(pt.map_range(BASE + PAGE_16K as u64, PAGE_16K), Ok(()));
        assert!(pt.leaf_present(BASE + PAGE_16K as u64));

        // A range crossing the 36-bit table reach is refused whole instead of
        // aliasing into a low L1 slot.
        let last = DART_IOVA_LIMIT - PAGE_16K as u64;
        assert_eq!(pt.map_range(last, 2 * PAGE_16K), Err(DartError::OutOfRange));
        assert!(!pt.level1_present(last));
        assert_eq!(
            pt.map_range(u64::MAX - 1, PAGE_16K),
            Err(DartError::OutOfRange)
        );
        assert_eq!(pt.unmap_range(DART_IOVA_LIMIT, PAGE_16K), 0);
    }

    #[test]
    fn vtd_slpt_maps_and_unmaps_for_a_non_snooping_unit() {
        // coherent=false writes every table store back with CLFLUSH.
        for coherent in [false, true] {
            let pt = VtdSlpt::new(coherent);
            pt.map_range(0x4020_0000, 0x2000);
            assert_eq!(pt.unmap_range(0x4020_0000, 0x3000), 2);
            assert_eq!(pt.unmap_range(0x4020_0000, 0x2000), 0);
        }
    }

    /// ADR-0033 CP1 D-G3/D-G10: an export window maps non-identity IOVAs in
    /// `[2^38, 2^39)`, `R`-only unless the export grants device writes, never
    /// over a present leaf, and unmaps with the same leaf-only teardown.
    #[test]
    fn vtd_slpt_export_window_is_non_identity_and_rights_exact() {
        const WINDOW: u64 = 1 << 38;
        const PHYS: u64 = 0x4060_0000;
        let pt = VtdSlpt::new(true);
        pt.map_range(PHYS, 0x1000);
        assert!(pt.window_free(WINDOW, 0x3000));
        assert!(!pt.window_free(PHYS, 0x1000), "identity leaf is not free");

        pt.map_window(WINDOW, PHYS, 0x2000, false);
        assert_eq!(pt.leaf_at(WINDOW), Some(PHYS | VTD_R));
        assert_eq!(pt.leaf_at(WINDOW + 0x1000), Some((PHYS + 0x1000) | VTD_R));
        assert!(!pt.window_free(WINDOW + 0x1000, 0x1000));
        assert!(pt.window_free(WINDOW + 0x2000, 0x1000));
        // The identity leaf of the same frame is untouched.
        assert_eq!(pt.leaf_at(PHYS), Some(PHYS | VTD_RW));

        pt.map_window(WINDOW + 0x4000, PHYS, 0x1000, true);
        assert_eq!(pt.leaf_at(WINDOW + 0x4000), Some(PHYS | VTD_RW));

        assert_eq!(pt.unmap_range(WINDOW, 0x5000), 3);
        assert!(pt.window_free(WINDOW, 0x5000));
        assert_eq!(pt.leaf_at(PHYS), Some(PHYS | VTD_RW));
    }
}
