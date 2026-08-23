//! AArch64 3-level (L1-L3) page table with 4KB granule.
//!
//! Uses TTBR0_EL1 for identity-mapped kernel + MMIO.  TCR_EL1 configured
//! for a 39-bit VA space (T0SZ=25) matching QEMU virt physical range.

use hal_paging::{PageFlags, PageTableTrait};
use types::*;

pub const PAGE_SIZE: usize = 4096;

const PTE_VALID: u64 = 1 << 0;
const PTE_TABLE: u64 = 1 << 1;
const PTE_PAGE: u64 = 1 << 1;
const PTE_AF: u64 = 1 << 10;
const PTE_SH_IS: u64 = 3 << 8;
/// AP[1] — grant EL0 access. AP[2:1] = 0b?1.
const PTE_AP_EL0: u64 = 1 << 6;
/// AP[2] — read-only at BOTH exception levels. AP[2:1] = 0b1? .
///
/// Set when `PageFlags::WRITE` is absent. Until W^X (phase 10) nothing consulted
/// the WRITE bit on AArch64, so every page came out read/write no matter what the
/// caller asked for; a read-only mapping request was silently upgraded.
const PTE_AP_RO: u64 = 1 << 7;
const PTE_UXN: u64 = 1 << 54;
const PTE_PXN: u64 = 1 << 53;
/// MAIR index 1 = Normal WB-WA-RA — for RAM regions.
const ATTR_NORMAL: u64 = 1 << 2;
/// MAIR index 0 = Device-nGnRnE — for MMIO device registers.
/// Non-cacheable, non-speculative, strictly-ordered; Inner-shareable (PTE_SH_IS) must NOT
/// be set for Device memory (it has no shareable concept — leave SH=0b00).
const ATTR_DEVICE: u64 = 0;

fn phys_to_pte_addr(phys: PhysAddr) -> u64 {
    ((phys as u64) >> 12) << 12
}

/// Invalidate the TLB entry for a single virtual address in every translation
/// regime this page table is live in, all ASIDs, broadcast across the
/// inner-shareable domain.
///
/// Regime coverage: `activate` installs one root table in `TTBR0_EL1`, and when
/// the kernel booted as an EL2 host it installs that SAME root in `TTBR0_EL2`
/// as well (`el2::el2_mmu_init`). Two regimes then cache translations for the
/// same VA independently, and `vaae1is` reaches only the EL1&0 one — a
/// permission change would be honoured for cells at EL0 while the kernel's own
/// EL2 accesses kept using the stale rights. `vae2is` covers the EL2 regime and
/// is issued only when EL2 is active, because the instruction is UNDEFINED at
/// EL1. The non-VHE EL2 regime has no ASIDs, so `vae2is` is already
/// all-contexts; there is no `vaae2is` counterpart to look for.
///
/// Ordering contract (Arm ARM §D8.13): `dsb ishst` first so the PTE store is
/// observable by the other PEs' table walkers before the TLBI is broadcast;
/// `dsb ish` after so the invalidation has completed; `isb` so this PE's
/// already-fetched instructions re-translate. Both TLBIs sit inside that ONE
/// bracket — giving each its own barrier pair would be slower and no more
/// correct. Callers that LOWER a page's permissions must invoke this before
/// returning; without the leading barrier a remote PE can re-walk and re-cache
/// the OLD entry after the TLBI.
///
/// Both `vaae1is` and `vae2is` take the VA shifted right by 12 (page number),
/// not the raw VA.
#[inline]
pub fn flush_tlb_page(virt: VAddr) {
    let page = (virt >> 12) as u64;
    // Read once, outside the asm: `tlbi vae2is` traps as UNDEFINED below EL2,
    // so the EL2 leg has to be selected at runtime rather than always issued.
    let el2 = super::el2::is_el2() as u64;
    // SAFETY: TLB maintenance from EL1/EL2 is privileged but always legal; the
    // sequence modifies no memory, only translation caches. The `vae2is` is
    // branched over unless `EL2_ACTIVE` says the kernel booted at EL2, the only
    // exception level where that encoding is defined. `nomem` is NOT set on the
    // block: the compiler must not sink PTE stores past the first barrier.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vaae1is, {page}",
            "cbz {el2}, 2f",
            "tlbi vae2is, {page}",
            "2:",
            "dsb ish",
            "isb",
            page = in(reg) page,
            el2 = in(reg) el2,
            options(nostack),
        );
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [u64; 512],
}

impl PageTable {
    pub const fn zero() -> Self {
        Self {
            entries: [0u64; 512],
        }
    }

    /// Reclaim empty intermediate tables on an already-unmapped 4 KiB path.
    pub fn prune_empty(&mut self, virt: VAddr, dealloc: &mut dyn FnMut(PhysAddr)) {
        let l1_index = (virt >> 30) & 0x1FF;
        let l1_entry = self.entries[l1_index];
        if l1_entry & PTE_VALID == 0 {
            return;
        }
        let l2_phys = (l1_entry & !0xFFF) as PhysAddr;
        // SAFETY: a valid table descriptor names a live L2 table.
        let l2 = unsafe { &mut *(l2_phys as *mut PageTable) };
        let l2_index = (virt >> 21) & 0x1FF;
        let l2_entry = l2.entries[l2_index];
        if l2_entry & PTE_VALID == 0 {
            if !l2.entries.iter().any(|entry| entry & PTE_VALID != 0) {
                self.entries[l1_index] = 0;
                dealloc(l2_phys);
            }
            return;
        }
        let l3_phys = (l2_entry & !0xFFF) as PhysAddr;
        // SAFETY: a valid table descriptor names a live L3 table.
        let l3 = unsafe { &mut *(l3_phys as *mut PageTable) };
        if l3.entries.iter().any(|entry| entry & PTE_VALID != 0) {
            return;
        }
        l2.entries[l2_index] = 0;
        dealloc(l3_phys);
        if l2.entries.iter().any(|entry| entry & PTE_VALID != 0) {
            return;
        }
        self.entries[l1_index] = 0;
        dealloc(l2_phys);
    }
    /// Return the hardware leaf entry for `virt` without synthesizing flags.
    pub fn leaf_entry(&self, virt: VAddr) -> Option<u64> {
        let l1 = self.entries[(virt >> 30) & 0x1FF];
        if l1 & PTE_VALID == 0 {
            return None;
        }
        let l2 = unsafe { &*((l1 & !0xFFF) as *const PageTable) };
        let l2_entry = l2.entries[(virt >> 21) & 0x1FF];
        if l2_entry & PTE_VALID == 0 {
            return None;
        }
        let l3 = unsafe { &*((l2_entry & !0xFFF) as *const PageTable) };
        let leaf = l3.entries[(virt >> 12) & 0x1FF];
        (leaf & PTE_VALID != 0).then_some(leaf)
    }
}

impl PageTableTrait for PageTable {
    fn init(&mut self) -> ViResult<PhysAddr> {
        self.entries = [0u64; 512];
        Ok(self as *mut _ as PhysAddr)
    }

    fn map(
        &mut self,
        virt: VAddr,
        phys: PhysAddr,
        flags: PageFlags,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()> {
        let l1_idx = (virt >> 30) & 0x1FF;
        let l2_idx = (virt >> 21) & 0x1FF;
        let l3_idx = (virt >> 12) & 0x1FF;

        let l2_table = self.get_or_alloc(l1_idx, alloc_fn)?;
        let l3_table = l2_table.get_or_alloc(l2_idx, alloc_fn)?;

        // Device MMIO: Device-nGnRnE (MAIR index 0, no SH).
        // Normal RAM:  Normal WB-WA-RA (MAIR index 1, Inner-shareable).
        let is_device = flags.bits() & PageFlags::DEVICE != 0;
        let (attr, sh) = if is_device {
            (ATTR_DEVICE, 0u64)
        } else {
            (ATTR_NORMAL, PTE_SH_IS)
        };
        let mut entry = phys_to_pte_addr(phys) | PTE_VALID | PTE_PAGE | PTE_AF | sh | attr;

        // Bit 54 is UXN in the EL1&0 regime but the ONLY XN bit in the EL2
        // (non-VHE) regime — the same table is live in both when the kernel
        // runs as EL2 host (virtualization=on / raspi3b). Kernel pages must
        // therefore leave bit 54 clear or EL2 instruction fetch aborts the
        // moment SCTLR_EL2.M is set. Nothing is lost at EL1: non-USER pages
        // have AP[1]=0, so EL0 cannot fetch from them regardless of UXN.
        if flags.bits() & PageFlags::USER != 0 {
            entry |= PTE_AP_EL0 | PTE_PXN;
        }
        // AP[2]: without it the WRITE flag has no effect on this arch and every
        // page is read/write, which would make the loader's W^X pass a silent
        // no-op on AArch64 while riscv64/x86_64 enforced it.
        if flags.bits() & PageFlags::WRITE == 0 {
            entry |= PTE_AP_RO;
        }
        if flags.bits() & PageFlags::EXECUTE == 0 {
            entry |= PTE_UXN | PTE_PXN;
        }

        // SAFETY: l3_table is a valid page table frame; l3_idx is in [0..512).
        unsafe {
            core::ptr::write_volatile(&mut l3_table.entries[l3_idx], entry);
        }
        // DSB ISH ensures all PTE writes (L1/L2 intermediates from get_or_alloc +
        // L3 leaf above) are visible to the page-table walker before any translation
        // that uses this mapping proceeds (Arm ARM §D8.11).
        // No `nomem` — the compiler must not reorder Rust stores past this barrier.
        unsafe {
            core::arch::asm!("dsb ish", options(nostack));
        }
        Ok(())
    }

    fn unmap(&mut self, virt: VAddr) -> ViResult<()> {
        let l1_idx = (virt >> 30) & 0x1FF;
        let l2_idx = (virt >> 21) & 0x1FF;
        let l3_idx = (virt >> 12) & 0x1FF;

        let l1_entry = self.entries[l1_idx];
        if l1_entry & PTE_VALID == 0 {
            return Err(ViError::NotFound);
        }
        let l2: &mut PageTable = unsafe { &mut *((l1_entry & !0xFFF) as *mut PageTable) };
        let l2_entry = l2.entries[l2_idx];
        if l2_entry & PTE_VALID == 0 {
            return Err(ViError::NotFound);
        }
        let l3: &mut PageTable = unsafe { &mut *((l2_entry & !0xFFF) as *mut PageTable) };
        l3.entries[l3_idx] = 0;
        unsafe {
            core::arch::asm!("tlbi vaae1is, {}", in(reg) (virt >> 12) as u64, options(nomem));
        }
        unsafe {
            core::arch::asm!("dsb sy", options(nomem, nostack));
        }
        Ok(())
    }

    fn translate(&self, virt: VAddr) -> Option<PhysAddr> {
        let l1_idx = (virt >> 30) & 0x1FF;
        let l2_idx = (virt >> 21) & 0x1FF;
        let l3_idx = (virt >> 12) & 0x1FF;
        let l1_entry = self.entries[l1_idx];
        if l1_entry & PTE_VALID == 0 {
            return None;
        }
        let l2: &PageTable = unsafe { &*((l1_entry & !0xFFF) as *const PageTable) };
        let l2_entry = l2.entries[l2_idx];
        if l2_entry & PTE_VALID == 0 {
            return None;
        }
        let l3: &PageTable = unsafe { &*((l2_entry & !0xFFF) as *const PageTable) };
        let l3_entry = l3.entries[l3_idx];
        if l3_entry & PTE_VALID == 0 {
            return None;
        }
        // AArch64 L3 output address is bits [47:12]; bits [63:52] hold upper
        // attributes (UXN=54, PXN=53, Contiguous=52) that must be stripped.
        Some(((l3_entry & 0x0000_FFFF_FFFF_F000) as PhysAddr) | (virt & 0xFFF))
    }

    unsafe fn activate(&self) {
        let ttbr0 = self as *const _ as u64;

        // Dispatch to the EL2 MMU activation path when booted with virtualization=on.
        if super::el2::is_el2() {
            // SAFETY: identity-covering table required; EL2_ACTIVE guarantees EL2.
            unsafe {
                super::el2::el2_mmu_init(ttbr0, hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base);
            }
            return;
        }

        // ── EL1 path (unchanged) ─────────────────────────────────────────────
        let mair: u64 = 0x0000_0000_0000_FF00; // index0=Device-nGnRnE(0x00), index1=Normal-WB-WA(0xFF)
                                               // TG0=4KB (bits 15:14 = 0b00, already zero at reset — no term needed)
        let tcr: u64 = 25      // T0SZ=25 (39-bit VA)
                     | (1 << 8)  // IRGN0=WB-WA
                     | (1 << 10) // ORGN0=WB-WA
                     | (3 << 12) // SH0=Inner-shareable
                     | (1 << 23); // EPD1=disable TTBR1
                                  // SAFETY: MMU activation sequence per AArch64 Architecture Reference Manual.
                                  // Order: write MAIR/TCR, then TTBR0, then barriers, then enable in SCTLR.
        unsafe {
            core::arch::asm!(
                "msr mair_el1, {mair}",
                "msr tcr_el1,  {tcr}",
                "isb",
                "msr ttbr0_el1, {ttbr0}",
                "dsb sy",
                "isb",
                // Invalidate all TLB entries before enabling the MMU so stale entries
                // do not cause faults on real hardware (per ARM ARM DDI 0487 D13.2.118).
                "tlbi vmalle1",   // invalidate all EL1 TLB entries
                "dsb nsh",        // ensure invalidation visible across inner-shareable domain
                "isb",
                "mrs x9, sctlr_el1",
                "orr x9, x9, #(1 << 0)",
                "orr x9, x9, #(1 << 2)",
                "orr x9, x9, #(1 << 12)",
                "msr sctlr_el1, x9",
                "dsb sy",
                "isb",
                mair  = in(reg) mair,
                tcr   = in(reg) tcr,
                ttbr0 = in(reg) ttbr0,
                out("x9") _,
                options(nostack),
            );
        }
    }
}

impl PageTable {
    fn get_or_alloc(
        &mut self,
        idx: usize,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<&mut PageTable> {
        let entry = self.entries[idx];
        if entry & PTE_VALID == 0 {
            let frame = alloc_fn().ok_or(ViError::OutOfMemory)?;
            // SAFETY: frame is a freshly allocated 4KB frame; identity-mapped pre-MMU.
            unsafe { core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE) };
            // SAFETY: self.entries[idx] is a PTE slot in a valid page table frame.
            unsafe {
                core::ptr::write_volatile(
                    &mut self.entries[idx],
                    (frame as u64) | PTE_VALID | PTE_TABLE,
                );
            }
        }
        let next_phys = (self.entries[idx] & !0xFFF) as PhysAddr;
        // SAFETY: identity-mapped; next_phys is a valid page table frame.
        Ok(unsafe { &mut *(next_phys as *mut PageTable) })
    }
}
