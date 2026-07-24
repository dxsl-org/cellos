//! x86 nested-paging (Intel EPT / AMD NPT) builder for the ViCell hypervisor.
//!
//! The x86 analog of [`super::stage2`] (ARM64 Stage-2). One 4-level tree per
//! guest: PML4 → PDPT → PD → PT, 4 KiB frames throughout (no large pages in the
//! MVP). Guest physical address (GPA) base is **0** — x86 Linux/PVH expects low
//! RAM starting at 0, unlike ARM's IPA 0x4000_0000.
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

use super::frame::{allocate_guest_ram, phys_to_virt, FRAME_ALLOCATOR};
use super::paging::PAGE_SIZE;

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
    /// L1/L2/L3 sub-table frames, tracked for `Drop`.
    sub_frames: Vec<usize>,
    /// Carved guest-RAM region for the SAS-isolation assertion in `map()`.
    guest_ram_pa: u64,
    guest_ram_pages: usize,
}

// SAFETY: not Send/Sync by default (raw `*mut u64`); only touched from
// single-CPU kernel context in the current TCG bring-up (mirror Stage2Table).
unsafe impl Send for NestedPageTable {}

impl NestedPageTable {
    /// Allocate a new nested-paging root (one 4 KiB PML4 frame, zeroed).
    ///
    /// Returns `None` when the frame allocator is exhausted.
    pub fn new(format: NestedFormat) -> Option<Self> {
        let root_pa = {
            let mut g = FRAME_ALLOCATOR.lock();
            g.as_mut()?.allocate_frame()? as u64
        };
        let root_va = phys_to_virt(root_pa as usize) as *mut u64;
        // SAFETY: freshly allocated frame is exclusively ours; 512 entries.
        unsafe {
            core::ptr::write_bytes(root_va, 0, 512);
        }
        Some(Self {
            format,
            root_pa,
            root_va,
            sub_frames: Vec::new(),
            guest_ram_pa: 0,
            guest_ram_pages: 0,
        })
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

    /// nCR3 control value (AMD VMCB field): the plain root PA.
    pub fn ncr3(&self) -> u64 {
        self.root_pa & PA_MASK
    }

    /// Carve `n_pages` contiguous frames for guest RAM (chunked scan, M2-safe),
    /// recording the region so `map()` can enforce SAS isolation.
    pub fn carve_guest_ram(&mut self, n_pages: usize) -> Option<u64> {
        let pa = allocate_guest_ram(n_pages)? as u64;
        self.guest_ram_pa = pa;
        self.guest_ram_pages = n_pages;
        Some(pa)
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
        let pa = {
            let mut g = FRAME_ALLOCATOR.lock();
            g.as_mut()?.allocate_frame()?
        };
        let va = phys_to_virt(pa) as *mut u64;
        // SAFETY: freshly allocated frame is exclusively ours; 512 entries.
        unsafe {
            core::ptr::write_bytes(va, 0, 512);
        }
        self.sub_frames.push(pa);
        Some((va, pa as u64))
    }

    /// Map `n_pages` × 4 KiB at guest `gpa` → host `hpa`.
    ///
    /// # Errors
    /// * [`EptMapError::Overflow`] — `gpa`/`hpa` + span wraps (C-x1).
    /// * [`EptMapError::OutOfBounds`] — range exceeds the 52-bit PA limit.
    /// * [`EptMapError::MmioHole`] — range overlaps a reserved MMIO hole.
    /// * [`EptMapError::SasViolation`] — `hpa` escapes the carved guest RAM.
    /// * [`EptMapError::OutOfMemory`] — sub-table allocation failed.
    pub fn map(
        &mut self,
        gpa: u64,
        hpa: u64,
        n_pages: usize,
        writable: bool,
    ) -> Result<(), EptMapError> {
        let span = (n_pages as u64) * PAGE_SIZE as u64;
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
            let guest_end = self.guest_ram_pa + (self.guest_ram_pages as u64 * PAGE_SIZE as u64);
            if hpa < self.guest_ram_pa || hpa_end > guest_end {
                return Err(EptMapError::SasViolation);
            }
        }
        let mut cur_gpa = gpa;
        let mut cur_hpa = hpa;
        for _ in 0..n_pages {
            self.map_single(cur_gpa, cur_hpa, writable)?;
            cur_gpa += PAGE_SIZE as u64;
            cur_hpa += PAGE_SIZE as u64;
        }
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
        self.map_single(gpa, hpa, writable)
    }

    /// Unmap `n_pages` starting at `gpa`; silently skips unmapped pages.
    /// Sub-tables are freed only in `Drop`.
    pub fn unmap(&mut self, gpa: u64, n_pages: usize) {
        let mut cur = gpa;
        for _ in 0..n_pages {
            self.unmap_single(cur);
            cur += PAGE_SIZE as u64;
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
        let pt = phys_to_virt(entry_pa(pde)) as *const u64;
        let pte = unsafe { *pt.add(pt_idx(gpa)) };
        if !self.is_present(pte) {
            return None;
        }
        Some((pte & PA_MASK) | (gpa & 0xFFF))
    }

    // ── Single-page walk ─────────────────────────────────────────────────────

    fn map_single(&mut self, gpa: u64, hpa: u64, writable: bool) -> Result<(), EptMapError> {
        // Descend/allocate three interior levels, then write the leaf.
        let mut level_va = self.root_va;
        for &idx in &[pml4_idx(gpa), pdpt_idx(gpa), pd_idx(gpa)] {
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
        // SAFETY: level_va is the PT; pt_idx < 512.
        let leaf = self.leaf_desc(hpa, writable);
        unsafe {
            *level_va.add(pt_idx(gpa)) = leaf;
        }
        Ok(())
    }

    fn unmap_single(&mut self, gpa: u64) {
        let mut level_va = self.root_va;
        for &idx in &[pml4_idx(gpa), pdpt_idx(gpa), pd_idx(gpa)] {
            // SAFETY: level_va valid for 512 entries; idx < 512.
            let e = unsafe { *level_va.add(idx) };
            if !self.is_present(e) {
                return;
            }
            level_va = phys_to_virt(entry_pa(e)) as *mut u64;
        }
        // SAFETY: level_va is the PT; clear the leaf.
        unsafe {
            *level_va.add(pt_idx(gpa)) = 0;
        }
    }
}

impl Drop for NestedPageTable {
    fn drop(&mut self) {
        let mut g = FRAME_ALLOCATOR.lock();
        if let Some(alloc) = g.as_mut() {
            for &pa in &self.sub_frames {
                alloc.deallocate_frame(pa);
            }
            alloc.deallocate_frame(self.root_pa as usize);
            if self.guest_ram_pages > 0 {
                let base = self.guest_ram_pa as usize;
                for i in 0..self.guest_ram_pages {
                    alloc.deallocate_frame(base + i * PAGE_SIZE);
                }
            }
        }
    }
}
