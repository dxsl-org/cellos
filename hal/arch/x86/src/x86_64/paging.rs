//! x86_64 4-level page table (PML4->PDPT->PD->PT), 4KB and 2MB pages.
//!
//! Two-mode walker:
//!
//! **Pre-paging (Limine PML4 active):** `PageTable::map` / `get_or_alloc` treat
//! physical PTE addresses as virtual via `phys_to_virt` (HHDM offset applied).
//!
//! **Kernel-owned PML4:** `walk_create` / `walk_read` are the HHDM-aware helpers
//! used by `kernel::memory::paging::init_kernel_paging` and `map_page`/`unmap_page`.
//! They always apply the HHDM offset from `kernel_phys_to_virt` before
//! dereferencing a PTE physical address.
use hal_paging::{PageFlags, PageTableTrait};
use types::*;
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const PAGE_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// HHDM offset — stored once at boot by `set_hhdm_offset`.
// Used by walk_create / walk_read so they can dereference PTE physical
// addresses under Limine's page tables (where phys != virt for RAM).
// ---------------------------------------------------------------------------
static HHDM_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Store the HHDM offset so PML4 walkers can dereference physical addresses.
///
/// Must be called before `walk_create` or `walk_read`.
/// The value must match the Limine HHDM base (`get_hhdm_offset()`).
pub fn set_hhdm_offset(offset: usize) {
    HHDM_OFFSET.store(offset, Ordering::Relaxed);
}

/// Convert a physical frame address to a dereferenceable virtual pointer.
///
/// Precondition: `set_hhdm_offset` has been called with the Limine HHDM base.
#[inline]
fn phys_to_virt_ptr(phys: usize) -> usize {
    phys + HHDM_OFFSET.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// PTE flag constants (x86_64 4-level paging).
// ---------------------------------------------------------------------------

/// Page present in physical memory.
pub const PTE_PRESENT:  u64 = 1 << 0;
/// Read/write (clear = read-only).
pub const PTE_WRITABLE: u64 = 1 << 1;
/// User-accessible (U/S bit; clear = supervisor only).
pub const PTE_USER:     u64 = 1 << 2;
/// Write-through cache policy.
pub const PTE_PWT:      u64 = 1 << 3;
/// Cache-disable (uncacheable; use for MMIO).
pub const PTE_PCD:      u64 = 1 << 4;
/// No-execute (requires IA32_EFER.NXE set by bootloader).
pub const PTE_NX:       u64 = 1 << 63;

// ---------------------------------------------------------------------------
// Composed flag sets for common mapping kinds.
// ---------------------------------------------------------------------------

/// Kernel read/write data mapping (supervisor, no-execute).
#[inline] pub fn pte_flags_kernel_rw()   -> u64 { PTE_PRESENT | PTE_WRITABLE | PTE_NX }
/// Kernel code mapping (supervisor, read-only, executable).
#[inline] pub fn pte_flags_kernel_code() -> u64 { PTE_PRESENT }
/// User read/write data (user-accessible, no-execute).
#[inline] pub fn pte_flags_user_rw()     -> u64 { PTE_PRESENT | PTE_WRITABLE | PTE_USER | PTE_NX }
/// User read-only data (user-accessible, no-execute).
#[inline] pub fn pte_flags_user_ro()     -> u64 { PTE_PRESENT | PTE_USER | PTE_NX }
/// User executable code (user-accessible, read-only, executable).
#[inline] pub fn pte_flags_user_exec()   -> u64 { PTE_PRESENT | PTE_USER }
/// MMIO mapping (supervisor, read/write, cache-disable, no NX).
#[inline] pub fn pte_flags_mmio()        -> u64 { PTE_PRESENT | PTE_WRITABLE | PTE_PCD }

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
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack)); }
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
    unsafe { asm!("mov cr3, {}", in(reg) phys, options(nomem, nostack)); }
}

/// Flush a single page from the TLB.
///
/// # Safety
/// `va` must be a virtual address; the instruction itself has no memory
/// side-effects other than invalidating the TLB entry for that address.
#[inline]
pub unsafe fn invlpg(va: usize) {
    // SAFETY: invlpg only invalidates one TLB entry; no memory is modified.
    unsafe { asm!("invlpg [{v}]", v = in(reg) va, options(nomem)); }
}

// ---------------------------------------------------------------------------
// HHDM-aware PML4 walkers.
// ---------------------------------------------------------------------------

/// Trait for the frame allocator callback used by `walk_create`.
pub trait FrameAllocatorFn {
    fn allocate(&mut self) -> Option<usize>;
}

/// Walk or create the 4-level page table path from `pml4` to the leaf PTE
/// for `va`, allocating intermediate tables via `allocator` as needed.
///
/// Returns a pointer to the leaf L1 PTE (in the PT).
///
/// # Safety
/// - `pml4` must point to a valid, at least partially-initialised PML4 table
///   accessible via the current virtual address space.
/// - `allocator` must return 4KB-aligned, zeroed physical frames.
/// - `set_hhdm_offset` must have been called before this function.
pub unsafe fn walk_create(
    pml4: *mut u64,
    va: usize,
    allocator: &mut dyn FnMut() -> Option<usize>,
) -> Option<*mut u64> {
    // Index at each level: PML4[47:39], PDPT[38:30], PD[29:21], PT[20:12].
    let i3 = (va >> 39) & 0x1FF;
    let i2 = (va >> 30) & 0x1FF;
    let i1 = (va >> 21) & 0x1FF;
    let i0 = (va >> 12) & 0x1FF;

    // Helper: get-or-create the next level table pointer.
    // `entry_ptr` is the virtual address of the PTE in the current table.
    // Returns virtual address of the next-level table.
    unsafe fn ensure_next(entry_ptr: *mut u64, alloc: &mut dyn FnMut() -> Option<usize>) -> Option<*mut u64> {
        // SAFETY: entry_ptr is within a valid page table frame.
        let entry = unsafe { core::ptr::read_volatile(entry_ptr) };
        if entry & PTE_PRESENT != 0 {
            // Already present: strip flags, apply HHDM offset.
            let next_phys = (entry & !0xFFF) as usize;
            Some(phys_to_virt_ptr(next_phys) as *mut u64)
        } else {
            // Allocate a new zeroed frame.
            let frame_phys = alloc()?;
            // SAFETY: frame_phys is a freshly allocated 4KB frame; zero it via HHDM virt ptr.
            unsafe {
                core::ptr::write_bytes(phys_to_virt_ptr(frame_phys) as *mut u8, 0, PAGE_SIZE);
            }
            // Intermediate tables: Present + Writable + User so all mappings can
            // set tighter per-leaf permissions. User bit is only load-bearing at
            // the leaf PTE; the CPU checks the ANDed chain, but setting it on
            // intermediates simplifies the caller (leaf flags override).
            let new_entry = frame_phys as u64 | PTE_PRESENT | PTE_WRITABLE | PTE_USER;
            // SAFETY: entry_ptr is the address of a valid PTE slot.
            unsafe { core::ptr::write_volatile(entry_ptr, new_entry); }
            Some(phys_to_virt_ptr(frame_phys) as *mut u64)
        }
    }

    // Walk PML4 → PDPT → PD → PT, creating tables as needed.
    // SAFETY: pml4 is a valid PML4 pointer (checked by caller).
    let pdpt_base = unsafe { ensure_next(pml4.add(i3), allocator)? };
    // SAFETY: pdpt_base points to the start of a valid PDPT frame.
    let pd_base   = unsafe { ensure_next(pdpt_base.add(i2), allocator)? };
    // SAFETY: pd_base points to the start of a valid PD frame.
    let pt_base   = unsafe { ensure_next(pd_base.add(i1), allocator)? };

    // Return pointer to the leaf PTE within the PT.
    // SAFETY: pt_base is valid and i0 is in [0, 511].
    Some(unsafe { pt_base.add(i0) })
}

/// Walk the 4-level page table path from `pml4` to the leaf PTE for `va`.
///
/// Returns `Some(pte_value)` if the page is present, `None` if any level
/// is absent.
///
/// # Safety
/// - `pml4` must point to a valid PML4 table accessible via the current
///   virtual address space.
/// - `set_hhdm_offset` must have been called before this function.
pub unsafe fn walk_read(pml4: *const u64, va: usize) -> Option<u64> {
    let i3 = (va >> 39) & 0x1FF;
    let i2 = (va >> 30) & 0x1FF;
    let i1 = (va >> 21) & 0x1FF;
    let i0 = (va >> 12) & 0x1FF;

    // SAFETY: pml4 is a valid PML4 pointer.
    let e3 = unsafe { core::ptr::read_volatile(pml4.add(i3)) };
    if e3 & PTE_PRESENT == 0 { return None; }
    let pdpt = phys_to_virt_ptr((e3 & !0xFFF) as usize) as *const u64;

    // SAFETY: pdpt derived from a present PML4 entry.
    let e2 = unsafe { core::ptr::read_volatile(pdpt.add(i2)) };
    if e2 & PTE_PRESENT == 0 { return None; }
    let pd = phys_to_virt_ptr((e2 & !0xFFF) as usize) as *const u64;

    // SAFETY: pd derived from a present PDPT entry.
    let e1 = unsafe { core::ptr::read_volatile(pd.add(i1)) };
    if e1 & PTE_PRESENT == 0 { return None; }
    let pt = phys_to_virt_ptr((e1 & !0xFFF) as usize) as *const u64;

    // SAFETY: pt derived from a present PD entry.
    let e0 = unsafe { core::ptr::read_volatile(pt.add(i0)) };
    if e0 & PTE_PRESENT == 0 { return None; }
    Some(e0)
}

// Internal aliases used by PageTableTrait impl below.
const PTE_P:  u64 = PTE_PRESENT;
const PTE_RW: u64 = PTE_WRITABLE;
const PTE_US: u64 = PTE_USER;
const PTE_PS: u64 = 1 << 7; // page-size (huge-page) bit — currently unused

#[repr(C, align(4096))]
pub struct PageTable { entries: [u64; 512] }
impl PageTable { pub const fn zero() -> Self { Self { entries: [0u64; 512] } } }

impl PageTableTrait for PageTable {
    fn init(&mut self) -> ViResult<PhysAddr> {
        self.entries = [0u64; 512];
        Ok(self as *mut _ as PhysAddr)
    }
    fn map(&mut self, virt: VAddr, phys: PhysAddr, flags: PageFlags,
           alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>) -> ViResult<()> {
        let i3 = (virt>>39)&0x1FF; let i2=(virt>>30)&0x1FF;
        let i1 = (virt>>21)&0x1FF; let i0=(virt>>12)&0x1FF;
        let pdpt = self.get_or_alloc(i3, alloc_fn)?;
        let pd   = pdpt.get_or_alloc(i2, alloc_fn)?;
        let pt   = pd.get_or_alloc(i1, alloc_fn)?;
        let mut e = phys as u64 | PTE_P;
        if flags.bits()&PageFlags::WRITE   !=0 { e|=PTE_RW; }
        if flags.bits()&PageFlags::USER    !=0 { e|=PTE_US; }
        if flags.bits()&PageFlags::EXECUTE ==0 { e|=PTE_NX; }
        pt.entries[i0] = e;
        Ok(())
    }
    fn unmap(&mut self, virt: VAddr) -> ViResult<()> {
        let e0=self.entries[(virt>>39)&0x1FF];
        if e0&PTE_P==0 { return Err(ViError::NotFound); }
        // SAFETY: e0 is a present PTE; the physical address is valid. Apply HHDM offset
        // so the pointer is dereferenceable under Limine's page tables.
        let pdpt: &mut PageTable = unsafe { &mut *(phys_to_virt_ptr((e0&!0xFFF) as usize) as *mut PageTable) };
        let e1=pdpt.entries[(virt>>30)&0x1FF];
        if e1&PTE_P==0 { return Err(ViError::NotFound); }
        // SAFETY: same as above.
        let pd: &mut PageTable = unsafe { &mut *(phys_to_virt_ptr((e1&!0xFFF) as usize) as *mut PageTable) };
        let e2=pd.entries[(virt>>21)&0x1FF];
        if e2&PTE_P==0 { return Err(ViError::NotFound); }
        // SAFETY: same as above.
        let pt: &mut PageTable = unsafe { &mut *(phys_to_virt_ptr((e2&!0xFFF) as usize) as *mut PageTable) };
        pt.entries[(virt>>12)&0x1FF] = 0;
        // SAFETY: invlpg flushes only the one virtual address from the TLB.
        unsafe { asm!("invlpg [{v}]", v=in(reg) virt, options(nomem)); }
        Ok(())
    }
    fn translate(&self, virt: VAddr) -> Option<PhysAddr> {
        let e0=self.entries[(virt>>39)&0x1FF]; if e0&PTE_P==0 {return None;}
        // SAFETY: e0 present; apply HHDM offset before dereferencing.
        let pdpt: &PageTable = unsafe { &*(phys_to_virt_ptr((e0&!0xFFF) as usize) as *const PageTable) };
        let e1=pdpt.entries[(virt>>30)&0x1FF]; if e1&PTE_P==0 {return None;}
        // SAFETY: e1 present.
        let pd: &PageTable = unsafe { &*(phys_to_virt_ptr((e1&!0xFFF) as usize) as *const PageTable) };
        let e2=pd.entries[(virt>>21)&0x1FF]; if e2&PTE_P==0 {return None;}
        if e2&PTE_PS!=0 { return Some(((e2&!0x1F_FFFF)+(virt&0x1F_FFFF) as u64) as PhysAddr); }
        // SAFETY: e2 present and not a huge page.
        let pt: &PageTable = unsafe { &*(phys_to_virt_ptr((e2&!0xFFF) as usize) as *const PageTable) };
        let e3=pt.entries[(virt>>12)&0x1FF]; if e3&PTE_P==0 {return None;}
        Some(((e3&!0xFFF)+(virt&0xFFF) as u64) as PhysAddr)
    }
    unsafe fn activate(&self) {
        // Physical address of this PageTable struct. Under Limine the struct
        // sits in HHDM-mapped RAM; we need the physical address for CR3.
        let virt = self as *const _ as usize;
        let offset = HHDM_OFFSET.load(Ordering::Relaxed);
        let phys = if virt >= offset && offset != 0 { virt - offset } else { virt } as u64;
        // SAFETY: phys is the physical address of this valid PML4; caller ensures
        // the kernel and the current stack are mapped so execution continues.
        unsafe { write_cr3(phys); }
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
    if hhdm == 0 { return; }

    // Active PML4 physical address (flags in low 12 bits stripped below).
    // SAFETY: reading CR3 is always valid in kernel mode.
    let cr3 = unsafe { read_cr3() } as usize & !0xFFF;

    // Macro: physical frame address → dereferenceable virtual pointer via HHDM.
    macro_rules! p2v {
        ($p:expr) => { ($p + hhdm) as *mut u64 };
    }

    // Walk PML4[256] → PDPT[0] → PD (all covering HHDM virtual base).
    // Index 256 corresponds to virtual 0xffff800000000000.
    let pml4 = p2v!(cr3);
    // SAFETY: cr3 is the live PML4 in usable RAM → accessible via HHDM.
    let e3 = unsafe { core::ptr::read_volatile(pml4.add(256)) };
    if e3 & PTE_PRESENT == 0 { return; }   // HHDM entirely absent
    if e3 & (1 << 7) != 0    { return; }   // 1 GiB leaf — everything mapped

    let pdpt = p2v!((e3 & !0xFFF) as usize);
    // SAFETY: PDPT is in usable RAM → accessible via HHDM.
    let e2 = unsafe { core::ptr::read_volatile(pdpt.add(0)) };
    if e2 & PTE_PRESENT == 0 { return; }   // First 1 GiB absent
    if e2 & (1 << 7) != 0    { return; }   // 1 GiB leaf — everything mapped

    let pd = p2v!((e2 & !0xFFF) as usize);

    // ── Part 1: BIOS area — fine-grained 4 KiB fix in PD[0]'s PT ────────────
    // SAFETY: PD is in usable RAM → accessible via HHDM.
    let e1 = unsafe { core::ptr::read_volatile(pd.add(0)) };
    if e1 & PTE_PRESENT != 0 && e1 & (1 << 7) == 0 {
        // PD[0] is a PT pointer; fill missing entries for [0x9F000, 0x100000).
        // PT covers [0, 2 MiB); entries 0x9F–0xFF are the BIOS/ROM area.
        let pt = p2v!((e1 & !0xFFF) as usize);
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

impl PageTable {
    fn get_or_alloc(&mut self, idx: usize, alloc_fn: &mut dyn FnMut()->Option<PhysAddr>)
        -> ViResult<&mut PageTable> {
        if self.entries[idx]&PTE_P==0 {
            let f = alloc_fn().ok_or(ViError::OutOfMemory)?;
            // SAFETY: f is a freshly allocated 4KB physical frame. Accessed via
            // HHDM virtual address so it is dereferenceable under Limine's PML4.
            unsafe { core::ptr::write_bytes(phys_to_virt_ptr(f) as *mut u8, 0, PAGE_SIZE) };
            self.entries[idx] = f as u64 | PTE_P | PTE_RW;
        }
        let next_phys = (self.entries[idx]&!0xFFF) as PhysAddr;
        // SAFETY: next_phys is a valid page table frame; HHDM offset makes it accessible.
        Ok(unsafe { &mut *(phys_to_virt_ptr(next_phys) as *mut PageTable) })
    }
}
