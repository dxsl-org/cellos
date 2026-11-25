//! x86 nested-paging (Intel EPT / AMD NPT) builder for the ViCell hypervisor.
//!
//! The x86 analog of [`crate::memory::stage2`] (ARM64 Stage-2). One 4-level tree per
//! guest: PML4 → PDPT → PD → PT, 4 KiB leaves, and 2 MiB leaves in the PD where
//! the table allows them ([`NestedPageTable::allow_large_leaves`]). Guest physical
//! address (GPA) base is **0** — x86 Linux/PVH expects low RAM starting at 0,
//! unlike ARM's IPA 0x4000_0000.
//!
//! # Invariants (enforced, mirror Stage-2)
//! - Guest RAM GPA range maps exclusively into the carved HPA region (SAS
//!   isolation, Law 4) — `map()` rejects any HPA outside the carve.
//! - Emulated-MMIO GPAs ([`MMIO_HOLES`]) are intentionally **unmapped** so guest
//!   accesses fault out (EPT-violation reason 48 / SVM NPF) to the hypervisor
//!   cell. Frozen before the first VM-entry.
//! - `Drop` frees every frame — no leak on VM teardown (Law 8).
//!
//! # EPT vs NPT leaf format (Intel SDM Vol.3 28.2.2 / AMD APM Vol.2 15.25.5)
//! Both are 4-level 512-entry trees; only the descriptor bits differ:
//! - **EPT** leaf: R(0) W(1) X(2) | memtype[5:3] (WB=6) | IPAT(6). Table
//!   (non-leaf) entries set R|W|X to permit the walk.
//! - **NPT** leaf: reuses ordinary x86 PTE bits — P(0) RW(1) US(2) | NX(63).
//!   Table entries set P|RW|US.

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::memory::cell_quota::Reservation;
use crate::memory::frame::{allocate_guest_ram, phys_to_virt};
use crate::memory::frame_run::{Batch, PrivateRun, RunKind};
use crate::memory::paging::PAGE_SIZE;

/// Which nested-paging format this tree encodes, selected at VM-create time
/// from the CPU vendor latched in `cpu_features`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NestedFormat {
    /// Intel EPT (R/W/X + memory-type leaf bits).
    Ept,
    /// AMD NPT (ordinary x86 PTE bits + NX).
    Npt,
}

// ── x86 4-level index math (identical for EPT and NPT) ───────────────────────

const PML4_SHIFT: u32 = 39; // bits[47:39]
const PDPT_SHIFT: u32 = 30; // bits[38:30]
const PD_SHIFT: u32 = 21; // bits[29:21]
const PT_SHIFT: u32 = 12; // bits[20:12]
const IDX_MASK: u64 = 0x1FF; // 9 bits, 512 entries

/// 52-bit guest-physical limit (x86 max PA width in the MVP).
const GPA_LIMIT: u64 = 1 << 52;

/// Output-address mask: bits[51:12].
const PA_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ── EPT descriptor bits ──────────────────────────────────────────────────────
const EPT_READ: u64 = 1 << 0;
const EPT_WRITE: u64 = 1 << 1;
const EPT_EXEC: u64 = 1 << 2;
/// Leaf memory type WB (6) in bits[5:3]; IPAT(6)=0 so the type is honoured.
const EPT_MEMTYPE_WB: u64 = 6 << 3;

// ── NPT descriptor bits (ordinary x86 PTE) ───────────────────────────────────
const NPT_PRESENT: u64 = 1 << 0;
const NPT_WRITE: u64 = 1 << 1;
const NPT_USER: u64 = 1 << 2;

/// Bit 7 of a page-directory entry, in both formats: the entry maps a 2 MiB
/// page itself instead of pointing at a page table.
const LARGE_LEAF: u64 = 1 << 7;
const LARGE_BYTES: u64 = 2 * 1024 * 1024;
/// 4 KiB pages under one 2 MiB leaf.
const LARGE_PAGES: u64 = 512;

// ── MMIO GPAs left unmapped before VM-entry (mirror ARM MMIO_HOLES) ──────────

/// GPA ranges intentionally absent from the tree so guest accesses trap.
/// The 16550 UART is **port I/O** (0x3F8), not MMIO — it needs no hole (the
/// I/O-exit path in P03 handles it). Frozen before the first VM-entry.
pub const MMIO_HOLES: &[(u64, u64)] = &[
    (0xd000_0000, 0xd000_4000), // virtio-mmio bus: 4 slots × 0x1000 (P06/P07/P08)
    (0xFEC0_0000, 0xFEC0_1000), // IOAPIC (unmodelled; no MADT IOAPIC entry → guest uses virtual-wire PIC)
                                // LAPIC (0xFEE00000) is NOT a hole: it is RAM-backed via map_device_frame
                                // (TCG has no DecodeAssist, so trap-and-emulate is impossible). The timer is
                                // polled kernel-side on HLT (see hal svm_vcpu). Guest RAM never reaches it.
];

// ── Errors (mirror S2MapError) ───────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum EptMapError {
    /// GPA or HPA range wraps (checked_add overflow, C-x1).
    Overflow,
    /// GPA or HPA exceeds the 52-bit PA space limit.
    OutOfBounds,
    /// HPA is outside the guest's carved RAM region (SAS isolation).
    SasViolation,
    /// Frame allocator exhausted.
    OutOfMemory,
    /// Attempt to map a reserved MMIO hole.
    MmioHole,
}

#[inline]
fn pml4_idx(gpa: u64) -> usize {
    ((gpa >> PML4_SHIFT) & IDX_MASK) as usize
}
#[inline]
fn pdpt_idx(gpa: u64) -> usize {
    ((gpa >> PDPT_SHIFT) & IDX_MASK) as usize
}
#[inline]
fn pd_idx(gpa: u64) -> usize {
    ((gpa >> PD_SHIFT) & IDX_MASK) as usize
}
#[inline]
fn pt_idx(gpa: u64) -> usize {
    ((gpa >> PT_SHIFT) & IDX_MASK) as usize
}
#[inline]
fn entry_pa(e: u64) -> usize {
    (e & PA_MASK) as usize
}

/// Source of [`NestedPageTable::epoch`]. Starts at 1: 0 is a CPU's record
/// before it has invalidated for any table.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

/// x86 nested page table (EPT or NPT) for one VM.
///
/// **Root** is a single 4 KiB PML4 frame (naturally 4 KiB-aligned — EPTP/nCR3
/// only require 4 KiB alignment, unlike ARM's 8 KiB concatenated root).
///
/// **Safety:** the raw pointer fields are valid while `self` lives; pointed-to
/// frames are freed in `Drop`. Single-CPU kernel context only (QEMU TCG).
pub struct NestedPageTable {
    format: NestedFormat,
    root_pa: u64,
    root_va: *mut u64,
    /// The root and every L1/L2/L3 sub-table, root first; freed in `Drop`.
    tables: Vec<PrivateRun>,
    /// Carved guest-RAM region for the SAS-isolation assertion in `map()`.
    guest_ram_pa: u64,
    guest_ram_pages: usize,
    /// The carved guest RAM itself; freed in `Drop`.
    guest_ram: Option<PrivateRun>,
    /// The owner's guest charge every frame of the table is drawn from before
    /// it is allocated; a frame the rest cannot cover is refused. The rest is
    /// refunded when the table drops.
    budget: Reservation,
    /// Names the translations this table gives right now: drawn fresh when the
    /// table is made and after every change to a leaf, and never drawn twice.
    /// A CPU that ran `INVEPT` for the root at this value holds nothing stale
    /// for it — neither from an earlier state of this table nor from a freed
    /// table whose root frame this one reuses.
    epoch: u64,
    /// `map` may write 2 MiB leaves (see [`Self::allow_large_leaves`]).
    large_leaves: bool,
}

// SAFETY: not Send/Sync by default (raw `*mut u64`); only touched from
// single-CPU kernel context in the current TCG bring-up (mirror Stage2Table).
unsafe impl Send for NestedPageTable {}

impl NestedPageTable {
    /// Frames of the PML4 root.
    pub const ROOT_PAGES: usize = 1;

    /// Most PDPT, PD and PT tables mapping `pages` contiguous 4 KiB pages at
    /// any base can need (with 4 KiB leaves, the most tables a mapping takes):
    /// one PT per 2 MiB, one PD per 1 GiB and one PDPT per 512 GiB the range
    /// touches.
    pub const fn table_bound(pages: usize) -> usize {
        pages.div_ceil(512)
            + 1
            + pages.div_ceil(512 * 512)
            + 1
            + pages.div_ceil(512 * 512 * 512)
            + 1
    }

    /// Allocate a new nested-paging root (one 4 KiB PML4 frame, zeroed),
    /// drawing it and every later frame of the table from `budget`.
    ///
    /// Returns `None` when `budget` cannot cover the root (nothing is
    /// allocated) or the frame allocator is exhausted.
    pub(crate) fn new(format: NestedFormat, mut budget: Reservation) -> Option<Self> {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        let root_charge = budget.split(Self::ROOT_PAGES * PAGE_SIZE)?;
        let root = PrivateRun::adopt(
            crate::memory::frame::allocate_frame()?,
            Self::ROOT_PAGES,
            root_charge,
            RunKind::PageTable,
        );
        let root_pa = root.base() as u64;
        let root_va = phys_to_virt(root_pa as usize) as *mut u64;
        // SAFETY: freshly allocated frame is exclusively ours; 512 entries.
        unsafe {
            core::ptr::write_bytes(root_va, 0, 512);
        }
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::created_frame(
            root_pa,
            match format {
                NestedFormat::Npt => 1,
                NestedFormat::Ept => 2,
            },
        );
        Some(Self {
            format,
            root_pa,
            root_va,
            tables: alloc::vec![root],
            guest_ram_pa: 0,
            guest_ram_pages: 0,
            guest_ram: None,
            budget,
            epoch: next_epoch(),
            large_leaves: false,
        })
    }

    /// Let [`Self::map`] write a 2 MiB leaf wherever a whole 2 MiB of guest and
    /// host addresses lines up: one entry instead of a page table of 512, and
    /// a walk one level shorter for the CPU and for the hypervisor under it.
    /// Only for a format the CPU walks large leaves in (EPT: 2 MiB pages in
    /// `IA32_VMX_EPT_VPID_CAP`); unmapping part of a large leaf splits it.
    pub(crate) fn allow_large_leaves(&mut self) {
        self.large_leaves = true;
    }

    /// Physical address of the PML4 root (for the EPTP/nCR3 formatters).
    #[inline]
    pub fn root_pa(&self) -> u64 {
        self.root_pa
    }

    /// EPTP control value (Intel VMCS field): root | WB memtype | walk-length-1.
    ///
    /// Bits[2:0] = memory type WB(6); bits[5:3] = page-walk length − 1 = 3
    /// (4 levels); bit6 = accessed/dirty (left 0 in the MVP).
    pub fn eptp(&self) -> u64 {
        (self.root_pa & PA_MASK) | 6 | (3 << 3)
    }

    /// See the field: what an entering CPU compares with its own record to
    /// decide whether it must `INVEPT` first.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// nCR3 control value (AMD VMCB field): the plain root PA.
    pub fn ncr3(&self) -> u64 {
        self.root_pa & PA_MASK
    }

    /// Carve `n_pages` contiguous frames for guest RAM (chunked scan, M2-safe),
    /// drawn from the table's budget first (refused, with nothing allocated,
    /// when the rest cannot cover them), recording the region so `map()` can
    /// enforce SAS isolation. A table carves once: a second carve is refused
    /// rather than dropping RAM the table may already map.
    pub fn carve_guest_ram(&mut self, n_pages: usize) -> Option<u64> {
        if self.guest_ram.is_some() {
            return None;
        }
        let charge = self.budget.split(n_pages.checked_mul(PAGE_SIZE)?)?;
        let pa = allocate_guest_ram(n_pages)?;
        self.guest_ram = Some(PrivateRun::adopt(pa, n_pages, charge, RunKind::Leaf));
        self.guest_ram_pa = pa as u64;
        self.guest_ram_pages = n_pages;
        Some(pa as u64)
    }

    // ── Descriptor encoders (vendor-dispatched) ──────────────────────────────

    /// Non-leaf (table-pointer) descriptor to the next level.
    #[inline]
    fn table_desc(&self, next_pa: u64) -> u64 {
        let perms = match self.format {
            NestedFormat::Ept => EPT_READ | EPT_WRITE | EPT_EXEC,
            NestedFormat::Npt => NPT_PRESENT | NPT_WRITE | NPT_USER,
        };
        (next_pa & PA_MASK) | perms
    }

    /// Leaf (page) descriptor for guest RAM.
    #[inline]
    fn leaf_desc(&self, hpa: u64, writable: bool) -> u64 {
        match self.format {
            NestedFormat::Ept => {
                let w = if writable { EPT_WRITE } else { 0 };
                (hpa & PA_MASK) | EPT_READ | w | EPT_EXEC | EPT_MEMTYPE_WB
            }
            NestedFormat::Npt => {
                let w = if writable { NPT_WRITE } else { 0 };
                (hpa & PA_MASK) | NPT_PRESENT | w | NPT_USER
            }
        }
    }

    /// True if `e` is a present entry in this format.
    #[inline]
    fn is_present(&self, e: u64) -> bool {
        match self.format {
            // EPT: any of R/W/X set means present.
            NestedFormat::Ept => e & (EPT_READ | EPT_WRITE | EPT_EXEC) != 0,
            NestedFormat::Npt => e & NPT_PRESENT != 0,
        }
    }

    fn alloc_subtable(&mut self) -> Option<(*mut u64, u64)> {
        let charge = self.budget.split(PAGE_SIZE)?;
        let pa = crate::memory::frame::allocate_frame()?;
        let va = phys_to_virt(pa) as *mut u64;
        // SAFETY: freshly allocated frame is exclusively ours; 512 entries.
        unsafe {
            core::ptr::write_bytes(va, 0, 512);
        }
        self.tables
            .push(PrivateRun::adopt(pa, 1, charge, RunKind::PageTable));
        Some((va, pa as u64))
    }

    /// Pages the root and sub-tables take (each table is one page).
    pub fn table_pages(&self) -> usize {
        self.tables.len()
    }

    /// Map `n_pages` × 4 KiB at guest `gpa` → host `hpa`.
    ///
    /// All-or-nothing: every interior table the range needs is allocated
    /// before the first leaf is written, so a failed call leaves the existing
    /// mapping exactly as it was (no partial window into host memory).
    ///
    /// # Errors
    /// * [`EptMapError::Overflow`] — the byte span or `gpa`/`hpa` + span wraps.
    /// * [`EptMapError::OutOfBounds`] — range exceeds the 52-bit PA limit.
    /// * [`EptMapError::MmioHole`] — range overlaps a reserved MMIO hole.
    /// * [`EptMapError::SasViolation`] — the range is larger than, or escapes,
    ///   the carved guest RAM.
    /// * [`EptMapError::OutOfMemory`] — sub-table allocation failed.
    pub fn map(
        &mut self,
        gpa: u64,
        hpa: u64,
        n_pages: usize,
        writable: bool,
    ) -> Result<(), EptMapError> {
        if self.guest_ram_pages > 0 && n_pages > self.guest_ram_pages {
            return Err(EptMapError::SasViolation);
        }
        // An unchecked product wraps to a tiny span for n_pages ≥ 2^52, which
        // would let every range check below pass while the loop walks far past
        // the carve (C-2).
        let span = (n_pages as u64)
            .checked_mul(PAGE_SIZE as u64)
            .ok_or(EptMapError::Overflow)?;
        let gpa_end = gpa.checked_add(span).ok_or(EptMapError::Overflow)?;
        let hpa_end = hpa.checked_add(span).ok_or(EptMapError::Overflow)?;
        if gpa_end > GPA_LIMIT || hpa_end > GPA_LIMIT {
            return Err(EptMapError::OutOfBounds);
        }
        for &(hole_base, hole_end) in MMIO_HOLES {
            if gpa < hole_end && gpa_end > hole_base {
                return Err(EptMapError::MmioHole);
            }
        }
        if self.guest_ram_pages > 0 {
            let guest_end = (self.guest_ram_pages as u64)
                .checked_mul(PAGE_SIZE as u64)
                .and_then(|bytes| self.guest_ram_pa.checked_add(bytes))
                .ok_or(EptMapError::Overflow)?;
            if hpa < self.guest_ram_pa || hpa_end > guest_end {
                return Err(EptMapError::SasViolation);
            }
        }
        // Pass 1: build every interior level. The only fallible step (a split
        // of a 2 MiB leaf here keeps its translations).
        let pages = n_pages as u64;
        let mut page = 0u64;
        while page < pages {
            let offset = page * PAGE_SIZE as u64;
            if self.takes_large_leaf(gpa + offset, hpa + offset, pages - page)? {
                page += LARGE_PAGES;
            } else {
                self.leaf_table(gpa + offset)?;
                page += 1;
            }
        }
        // Pass 2: write leaves. Cannot fail — every table now exists, and each
        // range decides as it did in pass 1.
        let mut page = 0u64;
        while page < pages {
            let offset = page * PAGE_SIZE as u64;
            if self.takes_large_leaf(gpa + offset, hpa + offset, pages - page)? {
                let slot = self.pd_slot(gpa + offset)?;
                let leaf = self.leaf_desc(hpa + offset, writable) | LARGE_LEAF;
                // SAFETY: slot is a live page-directory entry.
                unsafe {
                    *slot = leaf;
                }
                page += LARGE_PAGES;
            } else {
                self.map_single(gpa + offset, hpa + offset, writable)?;
                page += 1;
            }
        }
        self.invalidate();
        Ok(())
    }

    /// Map a single kernel-owned device frame at `gpa` → `hpa`, bypassing the
    /// guest-RAM SAS carve check and the [`MMIO_HOLES`] reservation.
    ///
    /// Used only for hypervisor-owned emulated-device pages the kernel
    /// allocates and frees itself — the RAM-backed xAPIC window (0xFEE00000),
    /// where per-access trap-and-emulate is impossible under QEMU TCG (empty
    /// DecodeAssist → no instruction bytes on the NPF). `hpa` must be a
    /// kernel-allocated frame the caller tracks for teardown; it is deliberately
    /// **not** guest RAM, so the SAS check that `map()` enforces does not apply.
    pub fn map_device_frame(
        &mut self,
        gpa: u64,
        hpa: u64,
        writable: bool,
    ) -> Result<(), EptMapError> {
        if gpa >= GPA_LIMIT || hpa >= GPA_LIMIT {
            return Err(EptMapError::OutOfBounds);
        }
        self.map_single(gpa, hpa, writable)?;
        self.invalidate();
        Ok(())
    }

    /// Unmap `n_pages` starting at `gpa`; silently skips unmapped pages and
    /// stops at the GPA limit. Sub-tables are freed only in `Drop`.
    pub fn unmap(&mut self, gpa: u64, n_pages: usize) {
        for page in 0..n_pages as u64 {
            let Some(cur) = page
                .checked_mul(PAGE_SIZE as u64)
                .and_then(|offset| gpa.checked_add(offset))
                .filter(|cur| *cur < GPA_LIMIT)
            else {
                break;
            };
            self.unmap_single(cur);
        }
        self.invalidate();
    }

    /// A leaf changed: move to a new epoch, so that every CPU invalidates
    /// before it next enters under this root, and drop this context's cached
    /// translations on the current CPU now.
    ///
    /// EPT guest-physical mappings are tagged by EP4TA and survive VM exits, so
    /// a cleared or rewritten leaf stays live until `INVEPT`. `INVEPT` is
    /// CPU-local: the registry invalidates on the entering CPU whenever that
    /// CPU's record is not this epoch, which covers CPUs this call cannot
    /// reach. NPT needs nothing here — every VMRUN flushes the guest's ASID.
    fn invalidate(&mut self) {
        self.epoch = next_epoch();
        if self.format == NestedFormat::Ept && crate::cpu_features::has_x86_virt() {
            // SAFETY: an EPT table exists only on the VMX backend, and
            // `has_x86_virt` means every online CPU completed VMXON (an AP that
            // fails it never comes online).
            if let Err(e) = unsafe { hal::x86_64::invept::invalidate_context(self.eptp()) } {
                log::error!("[ept] INVEPT root={:#x} failed: {:?}", self.root_pa, e);
            }
        }
    }

    /// Software walk: resolve `gpa` to its mapped HPA (for the P02 test-hook and
    /// diagnostics). `None` if any level along the walk is not present.
    pub fn translate(&self, gpa: u64) -> Option<u64> {
        // SAFETY: root_va valid for 512 entries; indices are masked to 9 bits.
        let pml4e = unsafe { *self.root_va.add(pml4_idx(gpa)) };
        if !self.is_present(pml4e) {
            return None;
        }
        let pdpt = phys_to_virt(entry_pa(pml4e)) as *const u64;
        let pdpte = unsafe { *pdpt.add(pdpt_idx(gpa)) };
        if !self.is_present(pdpte) {
            return None;
        }
        let pd = phys_to_virt(entry_pa(pdpte)) as *const u64;
        let pde = unsafe { *pd.add(pd_idx(gpa)) };
        if !self.is_present(pde) {
            return None;
        }
        if pde & LARGE_LEAF != 0 {
            return Some((pde & PA_MASK & !(LARGE_BYTES - 1)) | (gpa & (LARGE_BYTES - 1)));
        }
        let pt = phys_to_virt(entry_pa(pde)) as *const u64;
        let pte = unsafe { *pt.add(pt_idx(gpa)) };
        if !self.is_present(pte) {
            return None;
        }
        Some((pte & PA_MASK) | (gpa & 0xFFF))
    }

    // ── Single-page walk ─────────────────────────────────────────────────────

    /// Whether the range from `gpa` (to `hpa`, `pages_left` pages long) starts
    /// with a 2 MiB leaf: large leaves allowed, both addresses 2 MiB-aligned,
    /// 2 MiB left to map, and the page-directory entry not already pointing at
    /// a page table (whose other leaves would be lost). Allocates the upper
    /// levels of a candidate; changes no leaf.
    fn takes_large_leaf(
        &mut self,
        gpa: u64,
        hpa: u64,
        pages_left: u64,
    ) -> Result<bool, EptMapError> {
        if !self.large_leaves
            || !gpa.is_multiple_of(LARGE_BYTES)
            || !hpa.is_multiple_of(LARGE_BYTES)
            || pages_left < LARGE_PAGES
        {
            return Ok(false);
        }
        // SAFETY: pd_slot returns a live page-directory entry.
        let entry = unsafe { *self.pd_slot(gpa)? };
        Ok(!self.is_present(entry) || entry & LARGE_LEAF != 0)
    }

    /// The page-directory entry covering `gpa`, allocating missing upper
    /// levels. Writes only table descriptors.
    fn pd_slot(&mut self, gpa: u64) -> Result<*mut u64, EptMapError> {
        let mut level_va = self.root_va;
        for &idx in &[pml4_idx(gpa), pdpt_idx(gpa)] {
            // SAFETY: level_va valid for 512 entries; idx < 512.
            let slot = unsafe { level_va.add(idx) };
            let e = unsafe { *slot };
            level_va = if self.is_present(e) {
                phys_to_virt(entry_pa(e)) as *mut u64
            } else {
                let (va, pa) = self.alloc_subtable().ok_or(EptMapError::OutOfMemory)?;
                let desc = self.table_desc(pa);
                // SAFETY: slot valid; descriptor bits cannot overlap PA field.
                unsafe {
                    *slot = desc;
                }
                va
            };
        }
        // SAFETY: level_va is a live page directory; pd_idx < 512.
        Ok(unsafe { level_va.add(pd_idx(gpa)) })
    }

    /// Walk to the page table covering `gpa`, allocating missing interior
    /// levels and splitting a 2 MiB leaf in the way. Writes only table
    /// descriptors, never a leaf of its own.
    fn leaf_table(&mut self, gpa: u64) -> Result<*mut u64, EptMapError> {
        let slot = self.pd_slot(gpa)?;
        // SAFETY: slot is a live page-directory entry.
        let e = unsafe { *slot };
        if self.is_present(e) {
            return if e & LARGE_LEAF != 0 {
                self.split(slot, e)
            } else {
                Ok(phys_to_virt(entry_pa(e)) as *mut u64)
            };
        }
        let (va, pa) = self.alloc_subtable().ok_or(EptMapError::OutOfMemory)?;
        let desc = self.table_desc(pa);
        // SAFETY: slot valid; descriptor bits cannot overlap PA field.
        unsafe {
            *slot = desc;
        }
        Ok(va)
    }

    /// Replace the 2 MiB leaf `large` in `slot` by a page table of 512 4 KiB
    /// leaves with the same output addresses and permissions: the translations
    /// stay as they were. Returns the new page table.
    fn split(&mut self, slot: *mut u64, large: u64) -> Result<*mut u64, EptMapError> {
        let (va, pa) = self.alloc_subtable().ok_or(EptMapError::OutOfMemory)?;
        let base = large & PA_MASK & !(LARGE_BYTES - 1);
        let writable = large & (EPT_WRITE | NPT_WRITE) != 0;
        for index in 0..LARGE_PAGES {
            let leaf = self.leaf_desc(base + index * PAGE_SIZE as u64, writable);
            // SAFETY: va is the fresh page table; index < 512.
            unsafe {
                *va.add(index as usize) = leaf;
            }
        }
        let desc = self.table_desc(pa);
        // SAFETY: slot is a live page-directory entry.
        unsafe {
            *slot = desc;
        }
        Ok(va)
    }

    /// Write one leaf. The caller invalidates once its whole batch is written.
    fn map_single(&mut self, gpa: u64, hpa: u64, writable: bool) -> Result<(), EptMapError> {
        let pt = self.leaf_table(gpa)?;
        let leaf = self.leaf_desc(hpa, writable);
        // SAFETY: pt is a live page table; pt_idx < 512.
        unsafe {
            *pt.add(pt_idx(gpa)) = leaf;
        }
        Ok(())
    }

    fn unmap_single(&mut self, gpa: u64) {
        let mut level_va = self.root_va;
        for &idx in &[pml4_idx(gpa), pdpt_idx(gpa)] {
            // SAFETY: level_va valid for 512 entries; idx < 512.
            let e = unsafe { *level_va.add(idx) };
            if !self.is_present(e) {
                return;
            }
            level_va = phys_to_virt(entry_pa(e)) as *mut u64;
        }
        // SAFETY: level_va is a live page directory; pd_idx < 512.
        let slot = unsafe { level_va.add(pd_idx(gpa)) };
        let e = unsafe { *slot };
        if !self.is_present(e) {
            return;
        }
        let pt = if e & LARGE_LEAF != 0 {
            match self.split(slot, e) {
                Ok(pt) => pt,
                // No frame for the page table: the whole 2 MiB goes. That takes
                // away more than was asked and never leaves a page mapped that
                // the caller unmapped.
                Err(_) => {
                    // SAFETY: slot is a live page-directory entry.
                    unsafe {
                        *slot = 0;
                    }
                    return;
                }
            }
        } else {
            phys_to_virt(entry_pa(e)) as *mut u64
        };
        // SAFETY: pt is the page table; clear the leaf.
        unsafe {
            *pt.add(pt_idx(gpa)) = 0;
        }
    }
}

impl Drop for NestedPageTable {
    fn drop(&mut self) {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        // Translations must be gone before the root, tables or guest RAM can be
        // handed out again: a later VM whose root lands on the same frame would
        // otherwise resolve GPAs through this VM's cached entries.
        if self.format == NestedFormat::Ept && crate::cpu_features::has_x86_virt() {
            // SAFETY: as in `invalidate` — VMX backend, every online CPU in VMX operation.
            if let Err(e) = unsafe { hal::x86_64::invept::invalidate_all(self.eptp()) } {
                log::error!(
                    "[ept] teardown INVEPT root={:#x} failed: {:?}",
                    self.root_pa,
                    e
                );
            }
        }
        // An INVEPT error is logged and the frames are still freed, as
        // before: a later table whose root lands on these frames has another
        // epoch, so every CPU invalidates that root before it first enters
        // under it and never walks this VM's entries. NPT needs nothing: ASIDs
        // are never reused and every VMRUN flushes its guest's ASID.
        let mut batch = Batch::new();
        for run in self.tables.drain(..) {
            run.retire_into(&mut batch);
        }
        if let Some(run) = self.guest_ram.take() {
            run.retire_into(&mut batch);
        }
        let _ = batch.complete();
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::released(self.root_pa);
    }
}

#[cfg(test)]
#[path = "ept_tests.rs"]
mod tests;
