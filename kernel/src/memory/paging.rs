//! Paging management for ViCell kernel.
//!
//! Architecture dispatch:
//!   riscv64, aarch64   — SV39 / AArch64 page tables via HAL `PageTable`
//!   x86_64             — 4-level PML4 via HAL `PageTable`, read through the HHDM
//!   riscv32, x86, arm  — bare physical (no page tables); stubs return NotSupported

use crate::memory::frame::FrameAllocator;
use crate::memory::frame_run::Batch;
use crate::*;

#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
use hal::PageFlags;
#[cfg(cellos_wide)]
use hal::{PageTable, PageTableTrait};

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTableError {
    OutOfMemory,
    InvalidAddress,
    NotSupported,
    /// The PTE changed, but a remote hart did not confirm its invalidation
    /// within the probe bound: a lowering is not in force everywhere yet, so
    /// the caller must not report it as done.
    InvalidationUnconfirmed,
}

pub type PagingResult<T> = core::result::Result<T, PageTableError>;

// Re-export PageFlags from HAL. PageFlags is in hal-paging (always compiled) so
// it is available on all targets including riscv32.
pub use hal::PageFlags as Flags;

// Permission changes on existing mappings live in a sibling module but belong to
// this namespace — `paging::protect_page` is where a reader looks for them.
pub use super::page_protect::{protect_page, protect_pages, protect_range};

use crate::sync::Spinlock;

/// Global Kernel Root Page Table Address (physical).
pub static KERNEL_ROOT: Spinlock<Option<PhysAddr>> =
    Spinlock::ranked(None, crate::sync::rank::KERNEL_ROOT);

/// No-op paging init for bare physical addressing (RV32 Nano, SATP=0).
///
/// Phase-31 uses identity-mapped physical memory with no page tables.
/// Call in place of `init_kernel_paging` + `activate_paging` on riscv32.
pub fn init_bare() {}

/// The kernel's alias of the page-table root whose frame is `root_phys`: the
/// HHDM on x86_64, the identity map on riscv64 and aarch64.
///
/// A raw pointer, so each caller states its access: `&*` for a read-only walk
/// of a live root, `&mut *` only for the root's sole editor (the kernel root
/// under `KERNEL_ROOT`'s lock, or a root no other CPU can reach yet).
#[cfg(cellos_wide)]
pub(crate) fn table_at(root_phys: PhysAddr) -> *mut PageTable {
    crate::memory::frame::phys_to_virt(root_phys) as *mut PageTable
}

/// Intermediate tables of the shared kernel root are user-reachable: in a
/// shared address space the leaf's U/S bit is the only privilege check.
#[cfg(target_arch = "x86_64")]
pub(crate) const SAS_TABLES: hal::TablePolicy = hal::TablePolicy::UserReachable;

/// x86_64 kernel MMIO: supervisor, read/write, uncached, executable (`P|RW|PCD`
/// — the boot identity map never set NX on it).
#[cfg(target_arch = "x86_64")]
pub(crate) const MMIO_KERNEL_X86: Flags =
    Flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::UNCACHED);

/// x86_64 MMIO granted to a ring-3 Driver Cell: user, read/write, uncached,
/// never executable (`P|RW|US|PCD|NX`). Mirrors the riscv/aarch64
/// `cell_mmio_flags` posture — exclusivity is enforced by the resource
/// registry + LBI (cells are `forbid(unsafe_code)`, so only an `MmioRegion`
/// from a granted request can reach the range), not by per-cell page tables
/// (SAS has none).
#[cfg(target_arch = "x86_64")]
pub(crate) const MMIO_USER_X86: Flags =
    Flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::UNCACHED);

/// x86_64 user read/write data (`P|RW|US|NX`): framebuffers and user stacks.
#[cfg(target_arch = "x86_64")]
pub(crate) const USER_RW_X86: Flags = Flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER);

/// Flush all TLB entries. Call after unmapping PTEs to prevent stale entries.
///
/// RISC-V:   sfence.vma — full TLB shootdown from S-mode.
/// AArch64:  tlbi vmalle1is + dsb sy + isb — broadcast flush, all ASIDs, EL1.
/// x86_64:   all PCIDs and globals (INVPCID type 2 or untagged PGE toggle).
/// Bare physical (riscv32/x86/arm): no-op.
#[inline(always)]
pub fn tlb_flush_all() {
    crate::arch::tlb::flush_all();
}

// ─── riscv64 + aarch64 paging ────────────────────────────────────────────────

/// Initialize the kernel page table for riscv64 and aarch64.
///
/// Not used on x86_64 (separate path below) or bare-physical arches.
/// The returned root is intentionally not published through `KERNEL_ROOT`:
/// callers must activate it before acquiring global spinlocks on AArch64.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn init_kernel_paging(
    allocator: &mut FrameAllocator,
    mmap: &[crate::boot::MemoryMapEntry],
) -> PagingResult<PhysAddr> {
    #[cfg(target_arch = "aarch64")]
    {
        use hal::aarch64::backend::{ArmResources, Board};
        // The table depth must cover every identity-mapped window, not just
        // RAM: a device map above the RAM ceiling would otherwise fall outside
        // a 3-level walk.
        let parange = hal::aarch64::paging::read_hardware_parange();
        let ram_ends = mmap
            .iter()
            .map(|entry| (entry.base as u64).saturating_add(entry.length as u64));
        let mmio_ends = <Board as ArmResources>::KERNEL_MMIO_MAPS
            .iter()
            .map(|map| map.end as u64);
        let max_pa = ram_ends.chain(mmio_ends).max().unwrap_or(0);
        let config = hal::aarch64::paging::TranslationConfig::derive(parange, max_pa);
        hal::aarch64::paging::set_translation_config(config);
    }

    // 1. Allocate root page table frame and zero it.
    let root_frame = allocator
        .allocate_frame_uncapped()
        .ok_or(PageTableError::OutOfMemory)?;
    #[cfg(feature = "p01-trace")]
    crate::p01_retained::physical_table_created(root_frame, root_frame, 0);

    // SAFETY: root_frame is freshly allocated; we own it exclusively.
    unsafe {
        core::ptr::write_bytes(root_frame as *mut u8, 0, PAGE_SIZE);
    }

    let root_table = unsafe { &mut *(root_frame as *mut PageTable) };

    // 2. Identity-map all usable memory and kernel sections.
    for entry in mmap.iter() {
        let flags = match entry.ty {
            crate::boot::MemoryType::Usable => PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::EXECUTE
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            ),
            crate::boot::MemoryType::Kernel => PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::EXECUTE
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            ),
            crate::boot::MemoryType::Bootloader => PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            ),
            crate::boot::MemoryType::Framebuffer => PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            ),
            crate::boot::MemoryType::MMIO => PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            ),
            _ => continue,
        };

        let mut alloc_closure = || allocator.allocate_frame_uncapped();
        root_table
            .identity_map(
                entry.base,
                entry.base + entry.length,
                flags,
                &mut alloc_closure,
            )
            .map_err(|_| PageTableError::OutOfMemory)?;
    }

    // 3. Explicitly identity-map arch MMIO regions not in the memory map.
    //
    // AArch64: Device-nGnRnE (MAIR index 0) — required for MMIO.  Normal cacheable
    // (MAIR index 1) causes QEMU to reject EL0 device accesses with a Permission fault
    // even when the AP bits correctly grant EL0 read/write access.
    // RISC-V / other: no DEVICE bit; the arch paging code ignores it.
    #[cfg(target_arch = "aarch64")]
    let mmio_flags = PageFlags::from_bits(
        PageFlags::VALID
            | PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::DEVICE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY,
    );
    #[cfg(not(target_arch = "aarch64"))]
    let mmio_flags = PageFlags::from_bits(
        PageFlags::VALID
            | PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY,
    );
    let mut alloc_fn = || allocator.allocate_frame_uncapped();

    #[cfg(target_arch = "riscv64")]
    {
        let (clint_base, plic_base, plic_size, uart_region, rtc_region) =
            crate::platform::with(|p| {
                (
                    p.clint_base,
                    p.plic_base,
                    p.plic_size,
                    p.uart_base & !0xFFFF,
                    p.rtc_base & !0xFFF,
                )
            });
        root_table
            .identity_map(clint_base, clint_base + 0x10000, mmio_flags, &mut alloc_fn)
            .map_err(|_| PageTableError::OutOfMemory)?;
        root_table
            .identity_map(plic_base, plic_base + plic_size, mmio_flags, &mut alloc_fn)
            .map_err(|_| PageTableError::OutOfMemory)?;
        // Test-only terminal signaling on QEMU `virt` uses the SiFive test
        // device. Its page is outside RAM and must be present after paging
        // activates, otherwise a completed fixture faults while reporting.
        #[cfg(feature = "test-hooks")]
        root_table
            .identity_map(0x0010_0000, 0x0010_1000, mmio_flags, &mut alloc_fn)
            .map_err(|_| PageTableError::OutOfMemory)?;
        // UART page (0x10000000..0x10001000): kernel-only — S-mode with SUM=0 cannot
        // access USER pages, so the kernel UART driver (uart::init, poll_rhr, IRQ handler)
        // needs a non-USER mapping. Cells use sys_log / SBI; they do not touch UART MMIO.
        if uart_region != 0 {
            root_table
                .identity_map(uart_region, uart_region + 0x1000, mmio_flags, &mut alloc_fn)
                .map_err(|_| PageTableError::OutOfMemory)?;
        }
        // VirtIO MMIO (0x10001000–0x10010000): kernel-only at boot (mmio_flags).
        // Driver Cells reach their respective VirtIO slots via RequestMmio grants.
        let virtio_start = if uart_region != 0 {
            uart_region + 0x1000
        } else {
            0x10001000
        };
        root_table
            .identity_map(
                virtio_start,
                virtio_start + 0xF000,
                mmio_flags,
                &mut alloc_fn,
            )
            .map_err(|_| PageTableError::OutOfMemory)?;
        // Goldfish RTC (QEMU virt: 0x101000). Without this, sys_get_wall_secs/
        // sys_get_wall_time (op 2/3) dereference an unmapped MMIO address in
        // rtc::now_epoch_ns and the kernel takes a Load Page Fault (scause=13)
        // while servicing the syscall — first hit by the TLS handshake's cert
        // validity clock. The RTC window is one 4 KiB page.
        root_table
            .identity_map(rtc_region, rtc_region + 0x1000, mmio_flags, &mut alloc_fn)
            .map_err(|_| PageTableError::OutOfMemory)?;
        // PCIe ECAM bus-0 window (1 MiB at 0x3000_0000) for RISC-V virt gpex.
        // Required before pcie_ecam::init() accesses config space.
        // Only bus 0 is mapped; extend if a PCIe device lands on bus > 0.
        //
        // USER-inclusive (rv_cell_mmio, not kernel-only mmio_flags): the Platform
        // Cell (`/bin/platform`) runs in U-mode and scans ECAM directly (scan.rs →
        // mmio.rs read_volatile). With kernel-only flags its very first config read
        // (dev0 vendor-id at ECAM_BASE) took a Load Page Fault every boot, silently
        // killing the cell-driven PCIe scan (masked by never-die + the kernel S-mode
        // fallback scanner). The kernel's own pcie_ecam::init S-mode reads still work
        // against a USER page because SUM=1 (set right after activate_paging).
        root_table
            .identity_map(
                crate::dev::pcie_ecam::ECAM_BASE_RISCV,
                crate::dev::pcie_ecam::ECAM_BASE_RISCV + crate::dev::pcie_ecam::ECAM_BUS0_SIZE,
                mmio_flags,
                &mut alloc_fn,
            )
            .map_err(|_| PageTableError::OutOfMemory)?;
    }
    #[cfg(target_arch = "aarch64")]
    {
        use hal::aarch64::backend::{ArmResources, Board};
        // All device windows boot kernel-only (mmio_flags). A Cell reaches a
        // window solely through `RequestMmio`, which sets USER on the granted
        // whole pages (`grant_mmio_user`); retirement restores the boot posture
        // (`restore_mmio_boot_posture`).
        for map in <Board as ArmResources>::KERNEL_MMIO_MAPS {
            root_table
                .identity_map(map.base, map.end, mmio_flags, &mut alloc_fn)
                .map_err(|_| PageTableError::OutOfMemory)?;
        }
    }

    Ok(root_frame)
}

/// Activate virtual memory for riscv64 / aarch64.
///
/// # Safety
/// The root table MUST map the currently executing code and kernel stack.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub unsafe fn activate_paging(root_table_phys: PhysAddr) {
    // SAFETY: upheld by caller contract.
    let root_table = unsafe { &*(root_table_phys as *const PageTable) };
    // SAFETY: PageTable::activate writes SATP/TTBR0 using the table's physical address.
    unsafe {
        root_table.activate();
    }
}

// ─── riscv64 + aarch64 + x86_64: map_page / unmap_page ──────────────────────
//
// Every arch edits the kernel root through the HAL `PageTable` trait; the
// x86_64 legs (below) add the shared root's table policy and shootdowns.

/// Read-only diagnostic path inspection under KERNEL_ROOT. Stop at blocks/huge
/// leaves; never interpret leaf physical memory as another table.
#[cfg(all(feature = "p01-trace", cellos_wide))]
fn p01_observe_table_path(root: usize, va: usize, failed: bool, allocator: &FrameAllocator) {
    #[cfg(target_arch = "x86_64")]
    const LEVELS: &[usize] = &[39, 30, 21];
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    const LEVELS: &[usize] = &[30, 21];
    let mut table = root;
    for shift in LEVELS {
        let alias = crate::memory::frame::phys_to_virt(table) as *const u64;
        // SAFETY: the caller holds KERNEL_ROOT; each preceding descriptor was
        // positively a table, with the same geometry as the architecture HAL.
        let word = unsafe { core::ptr::read_volatile(alias.add((va >> shift) & 511)) };
        #[cfg(target_arch = "x86_64")]
        let next = if word & 1 == 0 || word & (1 << 7) != 0 {
            return;
        } else {
            (word & 0x000f_ffff_ffff_f000) as usize
        };
        #[cfg(target_arch = "riscv64")]
        let next = if word & 1 == 0 || word & 14 != 0 {
            return;
        } else {
            (((word >> 10) & 0x003f_ffff_ffff_ffff) << 12) as usize
        };
        #[cfg(target_arch = "aarch64")]
        let next = if word & 3 != 3 {
            return;
        } else {
            (word & !0xfff) as usize
        };
        // Inherited firmware tables may be outside managed bitmap memory; they
        // are not used_frames and must not acquire a fabricated allocator origin.
        if allocator.manages(next) {
            crate::p01_retained::physical_table_touched(next, root, failed);
        }
        table = next;
    }
}

/// Map a virtual page to a physical frame in the kernel address space.
///
/// riscv64/aarch64 issue no invalidation for a new mapping (the HAL map
/// neither does). A failed walk detaches the tables it published and retires
/// them into `batch`, which frees them only after a global flush confirms: a
/// remote hart may have cached a walk through them. The caller holds
/// `FRAME_ALLOCATOR` and finishes `batch` after releasing it. Returns
/// `NotSupported` on bare-physical arches (riscv32, x86_32, arm32).
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn map_page(
    allocator: &mut FrameAllocator,
    batch: &mut Batch,
    vaddr: VAddr,
    paddr: PhysAddr,
    flags: Flags,
) -> PagingResult<()> {
    let root_lock = KERNEL_ROOT.lock();
    if let Some(root_phys) = *root_lock {
        #[cfg(feature = "p01-trace")]
        let _physical_write = crate::p01_retained::physical_write();
        let root_table = unsafe { &mut *(root_phys as *mut PageTable) };
        let result = {
            let mut alloc_closure = || {
                let frame = allocator.allocate_frame_uncapped()?;
                #[cfg(feature = "p01-trace")]
                crate::p01_retained::physical_table_created(frame, root_phys, vaddr);
                Some(frame)
            };
            root_table.map(vaddr, paddr, flags, &mut alloc_closure)
        };
        #[cfg(feature = "p01-trace")]
        p01_observe_table_path(root_phys, vaddr, result.is_err(), allocator);
        if result.is_err() {
            let mut reclaimed = ReclaimedPageTables::new();
            root_table.prune_empty(vaddr, &mut |frame| reclaimed.record(frame));
            drop(root_lock);
            reclaimed.retire_into(batch);
            return Err(PageTableError::OutOfMemory);
        }
        Ok(())
    } else {
        Err(PageTableError::NotSupported)
    }
}

/// Unmap a virtual page from the kernel address space (clears the PTE).
/// Used to create guard pages. Returns Ok(()) if the page was not mapped.
///
/// The HAL unmap already invalidates (riscv64: local `sfence.vma`; aarch64:
/// broadcast `tlbi`), so `batch` is left untouched: a caller that needs the
/// remote harts too records the page itself, as it always flushed it itself.
/// Returns `NotSupported` on bare-physical arches.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn unmap_page(batch: &mut Batch, vaddr: VAddr) -> PagingResult<()> {
    let _ = batch;
    let root_lock = KERNEL_ROOT.lock();
    if let Some(root_phys) = *root_lock {
        let root_table = unsafe { &mut *(root_phys as *mut PageTable) };
        match root_table.unmap(vaddr) {
            Ok(()) => Ok(()),
            Err(_) => Ok(()), // Not mapped — that is acceptable for guard-page creation
        }
    } else {
        Err(PageTableError::NotSupported)
    }
}

// ─── x86_64 paging ───────────────────────────────────────────────────────────
//
// Limine boot environment:
//   - Physical RAM is mapped at HHDM_BASE + phys (NOT identity-mapped).
//   - Kernel text/data live at 0xFFFFFFFF80000000 (higher half).
//   - MMIO (LAPIC, HPET, IOAPIC) is NOT in the HHDM; must be identity-mapped.
//
// Strategy for init_kernel_paging:
//   1. Allocate a new PML4 frame and zero it.
//   2. Copy PML4 entries [256..512] from Limine's CR3 — preserves kernel+HHDM.
//   3. Identity-map validated ACPI MMIO (VA = PA) for timer/APIC access.
//   4. Store root in KERNEL_ROOT and return phys address.
//
// Every edit goes through the HAL `PageTable` trait on the HHDM alias of the
// root (`table_at`), with `SAS_TABLES` for the intermediate tables; the HAL
// walkers read every lower table through its `phys_to_virt`, so they work
// before and after the CR3 switch.

/// Build the kernel's own PML4 for x86_64.
///
/// Must be called after `set_phys_offset` (HHDM base) is stored in
/// `memory::frame` and after the HAL paging walker has the HHDM offset
/// set via `hal::paging::set_hhdm_offset`.
///
/// Parameters:
/// - `ioapic_base`: I/O APIC physical address from validated ACPI MADT
/// - `hpet_base`:   HPET physical address from the validated ACPI HPET table
/// - `lapic_base`:  Local APIC physical address from validated ACPI MADT
/// - `ecam_base`:   ECAM allocation base from validated ACPI MCFG
/// - `ecam_len`:    checked byte length of the inclusive MCFG bus range
///
/// All admitted MMIO regions are identity-mapped (VA == PA) so early hardware
/// initialization and the Platform Cell can access them after the CR3 switch.
///
/// Preconditions:
/// - The frame allocator is initialised.
/// - `hal::paging::set_hhdm_offset(hhdm)` has been called.
///
/// # Panics
/// Panics on OOM (any frame allocation fails).
#[cfg(target_arch = "x86_64")]
pub fn init_kernel_paging_x86(
    allocator: &mut FrameAllocator,
    ioapic_base: u64,
    hpet_base: u64,
    lapic_base: u64,
    ecam_base: u64,
    ecam_len: usize,
) -> PagingResult<PhysAddr> {
    use crate::memory::frame::phys_to_virt;
    use hal::paging::read_cr3;

    // 1. Allocate and zero a new PML4 frame.
    let pml4_phys = allocator
        .allocate_frame_uncapped()
        .ok_or(PageTableError::OutOfMemory)?;
    #[cfg(feature = "p01-trace")]
    crate::p01_retained::physical_table_created(pml4_phys, pml4_phys, 0);
    // SAFETY: pml4_phys is a freshly allocated 4KB frame; HHDM virt pointer is valid.
    unsafe {
        core::ptr::write_bytes(phys_to_virt(pml4_phys) as *mut u8, 0, PAGE_SIZE);
    }

    // SAFETY: the zeroed frame is ours until KERNEL_ROOT publishes it below.
    let pml4 = unsafe { &mut *table_at(pml4_phys) };

    // 2. Copy higher-half PML4 entries [256..512] from Limine's active PML4.
    //    These entries cover 0xFFFF_8000_0000_0000..0xFFFF_FFFF_FFFF_FFFF and include:
    //      - the HHDM window (index 256..384 depending on Limine's choice)
    //      - the kernel text/data at 0xFFFFFFFF80000000 (index 511)
    //    Copying them wholesale is safe: they remain read-only during init.
    {
        // SAFETY: read_cr3() returns the physical address of the live Limine PML4.
        let limine_cr3 = unsafe { read_cr3() } as usize;
        // SAFETY: the live Limine PML4, a 512-entry table read through the HHDM.
        let limine_pml4 = unsafe { &*(phys_to_virt(limine_cr3) as *const PageTable) };
        pml4.copy_kernel_higher_half(limine_pml4);
    }

    // 3. Identity-map MMIO regions so the CPU can reach them at VA == PA after
    //    the CR3 switch.  Physical addresses are below the 4GB boundary and NOT
    //    in the Limine HHDM window, so they must be explicitly identity-mapped.
    //
    //    The bases come from validated ACPI tables. A zero base is deliberately
    //    skipped so malformed firmware cannot fall through to q35-specific MMIO.
    //
    //    IOAPIC:  ioapic_base (MADT)                — 4 KB
    //    HPET:    hpet_base   (HPET table)          — 4 KB
    //    LAPIC:   lapic_base  (MADT)                — 4 KB
    let ioapic_base_usize = ioapic_base as usize;
    let hpet_base_usize = hpet_base as usize;
    let lapic_base_usize = lapic_base as usize;
    let ecam_base_usize = ecam_base as usize;
    let ecam_end = if ecam_base_usize == 0 {
        0
    } else {
        ecam_base_usize
            .checked_add(ecam_len)
            .filter(|end| *end > ecam_base_usize)
            .ok_or(PageTableError::InvalidAddress)?
    };
    let mmio_regions: &[(usize, usize)] = &[
        (ioapic_base_usize, ioapic_base_usize + PAGE_SIZE), // IOAPIC (from ACPI)
        (hpet_base_usize, hpet_base_usize + PAGE_SIZE),     // HPET   (from ACPI)
        (lapic_base_usize, lapic_base_usize + PAGE_SIZE),   // LAPIC  (from ACPI)
        (ecam_base_usize, ecam_end),                        // complete MCFG allocation
    ];
    for &(start, end) in mmio_regions {
        if start == 0 {
            continue;
        }
        let mut va = start;
        while va < end {
            let mut alloc_fn = || {
                let frame = allocator.allocate_frame_uncapped()?;
                #[cfg(feature = "p01-trace")]
                crate::p01_retained::physical_table_created(frame, pml4_phys, va);
                Some(frame)
            };
            pml4.map_with(va, va, MMIO_KERNEL_X86, SAS_TABLES, &mut alloc_fn)
                .map_err(|_| PageTableError::OutOfMemory)?;
            va += PAGE_SIZE;
        }
    }

    // Sanity: verify the IOAPIC PTE was written correctly.
    // Uses the runtime ioapic_base parsed from validated ACPI.
    if ioapic_base_usize != 0 {
        debug_assert!(
            pml4.leaf_entry(ioapic_base_usize).is_some(),
            "IOAPIC identity-map sanity check failed"
        );
    }

    log::info!("[kernel] x86_64 paging: kernel PML4 built");

    *KERNEL_ROOT.lock() = Some(pml4_phys);
    crate::hal::domain::record_kernel_cr3(pml4_phys);
    Ok(pml4_phys)
}

/// Activate paging on x86_64 by writing the PML4 physical address to CR3.
///
/// # Safety
/// The PML4 at `root_phys` must map the currently executing code, the kernel
/// stack, and any data the kernel needs immediately after this call.
/// An invalid mapping causes a triple-fault.
#[cfg(target_arch = "x86_64")]
pub unsafe fn activate_paging(root_phys: PhysAddr) {
    // PCID is decided here, before the first CR3 write: the tag policy depends on
    // CR4.PCIDE, and a nonzero PCID programmed while it is clear is a #GP. When
    // the CPU cannot carry a tag, domains use tag 0 and a full CR3 flush.
    let _ = hal::domain::init_pcid();
    // SAFETY: caller guarantees root_phys is a valid, fully populated PML4 that
    // keeps the kernel alive after the CR3 switch. Page-aligned, so tag 0.
    unsafe {
        hal::paging::write_cr3(root_phys as u64);
    }
    // Report the decision here, as soon as it is made: later boot stages
    // (Tier-3 VMM bring-up) can hang on a given accelerator, which would hide a
    // witness for a decision that was already made.
    emit_pcid_decision();
    log::info!("[kernel] paging activated");
}

/// Log the PCID decision as one line, with the live CR3 so the tag bits that
/// the policy produced are visible, not inferred.
#[cfg(target_arch = "x86_64")]
fn emit_pcid_decision() {
    let usable = hal::domain::pcid_usable();
    log::info!(
        "[kernel] x86_64 paging: PCID {} (CPUID pcid={} invpcid={}, CR4.PCIDE={}, CR3={:#x}{})",
        if usable { "enabled" } else { "disabled" },
        hal::domain::pcid_supported(),
        hal::domain::invpcid_supported(),
        u8::from(hal::domain::pcide_set()),
        hal::domain::read_cr3(),
        if usable {
            ""
        } else {
            " — domain roots use tag 0 with a full CR3 flush"
        },
    );
}

/// Map one 4 KiB page of the shared kernel root (x86_64) without invalidating
/// `vaddr`. A leaf that replaced a present one is recorded in `batch`: x86
/// caches no not-present entry, so only a replacement can leave a stale
/// translation. A failed walk detaches the tables it published and retires
/// them into `batch`, which frees them only after the flush confirms. The
/// caller holds `FRAME_ALLOCATOR` and finishes `batch` after releasing it.
#[cfg(target_arch = "x86_64")]
fn map_page_unflushed(
    allocator: &mut FrameAllocator,
    batch: &mut Batch,
    vaddr: VAddr,
    paddr: PhysAddr,
    flags: Flags,
) -> PagingResult<()> {
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or(PageTableError::NotSupported)?;
    #[cfg(feature = "p01-trace")]
    let _physical_write = crate::p01_retained::physical_write();
    // SAFETY: KERNEL_ROOT names the kernel's live PML4 and we hold its lock.
    let table = unsafe { &mut *table_at(root_phys) };
    let replaced = table.leaf_entry(vaddr).is_some();

    let result = {
        let mut alloc_fn = || {
            let frame = allocator.allocate_frame_uncapped()?;
            #[cfg(feature = "p01-trace")]
            crate::p01_retained::physical_table_created(frame, root_phys, vaddr);
            Some(frame)
        };
        table.map_with(vaddr, paddr, flags, SAS_TABLES, &mut alloc_fn)
    };
    #[cfg(feature = "p01-trace")]
    p01_observe_table_path(root_phys, vaddr, result.is_err(), allocator);
    if result.is_err() {
        // A failed walk may have published intermediate tables. Detach them
        // first; the batch invalidates every CPU's page-walk caches before it
        // recycles their frames.
        let mut reclaimed = ReclaimedPageTables::new();
        table.prune_empty(vaddr, &mut |frame| reclaimed.record(frame));
        drop(root_lock);
        reclaimed.retire_into(batch);
        return Err(PageTableError::OutOfMemory);
    }
    if replaced {
        batch.invalidate(vaddr);
    }
    Ok(())
}

/// Identity-map a MMIO BAR region [phys, phys+size) at VA == PA (x86_64).
///
/// Called by PCIe drivers at init time to make dynamically-assigned BAR windows
/// accessible as identity-mapped VA == PA. Each page in the range is mapped with
/// MMIO cache-disabling flags; existing mappings are silently overwritten.
///
/// Panics on OOM or if the frame allocator is not yet initialised.
#[cfg(target_arch = "x86_64")]
pub fn map_mmio_x86(phys: usize, size: usize) {
    map_mmio_x86_flags(phys, size, MMIO_KERNEL_X86);
}

/// Identity-map a MMIO region with USER access (x86_64).
///
/// Called by the `RequestMmio` syscall when a ring-3 Driver Cell claims a
/// range: the SAS identity map built at boot is kernel-only, so the claimed
/// window must be (re)mapped with `PTE_USER` before the cell can touch it.
/// Mirrors the riscv/aarch64 posture where cell-owned MMIO windows are mapped
/// user-accessible — exclusivity is enforced by the resource registry + LBI
/// (`forbid(unsafe_code)` cells cannot fabricate an MMIO dereference), not by
/// the U/S bit.
#[cfg(target_arch = "x86_64")]
pub fn map_mmio_user_x86(phys: usize, size: usize) {
    map_mmio_x86_flags(phys, size, MMIO_USER_X86);
}

/// Page-aligned identity window `[start, end)` covering `[phys, phys + size)`.
#[cfg(target_arch = "x86_64")]
fn mmio_page_window(phys: usize, size: usize) -> (usize, usize) {
    let page_mask = PAGE_SIZE - 1;
    (phys & !page_mask, (phys + size + page_mask) & !page_mask)
}

/// Write every leaf of the window first, then invalidate the whole window on
/// every online CPU with ONE shared flush (phase 07b), after the frame
/// allocator is released: a per-page shootdown made the 256 MiB ECAM window
/// cost 65,536 broadcast rounds under `FRAME_ALLOCATOR`, which grew with the
/// CPU count. The flush runs before this function returns. The window only
/// gains rights (census §3 row 4), so a CPU that does not confirm within the
/// probe can only fault on its older, narrower entry: the batch logs it and
/// the map proceeds.
#[cfg(target_arch = "x86_64")]
fn map_mmio_x86_flags(phys: usize, size: usize, flags: Flags) {
    let (start, end) = mmio_page_window(phys, size);
    let mut batch = Batch::new();
    crate::memory::frame::with(|allocator| {
        let mut va = start;
        while va < end {
            // Errors are logged but not fatal; a broken mapping will fault on first use.
            if let Err(e) = map_page_unflushed(allocator, &mut batch, va, va, flags) {
                log::warn!("[paging] map_mmio_x86: failed to map {:#x} ({:?})", va, e);
            }
            va += PAGE_SIZE;
        }
    })
    .expect("map_mmio_x86: frame allocator not ready");
    batch.invalidate_range(start, end - start);
    // A new MMIO window or a raised right: a CPU that has not confirmed yet can
    // only fault on the old (absent or narrower) entry. Logged by the batch.
    let _ = batch.complete();
}

/// Unmap a page of the shared kernel root (x86_64): clears the leaf and
/// records it in `batch`, whose finish invalidates it on every online CPU.
/// Returns Ok(()) even if the page was already unmapped (idempotent — safe for
/// guard-page creation); only a present leaf is cleared and recorded.
#[cfg(target_arch = "x86_64")]
pub fn unmap_page(batch: &mut Batch, vaddr: VAddr) -> PagingResult<()> {
    let root_lock = KERNEL_ROOT.lock();
    let Some(root_phys) = *root_lock else {
        return Ok(()); // paging not yet active — no TLB entry to clear
    };
    // SAFETY: KERNEL_ROOT names the kernel's live PML4 and we hold its lock.
    let table = unsafe { &mut *table_at(root_phys) };
    if table.leaf_entry(vaddr).is_some() && table.unmap(vaddr).is_ok() {
        batch.invalidate(vaddr);
    }
    Ok(())
}

// ─── MMIO user-access teardown (runtime revocation) ─────────────────────────
//
// The revoke counterpart of the `RequestMmio` mapping path: granting a window
// makes it reachable from ring 3, these take that reach away. Every leg keeps
// the frame identity-mapped for the kernel — SAS requires it, and the kernel
// keeps using MMIO windows after a Cell loses them.

/// Remove *user* accessibility from `[base, base+len)`.
///
/// The frame cannot be unmapped — the kernel still uses the window. Boot maps
/// every MMIO window kernel-only; a window is user-mapped only while a
/// `RequestMmio` grant holds it. Lowering the page's permissions makes the
/// Cell's next access fault.
/// Pages that are not mapped are skipped; a mapped page that cannot be
/// re-protected is an error, because that page would stay user-reachable.
#[cfg(cellos_wide)]
pub fn clear_mmio_user(base: usize, len: usize) -> PagingResult<usize> {
    let Some(end) = checked_page_end(base, len) else {
        return Ok(0);
    };
    let mut cleared = 0;
    let mut va = base & !(PAGE_SIZE - 1);
    while va < end {
        match protect_page(va, mmio_kernel_flags()) {
            Ok(()) => cleared += 1,
            // Not mapped: nothing to take away.
            Err(PageTableError::InvalidAddress) => {}
            Err(e) => {
                log::error!("[paging] clear_mmio_user: {va:#x} still user-reachable ({e:?})");
                return Err(e);
            }
        }
        va += PAGE_SIZE;
    }
    Ok(cleared)
}

/// Make the whole pages of `[base, base+len)` reachable from EL0/U-mode on
/// RISC-V/AArch64.
///
/// The counterpart of [`clear_mmio_user`], called by `RequestMmio` once the
/// registry granted the window (and by the capability self-test). Only pages
/// lying entirely inside the window gain USER: a page shared with registers
/// the Cell was not granted stays kernel-only, and the kernel mediates the
/// Cell's accesses to its granted bytes instead (`resource_registry`
/// mediation). Unmapped pages are skipped.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn grant_mmio_user(base: usize, len: usize) -> PagingResult<usize> {
    let mut granted = 0;
    for va in crate::resource_registry::user_pages(base, len).step_by(PAGE_SIZE) {
        match protect_page(va, mmio_user_flags()) {
            Ok(()) => granted += 1,
            // Not mapped: nothing to grant.
            Err(PageTableError::InvalidAddress) => {}
            Err(e) => {
                log::error!("[paging] grant_mmio_user: {va:#x} not user-reachable ({e:?})");
                return Err(e);
            }
        }
    }
    Ok(granted)
}

/// Return the whole pages of `[base, base+len)` to the access the boot map
/// gave them, once no Cell holds the window any more.
///
/// Every boot MMIO window is kernel-only on RISC-V and AArch64, so restoring
/// is clearing USER.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn restore_mmio_boot_posture(base: usize, len: usize) -> PagingResult<usize> {
    clear_mmio_user(base, len)
}

/// Boot MMIO mapping flags *with* `USER` — what an allowlisted window carries
/// while a Cell may reach it.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
fn mmio_user_flags() -> Flags {
    #[cfg(target_arch = "riscv64")]
    let bits =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;
    #[cfg(target_arch = "aarch64")]
    let bits = Flags::VALID
        | Flags::READ
        | Flags::WRITE
        | Flags::USER
        | Flags::DEVICE
        | Flags::ACCESSED
        | Flags::DIRTY;
    Flags::from_bits(bits)
}

/// Boot MMIO mapping flags with `USER` removed — what the kernel keeps after a
/// Cell's access to the window is revoked.
#[cfg(cellos_wide)]
pub(crate) fn mmio_kernel_flags() -> Flags {
    #[cfg(target_arch = "riscv64")]
    let bits = Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY;
    #[cfg(target_arch = "aarch64")]
    let bits =
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::DEVICE | Flags::ACCESSED | Flags::DIRTY;
    #[cfg(target_arch = "x86_64")]
    let bits = Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY;
    Flags::from_bits(bits)
}

/// End of the page-rounded range, or `None` when `base + len` overflows.
fn checked_page_end(base: usize, len: usize) -> Option<usize> {
    let page_mask = PAGE_SIZE - 1;
    base.checked_add(len)
        .map(|end| (end + page_mask) & !page_mask)
}

/// Test-hooks only: `(mapped, user_accessible)` for `vaddr`.
///
/// Reads the leaf the hardware walks, so a teardown assertion observes the
/// permission bits a revoked Cell would hit rather than a kernel-side flag.
#[cfg(feature = "test-hooks")]
pub fn mapping_state(vaddr: VAddr) -> (bool, bool) {
    #[cfg(cellos_wide)]
    {
        let Some(root_phys) = *KERNEL_ROOT.lock() else {
            return (false, false);
        };
        // SAFETY: the live kernel root table, walked read-only.
        let root = unsafe { &*table_at(root_phys) };
        let Some(leaf) = root.leaf(vaddr) else {
            return (false, false);
        };
        (leaf.valid, leaf.valid && leaf.user)
    }
    #[cfg(not(cellos_wide))]
    {
        let _ = vaddr;
        (false, false)
    }
}

/// Remove user accessibility from `[base, base+len)` on the running arch.
///
/// Used by runtime capability revocation: after this returns `Ok`, the next
/// access from the revoked Cell to the window faults, and the kernel's own
/// mapping of the frame is unchanged. `Err` means at least one mapped page is
/// still user-reachable and the caller must not report the revoke as complete.
pub fn revoke_mmio_user(base: usize, len: usize) -> PagingResult<usize> {
    #[cfg(cellos_wide)]
    {
        clear_mmio_user(base, len)
    }
    #[cfg(not(cellos_wide))]
    {
        let _ = (base, len);
        Ok(0)
    }
}

// ─── x86_64 map_page / map_page_deferred ─────────────────────────────────────

/// Map a 4 KiB page of the shared kernel root (x86_64). A leaf that replaced
/// a present one is recorded in `batch`, which the caller finishes after
/// releasing `FRAME_ALLOCATOR` and before anything relies on the new leaf; a
/// new leaf needs no invalidation (x86 caches no not-present entry).
///
/// The HAL translates `flags` for a new leaf and for `protect_page` alike, so
/// a page cannot mean one thing when created and another when re-protected.
/// Uncached (MMIO) mappings pass `Flags::UNCACHED`.
#[cfg(target_arch = "x86_64")]
pub fn map_page(
    allocator: &mut FrameAllocator,
    batch: &mut Batch,
    vaddr: VAddr,
    paddr: PhysAddr,
    flags: Flags,
) -> PagingResult<()> {
    map_page_unflushed(allocator, batch, vaddr, paddr, flags)
}

/// [`map_page`] for one page of a batch whose invalidation the caller issues
/// once, with [`flush_deferred_maps`] over the batch (Cell ELF segments, phase
/// 07b R1, owner grant `s2-phase07b-flush-batching-grant.md`). Now that
/// [`map_page`] records into a batch on every arch, the two are one function;
/// the name keeps the loader's contract visible.
///
/// Precondition: the leaf is new (absent before this call) or only gains
/// rights over what it already maps. Never use this to re-point a present leaf
/// to another frame or to remove a right, and never free a frame a leaf mapped
/// this way referred to before the batch has been finished — until then
/// another CPU may still hold the old translation.
pub(crate) fn map_page_deferred(
    allocator: &mut FrameAllocator,
    batch: &mut Batch,
    vaddr: VAddr,
    paddr: PhysAddr,
    flags: Flags,
) -> PagingResult<()> {
    map_page(allocator, batch, vaddr, paddr, flags)
}

/// Cover a [`map_page_deferred`] range `[start, end)` in `batch`.
///
/// x86_64: the range joins the batch, whose finish is ONE shared flush that
/// must complete before the spawn that maps the pages relocates through them
/// or returns. Other arches: nothing, as their mappings defer nothing.
pub(crate) fn flush_deferred_maps(batch: &mut Batch, start: VAddr, end: VAddr) {
    if cfg!(target_arch = "x86_64") && end > start {
        batch.invalidate_range(start, end - start);
    }
}

/// Intermediate page-table frames detached from the active root.
pub struct ReclaimedPageTables {
    frames: [PhysAddr; 3],
    len: usize,
}

impl ReclaimedPageTables {
    fn new() -> Self {
        Self {
            frames: [0; 3],
            len: 0,
        }
    }

    fn record(&mut self, frame: PhysAddr) {
        assert!(
            self.len < self.frames.len(),
            "page-table depth exceeds reclaim bound"
        );
        self.frames[self.len] = frame;
        self.len += 1;
        #[cfg(feature = "p01-trace")]
        crate::p01_retained::physical_table_detached(frame);
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Hand the detached tables to `batch`, which frees them only after a
    /// **global** flush confirms (census P10). A per-address flush is not
    /// enough for a detached table: RV64 `SFENCE.VMA` with an address orders
    /// only the leaf entry for it, not the non-leaf entries a walk cached. x86
    /// flushes everything for a shared invalidation anyway, and the batch
    /// issues one flush per operation however many tables it detaches.
    pub fn retire_into(self, batch: &mut Batch) {
        if self.len == 0 {
            return;
        }
        batch.invalidate_all();
        for frame in self.frames[..self.len].iter().copied() {
            batch.retire(crate::memory::frame_run::FrameRun::unpublished(
                frame,
                1,
                crate::memory::frame_run::RunOwner::Kernel,
                crate::memory::frame_run::RunKind::PageTable,
                crate::memory::frame_run::KernelAlias::Intact,
            ));
        }
    }
}

/// Detach empty intermediate tables after their final leaf was unmapped.
#[cfg(cellos_wide)]
pub fn prune_empty_tables(vaddr: VAddr) -> ReclaimedPageTables {
    let mut reclaimed = ReclaimedPageTables::new();
    let root = KERNEL_ROOT.lock();
    #[cfg(feature = "p01-trace")]
    let _physical_write = crate::p01_retained::physical_write();
    if let Some(root_phys) = *root {
        // SAFETY: KERNEL_ROOT names the live architecture page-table root and
        // we hold its lock.
        let table = unsafe { &mut *table_at(root_phys) };
        table.prune_empty(vaddr, &mut |frame| reclaimed.record(frame));
    }
    reclaimed
}

#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn prune_empty_tables(_vaddr: VAddr) -> ReclaimedPageTables {
    ReclaimedPageTables::new()
}

/// Test-hooks (x86_64): which live page-table role a frame holds, if any.
///
/// A frame that is still allocated after a mapping cycle is torn down is only
/// identifiable by address — a leaked page table, segment frame and stack frame
/// are all one 4 KiB frame — so this walks the kernel root and reports the first
/// intermediate table entry that names it, with the VA window that entry covers.
/// Leaf (data) frames are reported as such; anything the root cannot reach is an
/// orphan, which is what a leaked table looks like.
#[cfg(all(feature = "test-hooks", target_arch = "x86_64"))]
pub fn classify_table_frame(frame: PhysAddr) -> alloc::string::String {
    use alloc::format;
    use alloc::string::String;
    use hal::paging::TableLevel;

    let Some(root) = *KERNEL_ROOT.lock() else {
        return String::from("no-root");
    };
    if root == frame {
        return String::from("PML4-root");
    }
    // SAFETY: the kernel root and every table it names through present,
    // non-huge entries are live kernel page tables.
    let table = unsafe { &*table_at(root) };
    match table.find_table(frame) {
        Some((TableLevel::Pdpt, base)) => {
            format!("PDPT for VA 0x{:X}-0x{:X}", base, base + (1 << 39))
        }
        Some((TableLevel::Pd, base)) => format!("PD for VA 0x{:X}-0x{:X}", base, base + (1 << 30)),
        Some((TableLevel::Pt, base)) => format!("PT for VA 0x{:X}-0x{:X}", base, base + (1 << 21)),
        None => String::from("not-a-live-table"),
    }
}

// ─── remap_range_user ────────────────────────────────────────────────────────

/// Remap a range of already-allocated pages with user (U/S=1) permissions.
///
/// Used for user stacks which sit in identity-mapped usable RAM.
/// On x86_64 we re-walk and rewrite the PTEs with USER|WRITABLE|NX.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn remap_range_user(start: PhysAddr, pages: usize) {
    let mut root_guard = KERNEL_ROOT.lock();
    if let Some(root_addr) = root_guard.as_mut() {
        let table = unsafe { &mut *(*root_addr as *mut hal::paging::PageTable) };
        let _ = crate::memory::frame::with(|allocator| {
            #[cfg(feature = "p01-trace")]
            let _physical_write = crate::p01_retained::physical_write();
            use hal::traits::PageTableTrait;
            let flags = PageFlags::from_bits(
                PageFlags::VALID
                    | PageFlags::READ
                    | PageFlags::WRITE
                    | PageFlags::USER
                    | PageFlags::ACCESSED
                    | PageFlags::DIRTY,
            );
            use hal::paging::PAGE_SIZE;
            for i in 0..pages {
                let addr = start + (i * PAGE_SIZE);
                let mut alloc_closure = || {
                    let frame = allocator.allocate_frame_uncapped()?;
                    #[cfg(feature = "p01-trace")]
                    crate::p01_retained::physical_table_created(frame, *root_addr, addr);
                    Some(frame)
                };
                let result = table.map(addr, addr, flags, &mut alloc_closure);
                #[cfg(feature = "p01-trace")]
                p01_observe_table_path(*root_addr, addr, result.is_err(), allocator);
                result.expect("Failed to map user stack page!");
            }
        });
    }
}

/// Map a firmware-owned scanout range at its identity address for trusted cells.
///
/// Cellos Tier-1 uses a shared address space: `USER` makes this range visible
/// to every trusted cell, not only the registering display cell. The syscall
/// authority check prevents accidental arbitrary mappings; it is not hardware
/// per-cell isolation.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn map_display_framebuffer_user(start: PhysAddr, pages: usize) -> PagingResult<()> {
    if pages == 0 || start & (PAGE_SIZE - 1) != 0 {
        return Err(PageTableError::InvalidAddress);
    }
    let flags = Flags::from_bits(
        Flags::VALID
            | Flags::READ
            | Flags::WRITE
            | Flags::USER
            | Flags::DEVICE
            | Flags::ACCESSED
            | Flags::DIRTY,
    );
    crate::memory::frame::with(|allocator| -> PagingResult<()> {
        let root = KERNEL_ROOT.lock();
        let Some(root_phys) = *root else {
            return Err(PageTableError::NotSupported);
        };
        #[cfg(feature = "p01-trace")]
        let _physical_write = crate::p01_retained::physical_write();
        // SAFETY: KERNEL_ROOT names the active kernel-owned page table. The frame
        // allocator remains locked throughout this transaction, matching map_page.
        let table = unsafe { &mut *(root_phys as *mut PageTable) };
        for index in 0..pages {
            let address = start
                .checked_add(
                    index
                        .checked_mul(PAGE_SIZE)
                        .ok_or(PageTableError::InvalidAddress)?,
                )
                .ok_or(PageTableError::InvalidAddress)?;
            if table.translate(address).is_some() {
                return Err(PageTableError::InvalidAddress);
            }
        }
        for index in 0..pages {
            let address = start
                .checked_add(
                    index
                        .checked_mul(PAGE_SIZE)
                        .ok_or(PageTableError::InvalidAddress)?,
                )
                .ok_or(PageTableError::InvalidAddress)?;
            let mut allocate_table_frame = || {
                let frame = allocator.allocate_frame_uncapped()?;
                #[cfg(feature = "p01-trace")]
                crate::p01_retained::physical_table_created(frame, root_phys, address);
                Some(frame)
            };
            let result = table.map(address, address, flags, &mut allocate_table_frame);
            #[cfg(feature = "p01-trace")]
            p01_observe_table_path(root_phys, address, result.is_err(), allocator);
            if result.is_err() {
                for rollback in 0..index {
                    let address = start + rollback * PAGE_SIZE;
                    let _ = table.unmap(address);
                }
                tlb_flush_all();
                return Err(PageTableError::OutOfMemory);
            }
        }
        tlb_flush_all();
        Ok(())
    })
    .unwrap_or(Err(PageTableError::NotSupported))
}

#[cfg(target_arch = "x86_64")]
pub fn map_display_framebuffer_user(start: PhysAddr, pages: usize) -> PagingResult<()> {
    if pages == 0 || start & (PAGE_SIZE - 1) != 0 {
        return Err(PageTableError::InvalidAddress);
    }

    let mut batch = Batch::new();
    let mapped = crate::memory::frame::with(|allocator| -> PagingResult<()> {
        let root_phys = {
            let root = KERNEL_ROOT.lock();
            match *root {
                Some(r) => r,
                None => return Err(PageTableError::NotSupported),
            }
        };
        // SAFETY: the kernel's live PML4, walked read-only; FRAME_ALLOCATOR is
        // held, so no table frame on the walk can be recycled under it.
        let root = unsafe { &*table_at(root_phys) };

        for index in 0..pages {
            let address = start
                .checked_add(
                    index
                        .checked_mul(PAGE_SIZE)
                        .ok_or(PageTableError::InvalidAddress)?,
                )
                .ok_or(PageTableError::InvalidAddress)?;
            if root.leaf_entry(address).is_some() {
                return Err(PageTableError::InvalidAddress);
            }
        }

        for index in 0..pages {
            let address = start + index * PAGE_SIZE;
            let result = map_page(allocator, &mut batch, address, address, USER_RW_X86);
            if result.is_err() {
                for rollback in 0..index {
                    let rollback_addr = start + rollback * PAGE_SIZE;
                    let _ = unmap_page(&mut batch, rollback_addr);
                }
                tlb_flush_all();
                return Err(PageTableError::OutOfMemory);
            }
        }
        tlb_flush_all();
        Ok(())
    })
    .unwrap_or(Err(PageTableError::NotSupported));
    // Every leaf was absent before (checked above), so only a rollback or a
    // failed walk's tables are in the batch.
    let flushed = batch.complete();
    mapped?;
    if !flushed.is_confirmed() {
        return Err(PageTableError::InvalidationUnconfirmed);
    }
    Ok(())
}

#[cfg(not(cellos_wide))]
pub fn map_display_framebuffer_user(_start: PhysAddr, _pages: usize) -> PagingResult<()> {
    Err(PageTableError::NotSupported)
}

#[cfg(target_arch = "x86_64")]
pub fn remap_range_user(start: PhysAddr, pages: usize) {
    let mut batch = Batch::new();
    let _ = crate::memory::frame::with(|allocator| {
        for i in 0..pages {
            let addr = start + (i * PAGE_SIZE);
            // Map VA == PA (identity) with user-rw flags.
            let _ = map_page(allocator, &mut batch, addr, addr, USER_RW_X86);
        }
    });
    let _ = batch.complete();
}

// ─── Bare-physical stubs ─────────────────────────────────────────────────────

#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn unmap_page(_batch: &mut Batch, _vaddr: VAddr) -> PagingResult<()> {
    Err(PageTableError::NotSupported)
}

#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn map_page(
    _allocator: &mut FrameAllocator,
    _batch: &mut Batch,
    _vaddr: VAddr,
    _paddr: PhysAddr,
    _flags: Flags,
) -> PagingResult<()> {
    Err(PageTableError::NotSupported)
}

#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn remap_range_user(_start: PhysAddr, _pages: usize) {}

/// Hardware translation observed at one virtual address.
///
/// The raw leaf entry retains architecture-specific access bits so rollback
/// probes can distinguish an absent translation from a mapping with altered
/// permissions.
#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranslationProbe {
    pub(crate) phys: PhysAddr,
    pub(crate) pte: u64,
}

/// Read the active page table's leaf entry without relying on VMA bookkeeping.
#[cfg(all(
    feature = "test-hooks",
    any(target_arch = "riscv64", target_arch = "aarch64")
))]
pub(crate) fn translation_probe(vaddr: VAddr) -> Option<TranslationProbe> {
    #[cfg(target_arch = "riscv64")]
    {
        let satp = crate::arch::page_probe::root_register();
        let root_phys = (satp & ((1 << 44) - 1)) << 12;
        let root = (root_phys != 0).then(|| unsafe { &*(root_phys as *const hal::PageTable) })?;
        let pte = root.leaf_entry(vaddr)? as u64;
        return Some(TranslationProbe {
            phys: (((pte >> 10) & 0x003F_FFFF_FFFF_FFFF) as usize) << 12 | (vaddr & 0xFFF),
            pte,
        });
    }
    #[cfg(target_arch = "aarch64")]
    {
        let ttbr0 = crate::arch::page_probe::root_register();
        let root = unsafe { &*((ttbr0 & !0xFFF) as *const hal::PageTable) };
        let pte = root.leaf_entry(vaddr)?;
        return Some(TranslationProbe {
            phys: ((pte & 0x0000_FFFF_FFFF_F000) as usize) | (vaddr & 0xFFF),
            pte,
        });
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(all(feature = "test-hooks", target_arch = "x86_64"))]
pub(crate) fn translation_probe(vaddr: VAddr) -> Option<TranslationProbe> {
    use hal::paging::PTE_ADDR_MASK;

    let root_phys = (*KERNEL_ROOT.lock())?;
    // SAFETY: the kernel's live PML4, walked read-only.
    let pte = unsafe { &*table_at(root_phys) }.leaf_entry(vaddr)?;
    Some(TranslationProbe {
        phys: ((pte & PTE_ADDR_MASK) as usize) | (vaddr & 0xFFF),
        pte,
    })
}

#[cfg(all(
    feature = "test-hooks",
    any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm")
))]
pub(crate) fn translation_probe(_vaddr: VAddr) -> Option<TranslationProbe> {
    None
}

// ─── virt_to_phys ────────────────────────────────────────────────────────────

/// Translate a virtual address to its physical address by walking the kernel
/// page table.
///
/// Bare-physical arches (riscv32, x86_32, arm32): VA == PA.
/// Paged arches read the page-table root from the architecture register.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn virt_to_phys(vaddr: VAddr) -> Option<PhysAddr> {
    use hal::traits::PageTableTrait;

    #[cfg(target_arch = "riscv64")]
    {
        let satp = crate::arch::page_probe::root_register();
        let root_ppn = satp & ((1 << 44) - 1);
        let root_phys = root_ppn << 12;
        if root_phys == 0 {
            return None;
        }
        // SAFETY: root_phys is the physical address of the active root PageTable.
        let root_table = unsafe { &*(root_phys as *const hal::PageTable) };
        return root_table.translate(vaddr);
    }
    #[cfg(target_arch = "aarch64")]
    {
        let ttbr0 = crate::arch::page_probe::root_register();
        let root_phys = ttbr0 & !0xFFF;
        if root_phys == 0 {
            return None;
        }
        let root_table = unsafe { &*(root_phys as *const hal::PageTable) };
        return root_table.translate(vaddr);
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(target_arch = "x86_64")]
pub fn virt_to_phys(vaddr: VAddr) -> Option<PhysAddr> {
    let root_guard = KERNEL_ROOT.lock();
    let root_phys = (*root_guard)?;
    // SAFETY: the kernel's live PML4, read under its lock. The HAL extracts
    // the frame with PTE_ADDR_MASK, never `!0xFFF`, which would keep NX.
    unsafe { &*table_at(root_phys) }.translate(vaddr)
}

/// Bare-physical identity translation.
#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn virt_to_phys(vaddr: VAddr) -> Option<PhysAddr> {
    Some(vaddr)
}

// ─── Page-fault handler (x86_64) ─────────────────────────────────────────────

/// HAL-visible entry point: called by the IDT error-code handler via the
/// shared `vi_handle_page_fault` declaration in `hal/traits/arch/src/kernel_abi.rs`.
///
/// Decision tree:
///   - Kernel-mode fault: panic — a true kernel bug.
///   - User-mode PROTECTION violation (error_code bit 0 = P set): the page is
///     mapped and the access simply is not allowed. Terminate the cell. Never
///     demand-page it: after W^X a cell writing its own `.text` lands here, and
///     re-installing the mapping would hand back the writable page the loader
///     deliberately took away.
///   - User-mode fault on a NOT-PRESENT page: look up the current task's VMA
///     list; if a region covers `va`, install the mapping and return so the
///     faulting instruction is retried.
///   - User-mode fault with no VMA match: terminate the cell, matching the
///     riscv64 / aarch64 trap handlers. Only when there is no cell context to
///     charge the fault to does this panic.
///
/// # Safety
/// Must only be called from the #PF exception handler while the faulting
/// context is suspended on the interrupt stack. The caller must NOT hold any
/// spinlock that this function might try to acquire (KERNEL_ROOT, SCHEDULER,
/// FRAME_ALLOCATOR).
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub extern "Rust" fn vi_handle_page_fault(va: usize, error_code: u64, rip: u64, cs: u64, rsp: u64) {
    use crate::task::SCHEDULER;

    log::warn!(
        "[#PF-debug] va={:#x} err={:#x} rip={:#x} cs={:#x} rsp={:#x}",
        va,
        error_code,
        rip,
        cs,
        rsp
    );
    // Bit 2 of the error code = U/S: fault originated from user mode.
    let user_fault = error_code & (1 << 2) != 0;

    if !user_fault {
        let mut callers: [u64; 6] = [0; 6];
        let mut n = 0;
        let page_end = (rsp | 0xFFF) + 1;
        let max_words = if rsp != 0 && rsp.is_multiple_of(8) {
            (((page_end - rsp) / 8) as usize).min(64)
        } else {
            0
        };
        for i in 0..max_words {
            if n >= callers.len() {
                break;
            }
            let q = unsafe { core::ptr::read_volatile((rsp as *const u64).add(i)) };
            if (0xffff_ffff_8000_0000..0xffff_ffff_9000_0000).contains(&q) {
                callers[n] = q;
                n += 1;
            }
        }
        panic!(
            "[#PF kernel] va={:#x} error_code={:#x} rip={:#x} cs={:#x} rsp={:#x} ret-candidates={:#x?} — kernel-mode fault (may be #GP: CR2 can be stale)",
            va, error_code, rip, cs, rsp, &callers[..n]
        );
    }

    // Bit 0 of the error code = P: the page WAS present, so this is a permission
    // violation, not a missing mapping. Demand paging must not run — it would
    // re-install the page with the VMA's flags and undo the loader's W^X pass,
    // turning "cell tried to write its own .text" into "cell may write its own
    // .text, once per fault". Terminate instead, as riscv64/aarch64 already do.
    if error_code & 1 != 0 {
        fault_kill_cell(va, error_code, rip, cs, "permission violation");
        return;
    }

    // Try to find the faulting VA in the current task's VMA list.
    // We identify the current task via the scheduler's running-task pointer.
    let result: Option<(PhysAddr, Flags)> = {
        let mut sched = SCHEDULER.lock();
        if let Some(s) = sched.as_mut() {
            // `current_task_mut()` returns the task currently marked Running
            // (the one whose exception context we are inside).
            s.current_task_mut().and_then(|task| {
                task.vma.find(va).map(|region| {
                    let offset = va - region.va_start;
                    let pa = region.pa_start + offset;
                    let flags = region.flags;
                    (pa, flags)
                })
            })
        } else {
            None
        }
    };

    match result {
        Some((paddr, flags)) => {
            // Install the mapping on demand. For ELF-backed segments the ELF loader
            // (Phase 04) sets pa_start to the physical frame it copied the segment
            // into; we just wire it up here. For demand-allocated regions (Stack/Heap)
            // the ELF loader leaves pa_start = 0 and the fault handler allocates.
            //
            // The VMA list is empty for ELF-backed cells today (segments are
            // mapped eagerly by the loader), so in practice only future
            // demand-allocated regions reach here.
            let mut batch = Batch::new();
            crate::memory::frame::with(|alloc| {
                let effective_pa = if paddr == 0 {
                    // Demand-allocate a new zeroed frame. Exempt from the
                    // quarantine cap: this dormant path panics on `None`, and
                    // the cap must never turn a quarantine into a panic.
                    alloc.allocate_frame_uncapped().unwrap_or_else(|| {
                        panic!("[#PF] OOM allocating demand page at va={:#x}", va)
                    })
                } else {
                    paddr
                };
                let _ = map_page(alloc, &mut batch, va & !0xFFF, effective_pa & !0xFFF, flags);
            })
            .unwrap_or_else(|| panic!("[#PF] frame allocator unavailable at va={:#x}", va));
            let _ = batch.complete();
        }
        None => {
            fault_kill_cell(va, error_code, rip, cs, "no VMA covers this address");
        }
    }
}

#[cfg(target_arch = "x86_64")]
const _: crate::hal::HandlePageFault = vi_handle_page_fault;

/// Terminate the cell that took an unserviceable user-mode #PF, or panic if the
/// fault cannot be attributed to one.
///
/// Brings x86_64 in line with the riscv64 (`rv64/trap.rs`) and aarch64
/// (`aarch64/trap.rs`) handlers, which already kill the faulting cell and keep
/// the kernel running. Panicking here would turn every cell-level memory bug —
/// including the W^X violation this phase deliberately creates — into a
/// whole-system halt.
///
/// A non-zero cell id is required: a user-mode fault with no cell context means
/// the scheduler state is inconsistent, and killing "the current cell" would
/// destroy an innocent task or nothing at all.
///
/// Does not return when a cell is killed —
/// `terminate_current_cell_on_user_trap_fault` ends in `yield_cpu()`, which
/// switches to another task.
#[cfg(target_arch = "x86_64")]
fn fault_kill_cell(va: usize, error_code: u64, rip: u64, cs: u64, why: &str) {
    let cell_id = crate::sched::hart_local::current_cell_id();
    if cell_id == 0 {
        panic!(
            "[#PF user] va={:#x} error_code={:#x} rip={:#x} cs={:#x} — {} and no cell owns the fault",
            va, error_code, rip, cs, why
        );
    }
    log::error!(
        "[#PF user] va={:#x} error_code={:#x} rip={:#x} cs={:#x} — {}",
        va,
        error_code,
        rip,
        cs,
        why
    );
    // The #PF dispatcher established error-code U/S before entering this
    // helper, so this is the x86 user-trap provenance required by the
    // deferred retirement funnel.
    crate::task::terminate_current_cell_on_user_trap_fault(error_code as usize, rip as usize, va);
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    /// The batched MMIO flush covers exactly the pages the loop mapped: the
    /// window is rounded out to whole pages and is empty only for size 0.
    #[test]
    fn mmio_window_covers_every_mapped_page_once() {
        // 256 MiB ECAM: 65,536 pages, one flush.
        let (start, end) = mmio_page_window(0xb000_0000, 0x1000_0000);
        assert_eq!((start, (end - start) / PAGE_SIZE), (0xb000_0000, 65_536));
        // Unaligned start and size round out to both enclosing pages.
        assert_eq!(mmio_page_window(0xfebf_1004, 8), (0xfebf_1000, 0xfebf_2000));
        assert_eq!(mmio_page_window(0xfebf_1ffc, 8), (0xfebf_1000, 0xfebf_3000));
        // Size 0 maps nothing and flushes nothing.
        let (start, end) = mmio_page_window(0xfebf_1000, 0);
        assert_eq!(start, end);
    }
}
