//! x86_64 4-level page table (PML4->PDPT->PD->PT), 4KB and 2MB pages.
//!
//! One walker for every root: `PageTable` (the `PageTableTrait` methods,
//! `leaf_entry`, `prune_empty`, `find_table`) reads each lower table through
//! [`phys_to_virt`], the HHDM alias, so it works under Limine's PML4 and the
//! kernel's alike. The shared kernel root (`kernel::memory::paging`) and the
//! private roots (`AddressSpace`) differ only in their `TablePolicy`.
//! `map_bios_area` and `map_hhdm_firmware_range` patch Limine's live higher
//! half (huge leaves included) outside the trait.
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};
use hal_paging::{Leaf, PageFlags, PageTableTrait, TablePolicy};
use types::*;

pub const PAGE_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// HHDM offset — stored once at boot by `set_hhdm_offset`.
// Used by the walkers (`phys_to_virt`) so they can dereference PTE physical
// addresses under Limine's page tables (where phys != virt for RAM).
// ---------------------------------------------------------------------------
static HHDM_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Store the HHDM offset so PML4 walkers can dereference physical addresses.
///
/// Must be called before any walker runs.
/// The value must match the Limine HHDM base (`get_hhdm_offset()`).
pub fn set_hhdm_offset(offset: usize) {
    HHDM_OFFSET.store(offset, Ordering::Relaxed);
}

/// The address through which the HAL page-table walkers read a physical
/// frame: the Limine HHDM alias.
///
/// Precondition: `set_hhdm_offset` has been called with the Limine HHDM base.
#[inline]
pub fn phys_to_virt(phys: usize) -> usize {
    phys + HHDM_OFFSET.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// PTE flag constants (x86_64 4-level paging).
// ---------------------------------------------------------------------------

/// Page present in physical memory.
pub const PTE_PRESENT: u64 = 1 << 0;
/// Read/write (clear = read-only).
pub const PTE_WRITABLE: u64 = 1 << 1;
/// User-accessible (U/S bit; clear = supervisor only).
pub const PTE_USER: u64 = 1 << 2;
/// Write-through cache policy.
pub const PTE_PWT: u64 = 1 << 3;
/// Cache-disable (uncacheable; use for MMIO).
pub const PTE_PCD: u64 = 1 << 4;
/// PAT bit of a 4 KiB leaf (on a huge leaf, bit 7 is `PS` and PAT is bit 12).
pub const PTE_PAT: u64 = 1 << 7;
/// Global: the translation survives a CR3 reload (CR4.PGE).
pub const PTE_GLOBAL: u64 = 1 << 8;
/// No-execute (requires IA32_EFER.NXE set by bootloader).
pub const PTE_NX: u64 = 1 << 63;

/// Physical-address field of a PTE/PDE/PDPTE/PML4E: bits 51:12.
///
/// Extracting a physical address with `entry & !0xFFF` is WRONG on x86-64 —
/// it keeps bit 63 (NX) and bits 62:52 (ignored/available). Any NX-marked
/// entry (every user RW/RO page) then yields a "physical" address with bit 63
/// set; adding the HHDM offset wraps it into a NON-CANONICAL pointer and the
/// next dereference is a #GP (error code 0, CR2 stale) that masquerades as a
/// #PF at va=0.
pub const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ---------------------------------------------------------------------------
// CR3 / TLB helpers.
// ---------------------------------------------------------------------------

/// Read the current CR3 (physical address of active PML4).
///
/// # Safety
/// Reading CR3 is always valid in kernel (CPL 0) mode.
#[inline]
pub unsafe fn read_cr3() -> u64 {
    let cr3: u64;
    // SAFETY: reading CR3 is always valid in kernel mode.
    unsafe {
        asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
    }
    cr3
}

/// Write a new PML4 physical address into CR3, flushing the full TLB.
///
/// # Safety
/// `phys` must be the physical address of a valid, fully-populated PML4 that
/// maps the currently executing code path and kernel stack.  An invalid PML4
/// causes an immediate triple-fault.
#[inline]
pub unsafe fn write_cr3(phys: u64) {
    // SAFETY: caller guarantees the new PML4 keeps the kernel mapped.
    unsafe {
        asm!("mov cr3, {}", in(reg) phys, options(nomem, nostack));
    }
}

/// Flush a single page from the TLB.
///
/// # Safety
/// `va` must be a virtual address; the instruction itself has no memory
/// side-effects other than invalidating the TLB entry for that address.
#[inline]
pub unsafe fn invlpg(va: usize) {
    // SAFETY: invlpg only invalidates one TLB entry; no memory is modified.
    unsafe {
        asm!("invlpg [{v}]", v = in(reg) va, options(nomem));
    }
}

/// Invalidate the TLB entry for a single virtual address.
///
/// Safe counterpart of [`invlpg`], named to match the riscv64 / aarch64 HAL so
/// architecture-generic paging code can call one name. `invlpg` cannot corrupt
/// memory or violate any Rust invariant — it only drops a translation cache
/// entry — so the `unsafe` on the raw intrinsic is a legacy of its asm body,
/// not a real obligation on the caller.
///
/// Non-shootdown: on SMP the other cores keep their entry until an IPI reaches
/// them. Callers LOWERING permissions on a page reachable from another core
/// must arrange that shootdown themselves.
#[inline]
pub fn flush_tlb_page(va: usize) {
    // SAFETY: invlpg invalidates one TLB entry and writes no memory; it is
    // legal from Ring 0 for any address, mapped or not.
    unsafe {
        invlpg(va);
    }
}

/// The leaf translating `va` under the root table whose frame is `root`.
///
/// # Safety
/// `root` is the physical address of a live root table, and every table it
/// names is readable through [`phys_to_virt`].
pub unsafe fn leaf(root: PhysAddr, va: VAddr) -> Option<Leaf> {
    // SAFETY: the caller's contract.
    unsafe { &*(phys_to_virt(root) as *const PageTable) }.leaf(va)
}

/// Physical root of the translation in use on this CPU: `CR3` without its
/// low twelve (tag or cache-control) bits.
pub fn current_root() -> Option<PhysAddr> {
    let cr3: usize;
    // SAFETY: reading CR3 has no side effect.
    unsafe {
        asm!("mov {cr3}, cr3", cr3 = out(reg) cr3, options(nostack, nomem));
    }
    Some(cr3 & !0xFFF)
}

// Internal aliases used by PageTableTrait impl below.
const PTE_P: u64 = PTE_PRESENT;
const PTE_RW: u64 = PTE_WRITABLE;
const PTE_US: u64 = PTE_USER;
const PTE_PS: u64 = 1 << 7; // page-size (huge-page) bit on a PDPT/PD entry

/// The bits of a leaf that [`PageTableTrait::protect`] keeps: the frame, the
/// cache attribute (`PWT`, `PCD`, `PAT`) and `G`.
const PTE_KEEP_ON_PROTECT: u64 = PTE_ADDR_MASK | PTE_PWT | PTE_PCD | PTE_PAT | PTE_GLOBAL;

/// The access-rights bits a leaf takes from `flags`: always `P`, `RW` for
/// `WRITE`, `US` for `USER`, `NX` without `EXECUTE`.
fn leaf_rights(flags: PageFlags) -> u64 {
    let bits = flags.bits();
    let mut e = PTE_P;
    if bits & PageFlags::WRITE != 0 {
        e |= PTE_RW;
    }
    if bits & PageFlags::USER != 0 {
        e |= PTE_US;
    }
    if bits & PageFlags::EXECUTE == 0 {
        e |= PTE_NX;
    }
    e
}

/// [`leaf_rights`] plus the attributes only a new leaf takes from `flags`:
/// `G` for `GLOBAL`, `PCD` for `UNCACHED`.
fn leaf_bits(flags: PageFlags) -> u64 {
    let mut e = leaf_rights(flags);
    if flags.bits() & PageFlags::GLOBAL != 0 {
        e |= PTE_GLOBAL;
    }
    if flags.bits() & PageFlags::UNCACHED != 0 {
        e |= PTE_PCD;
    }
    e
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
    pub const fn empty() -> Self {
        Self::zero()
    }
    /// Copy PML4 slots 256..512 of `src`, one volatile load and store per
    /// entry: `src` may be a live root the hardware walker reads.
    pub fn copy_kernel_higher_half(&mut self, src: &PageTable) {
        for index in 256..512 {
            self.set_entry(index, src.entry(index));
        }
    }

    fn entry(&self, index: usize) -> u64 {
        // SAFETY: `index` is a slot of this table; the hardware walker and
        // other CPUs read it concurrently, hence the volatile access.
        unsafe { core::ptr::read_volatile(&self.entries[index]) }
    }

    fn set_entry(&mut self, index: usize, value: u64) {
        // SAFETY: as in `entry`; one aligned 64-bit store.
        unsafe { core::ptr::write_volatile(&mut self.entries[index], value) }
    }

    /// The table a present, non-huge entry names, for a read-only walk.
    fn table_of(entry: u64) -> &'static PageTable {
        // SAFETY: the caller checked that `entry` is a present table pointer;
        // its frame is reachable through `phys_to_virt`. Reads are volatile.
        unsafe { &*(phys_to_virt((entry & PTE_ADDR_MASK) as usize) as *const PageTable) }
    }

    /// [`Self::table_of`] for an edit, reached through `&mut` of its root.
    fn table_of_mut(entry: u64) -> &'static mut PageTable {
        // SAFETY: as in `table_of`; the caller holds the root mutably (the
        // kernel root under `KERNEL_ROOT`'s lock), so it is the only editor.
        unsafe { &mut *(phys_to_virt((entry & PTE_ADDR_MASK) as usize) as *mut PageTable) }
    }

    /// The page table holding the 4 KiB leaf slot of `virt`, or `None` when a
    /// level above it is absent or a huge leaf.
    fn leaf_table(&mut self, virt: VAddr) -> Option<&mut PageTable> {
        let mut entry = self.entry((virt >> 39) & 0x1FF);
        if entry & PTE_P == 0 {
            return None;
        }
        for shift in [30, 21] {
            entry = Self::table_of(entry).entry((virt >> shift) & 0x1FF);
            if entry & PTE_P == 0 || entry & PTE_PS != 0 {
                return None;
            }
        }
        Some(Self::table_of_mut(entry))
    }

    /// The entry that maps `virt` and the log2 size it maps: a 4 KiB leaf, or
    /// any entry with bit 7 set above the PT level. `None` when a level is
    /// absent.
    fn walk(&self, virt: VAddr) -> Option<(u64, u32)> {
        let mut table: &PageTable = self;
        for level in 0..4u32 {
            let shift = 39 - 9 * level;
            let entry = table.entry((virt >> shift) & 0x1FF);
            if entry & PTE_P == 0 {
                return None;
            }
            if level == 3 || entry & PTE_PS != 0 {
                return Some((entry, shift));
            }
            table = Self::table_of(entry);
        }
        None
    }

    /// Return the hardware leaf entry for `virt` without synthesizing flags.
    pub fn leaf_entry(&self, virt: VAddr) -> Option<u64> {
        self.walk(virt).map(|(entry, _)| entry)
    }

    /// The first intermediate table under this root whose frame is `frame`,
    /// with the first virtual address it covers. Slots are visited in index
    /// order, PML4 then PDPT then PD; absent entries and huge leaves are
    /// skipped. A diagnostic for leaked page-table frames.
    pub fn find_table(&self, frame: PhysAddr) -> Option<(TableLevel, VAddr)> {
        let is_table = |entry: u64| entry & PTE_P != 0 && entry & PTE_PS == 0;
        for i3 in 0..512usize {
            let level3 = self.entry(i3);
            if !is_table(level3) {
                continue;
            }
            if (level3 & PTE_ADDR_MASK) as PhysAddr == frame {
                return Some((TableLevel::Pdpt, i3 << 39));
            }
            let pdpt = Self::table_of(level3);
            for i2 in 0..512usize {
                let level2 = pdpt.entry(i2);
                if !is_table(level2) {
                    continue;
                }
                let window = (i3 << 39) | (i2 << 30);
                if (level2 & PTE_ADDR_MASK) as PhysAddr == frame {
                    return Some((TableLevel::Pd, window));
                }
                let pd = Self::table_of(level2);
                for i1 in 0..512usize {
                    let level1 = pd.entry(i1);
                    if is_table(level1) && (level1 & PTE_ADDR_MASK) as PhysAddr == frame {
                        return Some((TableLevel::Pt, window | (i1 << 21)));
                    }
                }
            }
        }
        None
    }
}

/// The level of an intermediate table under a PML4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableLevel {
    Pdpt,
    Pd,
    Pt,
}

impl PageTable {
    /// Reclaim empty intermediate tables on an already-unmapped 4 KiB path.
    pub fn prune_empty(&mut self, virt: VAddr, dealloc: &mut dyn FnMut(PhysAddr)) {
        let i3 = (virt >> 39) & 0x1FF;
        let root_entry = self.entries[i3];
        if root_entry & PTE_P == 0 || root_entry & PTE_PS != 0 {
            return;
        }
        let pdpt_phys = (root_entry & PTE_ADDR_MASK) as PhysAddr;
        // SAFETY: a present non-huge root entry names a live PDPT.
        let pdpt = unsafe { &mut *(phys_to_virt(pdpt_phys) as *mut PageTable) };
        let i2 = (virt >> 30) & 0x1FF;
        let pdpt_entry = pdpt.entries[i2];
        if pdpt_entry & PTE_P == 0 {
            if !pdpt.entries.iter().any(|entry| entry & PTE_P != 0) {
                self.entries[i3] = 0;
                dealloc(pdpt_phys);
            }
            return;
        }
        if pdpt_entry & PTE_PS != 0 {
            return;
        }
        let pd_phys = (pdpt_entry & PTE_ADDR_MASK) as PhysAddr;
        // SAFETY: a present non-huge PDPT entry names a live page directory.
        let pd = unsafe { &mut *(phys_to_virt(pd_phys) as *mut PageTable) };
        let i1 = (virt >> 21) & 0x1FF;
        let pd_entry = pd.entries[i1];
        if pd_entry & PTE_P == 0 {
            if !pd.entries.iter().any(|entry| entry & PTE_P != 0) {
                pdpt.entries[i2] = 0;
                dealloc(pd_phys);
                if !pdpt.entries.iter().any(|entry| entry & PTE_P != 0) {
                    self.entries[i3] = 0;
                    dealloc(pdpt_phys);
                }
            }
            return;
        }
        if pd_entry & PTE_PS != 0 {
            return;
        }
        let pt_phys = (pd_entry & PTE_ADDR_MASK) as PhysAddr;
        // SAFETY: a present non-huge directory entry names a live page table.
        let pt = unsafe { &mut *(phys_to_virt(pt_phys) as *mut PageTable) };
        if pt.entries.iter().any(|entry| entry & PTE_P != 0) {
            return;
        }
        pd.entries[i1] = 0;
        dealloc(pt_phys);
        if pd.entries.iter().any(|entry| entry & PTE_P != 0) {
            return;
        }
        pdpt.entries[i2] = 0;
        dealloc(pd_phys);
        if pdpt.entries.iter().any(|entry| entry & PTE_P != 0) {
            return;
        }
        self.entries[i3] = 0;
        dealloc(pdpt_phys);
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
        self.map_with(virt, phys, flags, TablePolicy::FollowLeaf, alloc_fn)
    }
    fn map_with(
        &mut self,
        virt: VAddr,
        phys: PhysAddr,
        flags: PageFlags,
        tables: TablePolicy,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()> {
        let i3 = (virt >> 39) & 0x1FF;
        let i2 = (virt >> 30) & 0x1FF;
        let i1 = (virt >> 21) & 0x1FF;
        let i0 = (virt >> 12) & 0x1FF;
        let user = flags.bits() & PageFlags::USER != 0;
        let pdpt = self.get_or_alloc(i3, user, tables, alloc_fn)?;
        let pd = pdpt.get_or_alloc(i2, user, tables, alloc_fn)?;
        let pt = pd.get_or_alloc(i1, user, tables, alloc_fn)?;
        pt.set_entry(i0, phys as u64 | leaf_bits(flags));
        Ok(())
    }
    fn protect(&mut self, virt: VAddr, flags: PageFlags) -> ViResult<()> {
        let pt = self.leaf_table(virt).ok_or(ViError::NotFound)?;
        let i0 = (virt >> 12) & 0x1FF;
        let old = pt.entry(i0);
        if old & PTE_P == 0 {
            return Err(ViError::NotFound);
        }
        // `P` is always set: a protect without `VALID` does not unmap here.
        pt.set_entry(i0, (old & PTE_KEEP_ON_PROTECT) | leaf_rights(flags));
        Ok(())
    }
    /// Clears the 4 KiB leaf slot of `virt`. No TLB invalidation: the caller
    /// owns it (the shared root's global shootdown, a private root's tag
    /// invalidation). A local `invlpg` here would only reach the current
    /// PCID, the kernel root's, never a private root's non-global leaf.
    fn unmap(&mut self, virt: VAddr) -> ViResult<()> {
        let pt = self.leaf_table(virt).ok_or(ViError::NotFound)?;
        pt.set_entry((virt >> 12) & 0x1FF, 0);
        Ok(())
    }
    fn translate(&self, virt: VAddr) -> Option<PhysAddr> {
        let (entry, shift) = self.walk(virt)?;
        let offset = (1u64 << shift) - 1;
        Some(((entry & PTE_ADDR_MASK & !offset) | (virt as u64 & offset)) as PhysAddr)
    }
    fn leaf(&self, virt: VAddr) -> Option<Leaf> {
        let entry = self.leaf_entry(virt)?;
        Some(Leaf {
            pa: (entry & PTE_ADDR_MASK) as PhysAddr,
            valid: true,
            read: true,
            write: entry & PTE_RW != 0,
            user: entry & PTE_US != 0,
            exec: entry & PTE_NX == 0,
        })
    }
    unsafe fn activate(&self) {
        // Physical address of this PageTable struct. Under Limine the struct
        // sits in HHDM-mapped RAM; we need the physical address for CR3.
        let virt = self as *const _ as usize;
        let offset = HHDM_OFFSET.load(Ordering::Relaxed);
        let phys = if virt >= offset && offset != 0 {
            virt - offset
        } else {
            virt
        } as u64;
        // SAFETY: phys is the physical address of this valid PML4; caller ensures
        // the kernel and the current stack are mapped so execution continues.
        unsafe {
            write_cr3(phys);
        }
    }
}

/// Patch Limine's HHDM to cover the BIOS area and ACPI-reserved RAM regions.
///
/// Limine 8.x maps only usable e820 RAM in the HHDM.  Two categories of
/// physical addresses within the first 256 MiB are intentionally absent:
///
/// 1. **BIOS area** [0x9F000–0x100000) — EBDA, VGA option ROM, BIOS ROM.
///    The ACPI RSDP is typically here (~0xF52E0 on q35).
///    Fixed with fine-grained 4 KiB PTEs in PD[0]'s existing PT.
///
/// 2. **ACPI-reserved RAM** (e.g. [0xFE00000–0x10000000) on q35 256 MB) —
///    marked type 3/4 in the e820 map; Limine leaves PD entries absent.
///    Fixed with 2 MiB huge-page PTEs for each absent PD slot.
///
/// Both fixes use only HHDM-accessible frames (PML4, PDPT, PD, PT are all
/// allocated by Limine from usable RAM, so they are reachable via HHDM).
///
/// Must be called after `set_hhdm_offset`.  Absent/huge-page levels above
/// the PD are treated as "nothing to do" (the HHDM simply isn't present).
///
/// # Safety
/// Must run in kernel mode (CPL 0) after `set_hhdm_offset` has been stored.
pub unsafe fn map_bios_area() {
    let hhdm = HHDM_OFFSET.load(Ordering::Relaxed);
    if hhdm == 0 {
        return;
    }

    // Active PML4 physical address (flags in low 12 bits stripped below).
    // SAFETY: reading CR3 is always valid in kernel mode.
    let cr3 = unsafe { read_cr3() } as usize & !0xFFF;

    // Macro: physical frame address → dereferenceable virtual pointer via HHDM.
    macro_rules! p2v {
        ($p:expr) => {
            ($p + hhdm) as *mut u64
        };
    }

    // Walk PML4[256] → PDPT[0] → PD (all covering HHDM virtual base).
    // Index 256 corresponds to virtual 0xffff800000000000.
    let pml4 = p2v!(cr3);
    // SAFETY: cr3 is the live PML4 in usable RAM → accessible via HHDM.
    let e3 = unsafe { core::ptr::read_volatile(pml4.add(256)) };
    if e3 & PTE_PRESENT == 0 {
        return;
    } // HHDM entirely absent
    if e3 & (1 << 7) != 0 {
        return;
    } // 1 GiB leaf — everything mapped

    let pdpt = p2v!((e3 & PTE_ADDR_MASK) as usize);
    // SAFETY: PDPT is in usable RAM → accessible via HHDM.
    let e2 = unsafe { core::ptr::read_volatile(pdpt.add(0)) };
    if e2 & PTE_PRESENT == 0 {
        return;
    } // First 1 GiB absent
    if e2 & (1 << 7) != 0 {
        return;
    } // 1 GiB leaf — everything mapped

    let pd = p2v!((e2 & PTE_ADDR_MASK) as usize);

    // ── Part 1: BIOS area — fine-grained 4 KiB fix in PD[0]'s PT ────────────
    // SAFETY: PD is in usable RAM → accessible via HHDM.
    let e1 = unsafe { core::ptr::read_volatile(pd.add(0)) };
    if e1 & PTE_PRESENT != 0 && e1 & (1 << 7) == 0 {
        // PD[0] is a PT pointer; fill missing entries for [0x9F000, 0x100000).
        // PT covers [0, 2 MiB); entries 0x9F–0xFF are the BIOS/ROM area.
        let pt = p2v!((e1 & PTE_ADDR_MASK) as usize);
        for page in 0x9F_usize..=0xFF {
            // SAFETY: PT is in usable RAM → accessible via HHDM.
            let pte_ptr = unsafe { pt.add(page) };
            if unsafe { core::ptr::read_volatile(pte_ptr) } & PTE_PRESENT == 0 {
                let pte = (page as u64 * 0x1000) | PTE_PRESENT | PTE_WRITABLE | PTE_NX;
                // SAFETY: writing to a valid PT slot in usable-RAM-backed frame.
                unsafe { core::ptr::write_volatile(pte_ptr, pte) };
            }
        }
    }

    // ── Part 2: absent 2 MiB regions in physical [2 MiB, 256 MiB) ───────────
    // PD[0] was handled above (fine-grained).  PD[1..=127] covers [2MiB, 256MiB).
    // Any absent PD entry is an ACPI-reserved or bootloader-reserved hole;
    // map it as a 2 MiB huge page (PS=1 in the PD entry) so ACPI tables
    // at e.g. [0xFE00000, 0x10000000) (PD[127]) become reachable via HHDM.
    for i in 1_usize..128 {
        // SAFETY: pd is in usable RAM; i < 512.
        let pde = unsafe { pd.add(i) };
        if unsafe { core::ptr::read_volatile(pde) } & PTE_PRESENT != 0 {
            continue; // already mapped by Limine
        }
        // 2 MiB huge page: PS bit (7), physical base = i * 2 MiB.
        let huge = (i as u64 * 0x200000) | PTE_PRESENT | PTE_WRITABLE | PTE_NX | (1 << 7);
        // SAFETY: writing to a valid PD slot.
        unsafe { core::ptr::write_volatile(pde, huge) };
    }

    // Flush the entire TLB by reloading CR3 with the same physical value.
    // This is cheaper than 96 + 127 individual invlpg calls and is correct
    // because we are not changing the kernel's own mappings, only patching HHDM.
    // SAFETY: same PML4, no mapping removed — kernel code path stays valid.
    unsafe { write_cr3(cr3 as u64) };
}

/// Extend the live Limine HHDM over a firmware-owned physical range.
///
/// Missing pages are installed with 4 KiB leaves so mapping a small firmware
/// record never aliases unrelated ROM or MMIO in the surrounding 2 MiB span.
/// Existing 1 GiB, 2 MiB, or 4 KiB mappings are preserved. The allocator must
/// return HHDM-accessible, zeroed-or-discardable 4 KiB frames for new page-table
/// levels. This is intended for ACPI reclaim/NVS ranges before table parsing.
///
/// # Safety
/// The caller must run at CPL0 after [`set_hhdm_offset`], and `allocator` must
/// exclusively own every frame it returns.
pub unsafe fn map_hhdm_firmware_range(
    phys_start: usize,
    length: usize,
    allocator: &mut dyn FnMut() -> Option<usize>,
) -> bool {
    let hhdm = HHDM_OFFSET.load(Ordering::Relaxed);
    if hhdm == 0 || hhdm & (0x20_0000 - 1) != 0 || length == 0 {
        return false;
    }
    let Some(phys_end) = phys_start.checked_add(length) else {
        return false;
    };
    if hhdm.checked_add(phys_end - 1).is_none() {
        return false;
    }
    let start = phys_start & !(PAGE_SIZE - 1);
    let Some(end) = phys_end.checked_add(PAGE_SIZE - 1) else {
        return false;
    };
    let end = end & !(PAGE_SIZE - 1);
    // SAFETY: reading the active CR3 is valid at CPL0.
    let cr3 = unsafe { read_cr3() } as usize & !0xFFF;
    let Some(pml4_virt) = cr3.checked_add(hhdm) else {
        return false;
    };
    let pml4 = pml4_virt as *mut u64;

    unsafe fn table_for(
        entry: *mut u64,
        hhdm: usize,
        allocator: &mut dyn FnMut() -> Option<usize>,
    ) -> Option<*mut u64> {
        // SAFETY: `entry` belongs to a live page-table frame.
        let current = unsafe { core::ptr::read_volatile(entry) };
        if current & PTE_PRESENT != 0 {
            if current & (1 << 7) != 0 {
                return None;
            }
            let table = ((current & PTE_ADDR_MASK) as usize).checked_add(hhdm)?;
            return Some(table as *mut u64);
        }
        let frame = allocator()?;
        if frame & (PAGE_SIZE - 1) != 0 {
            return None;
        }
        let table = frame.checked_add(hhdm)? as *mut u64;
        // SAFETY: allocator returned an exclusively owned 4 KiB frame.
        unsafe { core::ptr::write_bytes(table as *mut u8, 0, PAGE_SIZE) };
        // SAFETY: installing a present writable page-table pointer is atomic.
        unsafe {
            core::ptr::write_volatile(entry, frame as u64 | PTE_PRESENT | PTE_WRITABLE);
        }
        Some(table)
    }

    let mut page = start;
    while page < end {
        let Some(va) = hhdm.checked_add(page) else {
            return false;
        };
        let i3 = (va >> 39) & 0x1FF;
        let i2 = (va >> 30) & 0x1FF;
        let i1 = (va >> 21) & 0x1FF;
        let i0 = (va >> 12) & 0x1FF;

        // SAFETY: indices are bounded to one page-table frame.
        let Some(pdpt) = (unsafe { table_for(pml4.add(i3), hhdm, allocator) }) else {
            return false;
        };
        // A 1 GiB leaf already covers this range.
        let pdpt_entry = unsafe { core::ptr::read_volatile(pdpt.add(i2)) };
        if pdpt_entry & PTE_PRESENT != 0 && pdpt_entry & (1 << 7) != 0 {
            page += PAGE_SIZE;
            continue;
        }
        let Some(pd) = (unsafe { table_for(pdpt.add(i2), hhdm, allocator) }) else {
            return false;
        };
        let pde = unsafe { pd.add(i1) };
        let current_pde = unsafe { core::ptr::read_volatile(pde) };
        if current_pde & PTE_PRESENT != 0 && current_pde & (1 << 7) != 0 {
            page += PAGE_SIZE;
            continue;
        }
        let Some(pt) = (unsafe { table_for(pde, hhdm, allocator) }) else {
            return false;
        };
        let pte = unsafe { pt.add(i0) };
        if unsafe { core::ptr::read_volatile(pte) } & PTE_PRESENT == 0 {
            unsafe {
                core::ptr::write_volatile(pte, page as u64 | PTE_PRESENT | PTE_WRITABLE | PTE_NX)
            };
        }
        page += PAGE_SIZE;
    }

    // SAFETY: only additive HHDM mappings changed; reloading CR3 flushes stale misses.
    unsafe { write_cr3(cr3 as u64) };
    true
}

impl PageTable {
    fn get_or_alloc(
        &mut self,
        idx: usize,
        user: bool,
        tables: TablePolicy,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<&mut PageTable> {
        // U/S is ANDed across all four levels. A lower-level index says
        // nothing about privilege (QuickJS's low-half VA has PD index 384).
        // Under `FollowLeaf`, supervisor leaves remain protected when an
        // ancestor is upgraded; under `UserReachable` every new table is
        // user-reachable and only the leaf decides.
        let us_bit = match tables {
            TablePolicy::FollowLeaf if !user => 0,
            _ => PTE_US,
        };
        let entry = self.entry(idx);
        if entry & PTE_P == 0 {
            let f = alloc_fn().ok_or(ViError::OutOfMemory)?;
            // SAFETY: f is a freshly allocated 4KB physical frame. Accessed via
            // HHDM virtual address so it is dereferenceable under Limine's PML4.
            unsafe { core::ptr::write_bytes(phys_to_virt(f) as *mut u8, 0, PAGE_SIZE) };
            // The zeroing is a plain store: keep it ahead of the volatile
            // store that publishes the table to the walker and other CPUs.
            core::sync::atomic::compiler_fence(Ordering::Release);
            self.set_entry(idx, f as u64 | PTE_P | PTE_RW | us_bit);
        } else if tables == TablePolicy::FollowLeaf && us_bit != 0 {
            self.set_entry(idx, entry | us_bit);
        }
        Ok(Self::table_of_mut(self.entry(idx)))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{boxed::Box, vec::Vec};

    #[test]
    fn user_execution_crosses_high_table_indices_without_exposing_supervisor_leaves() {
        let mut root = Box::new(PageTable::zero());
        let mut frames: Vec<Box<PageTable>> = Vec::new();
        let mut allocate = || {
            let mut table = Box::new(PageTable::zero());
            let address = table.as_mut() as *mut PageTable as usize;
            frames.push(table);
            Some(address)
        };
        let entry = 0x1b0000000;
        let supervisor = entry + PAGE_SIZE;
        root.map(
            supervisor,
            0x200000,
            PageFlags::from_bits(PageFlags::READ | PageFlags::WRITE),
            &mut allocate,
        )
        .unwrap();
        root.map(
            entry,
            0x100000,
            PageFlags::from_bits(PageFlags::READ | PageFlags::EXECUTE | PageFlags::USER),
            &mut allocate,
        )
        .unwrap();
        // Exercise high PDPT and PT indices as well as the failing PD index.
        let high = (255usize << 39) | (511usize << 30) | (511usize << 21) | (511usize << 12);
        root.map(
            high,
            0x300000,
            PageFlags::from_bits(PageFlags::READ | PageFlags::USER),
            &mut allocate,
        )
        .unwrap();

        fn chain(root: &PageTable, va: usize) -> [u64; 4] {
            let mut entries = [0; 4];
            let mut table = root;
            for (level, shift) in [39, 30, 21, 12].into_iter().enumerate() {
                let entry = table.entries[(va >> shift) & 511];
                entries[level] = entry;
                assert_ne!(entry & PTE_PRESENT, 0);
                if level < 3 {
                    // SAFETY: the test owns all aligned intermediate frames.
                    table = unsafe {
                        &*(phys_to_virt((entry & PTE_ADDR_MASK) as usize) as *const PageTable)
                    };
                }
            }
            entries
        }
        let code = chain(&root, entry);
        assert!(code.iter().all(|pte| pte & PTE_USER != 0));
        assert_eq!(code[3] & (PTE_WRITABLE | PTE_NX), 0);
        let data = chain(&root, supervisor);
        assert_eq!(data[3] & PTE_USER, 0);
        assert_ne!(data[3] & PTE_NX, 0);
        assert!(chain(&root, high).iter().all(|pte| pte & PTE_USER != 0));
    }
}
