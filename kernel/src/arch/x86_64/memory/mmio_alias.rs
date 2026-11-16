// SPDX-License-Identifier: MPL-2.0
//! The x86_64 kernel-only MMIO alias window, compiled as `memory::paging`'s
//! `mmio_alias` child module (`#[path]`), so it keeps using paging's private
//! mapping helpers.
//!
//! A `RequestMmio` grant turns the identity (VA == PA) leaves of its window
//! USER so the owning cell can reach them, and under SMAP a supervisor access
//! through a USER leaf faults. The kernel therefore reaches MMIO a cell may be
//! granted (the ECAM window, pieces of PCIe BARs) only through a second map of
//! the same frames in this window: supervisor, NX and `PCD`, the cache
//! attribute of the identity MMIO leaves. Nothing here is USER, so cells see
//! none of it, and their identity leaves stay as their grant left them. MMIO
//! the kernel drives through the identity map instead (LAPIC, I/O APICs, HPET,
//! VT-d units) is recorded, and no grant may reach it.

use super::{
    map_page_unflushed, mmio_page_window, Batch, Flags, PageTable, PageTableError, PageTableTrait,
    PagingResult, PAGE_SIZE, SAS_TABLES,
};
use crate::sync::Spinlock;
use core::sync::atomic::{AtomicUsize, Ordering};

/// First VA of the alias window: PML4 slot 511, PDPT slot 0. Limine keeps the
/// kernel image in the top 2 GiB of slot 511 (PDPT slots 510-511). Every root
/// (kernel, AP, domain) copies PML4 slots 256..512, so all of them share slot
/// 511's PDPT and see each table created below it, whenever it is created.
pub(crate) const MMIO_ALIAS_BASE: usize = 0xFFFF_FF80_0000_0000;

/// End of the alias window: one PDPT slot (1 GiB).
const MMIO_ALIAS_END: usize = MMIO_ALIAS_BASE + (1 << 30);

/// The ECAM alias ([`map_ecam_alias`]) takes the first 256 MiB of the window,
/// the largest MCFG allocation (256 buses of 1 MiB); [`kernel_mmio_alias`]
/// hands out the rest.
const ECAM_ALIAS_SPAN: usize = 256 << 20;

/// Alias leaves: supervisor, read/write, NX, `PCD` — the identity MMIO leaves'
/// cache attribute (`MMIO_KERNEL_X86`) without their execute right.
pub(crate) const MMIO_ALIAS_X86: Flags =
    Flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::UNCACHED);

/// Aliases [`kernel_mmio_alias`] can hold: one per MSI-X table and virtio
/// common-configuration page the kernel touches, so a few per PCI function.
const MMIO_ALIAS_SLOTS: usize = 64;

/// Kernel-driven identity windows [`KernelMmio::own`] can hold: the boot
/// LAPIC, I/O APIC and HPET pages, every further I/O APIC and every VT-d unit.
const KERNEL_WINDOW_SLOTS: usize = 32;

/// Kernel VA of the ECAM allocation base; 0 until [`map_ecam_alias`] mapped
/// the alias (no admitted ECAM window).
static ECAM_ALIAS: AtomicUsize = AtomicUsize::new(0);

/// The kernel's VA of the admitted ECAM window's base, or 0 when there is
/// none: every kernel config-space access goes through it, never through the
/// identity window the Platform Cell's claim makes USER.
pub fn ecam_kernel_alias() -> usize {
    ECAM_ALIAS.load(Ordering::Acquire)
}

/// One alias of physical pages `[start, end)` at `va`; usable once `mapped`.
#[derive(Clone, Copy)]
struct MmioAlias {
    start: usize,
    end: usize,
    va: usize,
    mapped: bool,
}

/// What the kernel knows of the MMIO it reaches. Page-aligned physical ranges
/// throughout.
struct KernelMmio {
    /// First VA [`Self::reserve`] has not handed out. It only grows: no alias
    /// VA is ever reused, so a new alias never meets a stale translation.
    next: usize,
    aliases: [Option<MmioAlias>; MMIO_ALIAS_SLOTS],
    /// Windows the kernel drives through the identity map.
    owned: [Option<(usize, usize)>; KERNEL_WINDOW_SLOTS],
}

impl KernelMmio {
    const fn new() -> Self {
        Self {
            next: MMIO_ALIAS_BASE + ECAM_ALIAS_SPAN,
            aliases: [None; MMIO_ALIAS_SLOTS],
            owned: [None; KERNEL_WINDOW_SLOTS],
        }
    }

    /// VA of page `start` in a mapped alias covering `[start, end)`.
    fn alias_of(&self, start: usize, end: usize) -> Option<usize> {
        self.aliases
            .iter()
            .flatten()
            .find(|alias| alias.mapped && alias.start <= start && end <= alias.end)
            .map(|alias| alias.va + (start - alias.start))
    }

    /// A slot and fresh VA for `[start, end)`, not yet usable: `(slot, va)`,
    /// or `None` when no slot or not enough VA is left.
    fn reserve(&mut self, start: usize, end: usize) -> Option<(usize, usize)> {
        let slot = self.aliases.iter().position(Option::is_none)?;
        let va = self.next;
        if MMIO_ALIAS_END - va < end - start {
            return None;
        }
        self.next = va + (end - start);
        self.aliases[slot] = Some(MmioAlias {
            start,
            end,
            va,
            mapped: false,
        });
        Some((slot, va))
    }

    /// End a reservation: usable once every page is mapped, otherwise the slot
    /// is freed (its VA stays spent).
    fn settle(&mut self, slot: usize, mapped: bool) {
        match &mut self.aliases[slot] {
            Some(alias) if mapped => alias.mapped = true,
            entry => *entry = None,
        }
    }

    /// Record `[start, end)` as driven by the kernel through the identity map;
    /// `false` when it is not covered already and no slot is left.
    fn own(&mut self, start: usize, end: usize) -> bool {
        if self
            .owned
            .iter()
            .flatten()
            .any(|&(s, e)| s <= start && end <= e)
        {
            return true;
        }
        match self.owned.iter_mut().find(|window| window.is_none()) {
            Some(window) => {
                *window = Some((start, end));
                true
            }
            None => false,
        }
    }

    /// Whether `[start, end)` reaches a kernel-driven window.
    fn reaches_owned(&self, start: usize, end: usize) -> bool {
        self.owned
            .iter()
            .flatten()
            .any(|&(s, e)| start < e && s < end)
    }
}

/// Never held while logging, mapping or flushing: each user takes it for one
/// lookup or update and drops it.
static KERNEL_MMIO: Spinlock<KernelMmio> =
    Spinlock::ranked(KernelMmio::new(), crate::sync::rank::KERNEL_MMIO);

/// Map the kernel's alias of the ECAM allocation `[ecam_base, ecam_base +
/// ecam_len)` into `pml4`, the kernel root `init_kernel_paging_x86` is
/// building, at the start of the window; `alloc(va)` gives each new table.
/// `Err(InvalidAddress)`, logged: the allocation is larger than the ECAM span,
/// or the window is already mapped (Limine put something in slot 511 below the
/// kernel image). `Err(OutOfMemory)`: a page table could not be allocated.
pub(super) fn map_ecam_alias(
    pml4: &mut PageTable,
    ecam_base: usize,
    ecam_len: usize,
    alloc: &mut dyn FnMut(usize) -> Option<usize>,
) -> PagingResult<()> {
    let (start, end) = mmio_page_window(ecam_base, ecam_len);
    let window_free = pml4.translate(MMIO_ALIAS_BASE).is_none()
        && pml4.translate(MMIO_ALIAS_END - PAGE_SIZE).is_none();
    if end - start > ECAM_ALIAS_SPAN || !window_free {
        log::error!(
            "[paging] ECAM {ecam_base:#x} (+{ecam_len:#x}): no kernel alias (window {}, span {:#x} of {ECAM_ALIAS_SPAN:#x})",
            if window_free { "free" } else { "already mapped" },
            end - start
        );
        return Err(PageTableError::InvalidAddress);
    }
    let mut offset = 0;
    while offset < end - start {
        let va = MMIO_ALIAS_BASE + offset;
        pml4.map_with(va, start + offset, MMIO_ALIAS_X86, SAS_TABLES, &mut || {
            alloc(va)
        })
        .map_err(|_| PageTableError::OutOfMemory)?;
        offset += PAGE_SIZE;
    }
    ECAM_ALIAS.store(MMIO_ALIAS_BASE + (ecam_base - start), Ordering::Release);
    Ok(())
}

/// Record `[start, end)` (page-aligned) as MMIO the kernel drives through its
/// identity map, which no `RequestMmio` grant may then reach
/// ([`kernel_mmio_refused`]); `false` when no slot is left (nothing recorded).
pub(super) fn own_kernel_window(start: usize, end: usize) -> bool {
    KERNEL_MMIO.lock().own(start, end)
}

/// The kernel's VA of physical `[phys, phys + size)` in the alias window, for
/// MMIO a cell may be granted (pieces of a PCIe BAR): the kernel never touches
/// such MMIO through its identity leaves. The first call maps the enclosing
/// pages there (supervisor, NX, `PCD`); later calls for those pages, or for a
/// range inside them, reuse that alias. Two racing first calls may each make
/// one; both are valid.
///
/// `Err(OutOfMemory)`: no slot or VA is left in the window, or a page table
/// could not be allocated; logged, and nothing usable was made.
/// `Err(InvalidAddress)`: the range wraps. On `Err` the caller must not touch
/// the range: it fails that device closed. Maps and flushes with no lock of
/// its own held, so call it before taking a lock a CPU may spin on during a
/// TLB shootdown.
///
/// Panics if the frame allocator is not yet initialised.
#[must_use = "a refused alias must not be accessed"]
pub fn kernel_mmio_alias(phys: usize, size: usize) -> PagingResult<usize> {
    let page_mask = PAGE_SIZE - 1;
    let Some(end) = phys
        .checked_add(size.max(1))
        .and_then(|end| end.checked_add(page_mask))
        .map(|end| end & !page_mask)
    else {
        return Err(PageTableError::InvalidAddress);
    };
    let start = phys & !page_mask;
    let reserved = {
        let mut mmio = KERNEL_MMIO.lock();
        if let Some(va) = mmio.alias_of(start, end) {
            return Ok(va + (phys - start));
        }
        mmio.reserve(start, end)
    };
    let Some((slot, va)) = reserved else {
        log::error!(
            "[paging] kernel MMIO alias of {phys:#x} (+{size:#x}) refused: the alias window is full"
        );
        return Err(PageTableError::OutOfMemory);
    };
    let mut batch = Batch::new();
    let mapped = crate::memory::frame::with(|allocator| {
        let mut offset = 0;
        while offset < end - start {
            map_page_unflushed(
                allocator,
                &mut batch,
                va + offset,
                start + offset,
                MMIO_ALIAS_X86,
            )?;
            offset += PAGE_SIZE;
        }
        Ok(())
    })
    .expect("kernel_mmio_alias: frame allocator not ready");
    // The VA is fresh, so the batch holds only tables the walk created.
    let _ = batch.complete();
    KERNEL_MMIO.lock().settle(slot, mapped.is_ok());
    if let Err(error) = mapped {
        log::error!("[paging] kernel MMIO alias of {phys:#x} (+{size:#x}) not mapped ({error:?})");
        return Err(error);
    }
    Ok(va + (phys - start))
}

/// Refuse a `RequestMmio` grant of `[base, base + len)` that reaches a page
/// the kernel drives through its identity map (the boot LAPIC, I/O APIC and
/// HPET pages, and every `map_mmio_x86` window): the grant would turn those
/// leaves USER, and the kernel's next access through them would fault under
/// SMAP. Logs the refusal; `true` means refused.
pub(crate) fn kernel_mmio_refused(base: usize, len: usize) -> bool {
    let page_mask = PAGE_SIZE - 1;
    let start = base & !page_mask;
    let end = base.saturating_add(len).saturating_add(page_mask) & !page_mask;
    let refused = KERNEL_MMIO.lock().reaches_owned(start, end);
    if refused {
        log::error!(
            "[paging] RequestMmio: {base:#x} (+{len:#x}) refused: it reaches MMIO the kernel drives (LAPIC, I/O APIC, HPET or VT-d)"
        );
    }
    refused
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The alias window hands out fresh VA above the ECAM span, reuses a
    /// mapped alias for the same or an inner range, never reuses VA, and
    /// refuses once no slot or VA is left.
    #[test]
    fn kernel_mmio_aliases_are_reused_and_bounded() {
        let mut mmio = KernelMmio::new();
        let (slot, va) = mmio.reserve(0xFEBF_1000, 0xFEBF_3000).expect("first alias");
        assert_eq!(va, MMIO_ALIAS_BASE + ECAM_ALIAS_SPAN);
        // Not usable until every page is mapped.
        assert_eq!(mmio.alias_of(0xFEBF_1000, 0xFEBF_3000), None);
        mmio.settle(slot, true);
        assert_eq!(mmio.alias_of(0xFEBF_1000, 0xFEBF_3000), Some(va));
        assert_eq!(
            mmio.alias_of(0xFEBF_2000, 0xFEBF_3000),
            Some(va + PAGE_SIZE)
        );
        assert_eq!(mmio.alias_of(0xFEBF_2000, 0xFEBF_4000), None);
        // A failed map frees the slot but not its VA.
        let (failed, failed_va) = mmio.reserve(0xFEBF_8000, 0xFEBF_A000).expect("second");
        assert_eq!(failed_va, va + 2 * PAGE_SIZE);
        mmio.settle(failed, false);
        assert_eq!(mmio.alias_of(0xFEBF_8000, 0xFEBF_9000), None);
        let (_, next_va) = mmio.reserve(0xFEBF_8000, 0xFEBF_9000).expect("third");
        assert_eq!(next_va, failed_va + 2 * PAGE_SIZE);
        // Slots run out.
        let mut mmio = KernelMmio::new();
        for page in 0..MMIO_ALIAS_SLOTS {
            let start = 0xC000_0000 + page * PAGE_SIZE;
            assert!(mmio.reserve(start, start + PAGE_SIZE).is_some());
        }
        assert_eq!(mmio.reserve(0xD000_0000, 0xD000_1000), None);
        // VA runs out: the window above the ECAM span holds 768 MiB.
        let mut mmio = KernelMmio::new();
        let rest = MMIO_ALIAS_END - (MMIO_ALIAS_BASE + ECAM_ALIAS_SPAN);
        assert_eq!(mmio.reserve(0, rest + PAGE_SIZE), None);
        assert!(mmio.reserve(0, rest).is_some());
        assert_eq!(mmio.reserve(0x1000, 0x2000), None);
    }

    /// A kernel-driven window refuses every grant that reaches one of its
    /// pages, and recording a window already covered takes no slot.
    #[test]
    fn kernel_driven_windows_refuse_overlapping_grants() {
        let mut mmio = KernelMmio::new();
        assert!(mmio.own(0xFEE0_0000, 0xFEE0_1000));
        assert!(mmio.reaches_owned(0xFEE0_0000, 0xFEE0_1000));
        assert!(mmio.reaches_owned(0xFED0_0000, 0xFEF0_0000));
        assert!(!mmio.reaches_owned(0xFEDF_F000, 0xFEE0_0000));
        assert!(!mmio.reaches_owned(0xFEE0_1000, 0xFEE0_2000));
        for unit in 0..KERNEL_WINDOW_SLOTS - 1 {
            let start = 0xFED9_0000 + unit * PAGE_SIZE;
            assert!(mmio.own(start, start + PAGE_SIZE));
        }
        assert!(mmio.own(0xFEE0_0000, 0xFEE0_1000), "already covered");
        assert!(!mmio.own(0xFEC0_0000, 0xFEC0_1000), "no slot left");
    }
}
