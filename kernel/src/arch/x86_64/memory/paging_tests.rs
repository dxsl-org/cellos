//! PTE-equivalence harness for the x86_64 shared kernel root.
//!
//! Each call site that edits `KERNEL_ROOT` is replayed twice, on two scratch
//! roots: once through the raw path it used before (`walk_create` /
//! `walk_read` with the kernel's raw `pte_flags_*` / `pte_bits_from_flags`
//! composition, the old bodies without their lock and TLB shootdown), once
//! through the HAL page-table trait with the flags the kernel passes now.
//! After every step both roots hold the same entries at every level:
//! intermediate entries compare by their flag bits and by the allocation order
//! of the table they name, every other entry bit for bit.
//!
//! Host-run: the HHDM offset is 0, so a boxed table's address is its frame.

use crate::memory::paging::{self, Flags};
use alloc::boxed::Box;
use alloc::vec::Vec;
use hal::paging::{PTE_ADDR_MASK, PTE_NX, PTE_PCD, PTE_PRESENT, PTE_USER, PTE_WRITABLE};
use hal::PageTable;

const PS: u64 = 1 << 7;
const PAGE: usize = 4096;
/// What every intermediate entry of the shared root carries besides its frame.
const TABLE_BITS: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER;

/// One scratch root and the tables allocated under it, in allocation order.
struct Scratch {
    root: Box<PageTable>,
    tables: Vec<Box<PageTable>>,
    /// Allocations left before the allocator reports out-of-memory.
    budget: usize,
}

impl Scratch {
    fn new() -> Self {
        Self {
            root: Box::new(PageTable::zero()),
            tables: Vec::new(),
            budget: usize::MAX,
        }
    }

    fn root_address(&self) -> usize {
        self.root.as_ref() as *const PageTable as usize
    }

    /// The root and an allocator that records every table it hands out.
    fn parts(&mut self) -> (&mut PageTable, impl FnMut() -> Option<usize> + '_) {
        let Self {
            root,
            tables,
            budget,
        } = self;
        let allocate = move || {
            *budget = budget.checked_sub(1)?;
            let mut table = Box::new(PageTable::zero());
            let address = table.as_mut() as *mut PageTable as usize;
            tables.push(table);
            Some(address)
        };
        (root.as_mut(), allocate)
    }

    fn allocation_index(&self, frame: u64) -> usize {
        self.tables
            .iter()
            .position(|table| table.as_ref() as *const PageTable as u64 == frame)
            .expect("an intermediate entry names a table this root allocated")
    }
}

fn entry(table: usize, index: usize) -> u64 {
    // SAFETY: `table` is a live boxed scratch table; `index` < 512.
    unsafe { core::ptr::read_volatile((table as *const u64).add(index)) }
}

/// Compare both roots at every level. Returns `(slots, non-zero entries)`.
fn same_tree(a: &Scratch, b: &Scratch, step: &str) -> (usize, usize) {
    fn level(
        a: &Scratch,
        b: &Scratch,
        tables: (usize, usize),
        depth: u32,
        step: &str,
        count: &mut (usize, usize),
    ) {
        for index in 0..512 {
            let (ea, eb) = (entry(tables.0, index), entry(tables.1, index));
            count.0 += 1;
            if ea != 0 || eb != 0 {
                count.1 += 1;
            }
            let is_table = depth < 3 && ea & PTE_PRESENT != 0 && ea & PS == 0;
            if !is_table {
                assert_eq!(ea, eb, "{step}: level {depth} index {index}");
                continue;
            }
            assert_eq!(
                ea & !PTE_ADDR_MASK,
                eb & !PTE_ADDR_MASK,
                "{step}: level {depth} index {index} table flags"
            );
            let (na, nb) = (ea & PTE_ADDR_MASK, eb & PTE_ADDR_MASK);
            assert_eq!(
                a.allocation_index(na),
                b.allocation_index(nb),
                "{step}: level {depth} index {index} table order"
            );
            level(a, b, (na as usize, nb as usize), depth + 1, step, count);
        }
    }
    let mut count = (0, 0);
    level(
        a,
        b,
        (a.root_address(), b.root_address()),
        0,
        step,
        &mut count,
    );
    count
}

/// The raw path, as the kernel ran it before the trait carried the shared root.
mod old {
    use super::{Flags, Scratch};
    use hal::paging::{
        walk_create, walk_read, PTE_ADDR_MASK, PTE_NX, PTE_PCD, PTE_PRESENT, PTE_PWT, PTE_USER,
        PTE_WRITABLE,
    };

    fn pml4(s: &mut Scratch) -> *mut u64 {
        s.root.as_mut() as *mut hal::PageTable as *mut u64
    }

    /// `map_page_x86_unflushed` (and the `init_kernel_paging_x86` MMIO loop):
    /// walk-or-create, then write `paddr | flags`.
    pub fn map(s: &mut Scratch, vaddr: usize, paddr: usize, flags: u64) -> Result<(), ()> {
        let pml4_virt = pml4(s);
        let (_, mut alloc_fn) = s.parts();
        let pte_ptr = unsafe { walk_create(pml4_virt, vaddr, &mut alloc_fn) }.ok_or(())?;
        unsafe {
            core::ptr::write_volatile(pte_ptr, paddr as u64 | flags);
        }
        Ok(())
    }

    /// `page_protect::pte_bits_from_flags`.
    pub fn pte_bits_from_flags(flags: Flags) -> u64 {
        let bits = flags.bits();
        let mut pte = PTE_PRESENT;
        if bits & hal::PageFlags::WRITE != 0 {
            pte |= PTE_WRITABLE;
        }
        if bits & hal::PageFlags::USER != 0 {
            pte |= PTE_USER;
        }
        if bits & hal::PageFlags::EXECUTE == 0 {
            pte |= PTE_NX;
        }
        pte
    }

    /// `page_protect::rewrite_leaf_x86`.
    pub fn protect(s: &mut Scratch, page_va: usize, new_flags: Flags) -> Result<(), ()> {
        let root = pml4(s);
        let existing = unsafe { walk_read(root as *const u64, page_va) }.ok_or(())?;
        if existing & PTE_PRESENT == 0 {
            return Err(());
        }
        let cache_bits = existing & (PTE_PWT | PTE_PCD);
        let new_pte = (existing & PTE_ADDR_MASK) | cache_bits | pte_bits_from_flags(new_flags);
        let mut no_alloc = || None::<usize>;
        let pte_ptr = unsafe { walk_create(root, page_va, &mut no_alloc) }.ok_or(())?;
        unsafe {
            core::ptr::write_volatile(pte_ptr, new_pte);
        }
        Ok(())
    }

    /// `unmap_page_x86` without its shootdown: whether a leaf was cleared.
    pub fn unmap(s: &mut Scratch, vaddr: usize) -> bool {
        let root = pml4(s);
        let existing = unsafe { walk_read(root as *const u64, vaddr) };
        if let Some(pte) = existing {
            if pte & PTE_PRESENT != 0 {
                let mut noop_alloc = || None::<usize>;
                if let Some(pte_ptr) = unsafe { walk_create(root, vaddr, &mut noop_alloc) } {
                    unsafe {
                        core::ptr::write_volatile(pte_ptr, 0u64);
                    }
                    return true;
                }
            }
        }
        false
    }

    /// The shootdown fixture's in-place re-point: an existing leaf rewritten
    /// through `walk_create` with no allocator.
    pub fn repoint(s: &mut Scratch, vaddr: usize, paddr: usize, flags: u64) {
        let root = pml4(s);
        let mut no_allocate = || None;
        let leaf = unsafe { walk_create(root, vaddr, &mut no_allocate) }.expect("fixture leaf");
        unsafe {
            core::ptr::write_volatile(leaf, paddr as u64 | flags);
        }
    }
}

/// The trait path the kernel takes now.
mod new {
    use super::{Flags, Scratch};
    use crate::memory::paging::SAS_TABLES;
    use hal::PageTableTrait;

    pub fn map(s: &mut Scratch, vaddr: usize, paddr: usize, flags: Flags) -> Result<(), ()> {
        let (root, mut allocate) = s.parts();
        root.map_with(vaddr, paddr, flags, SAS_TABLES, &mut allocate)
            .map_err(drop)
    }

    pub fn protect(s: &mut Scratch, page_va: usize, flags: Flags) -> Result<(), ()> {
        s.root.protect(page_va, flags).map_err(drop)
    }

    pub fn unmap(s: &mut Scratch, vaddr: usize) -> bool {
        s.root.leaf_entry(vaddr).is_some() && s.root.unmap(vaddr).is_ok()
    }

    pub fn repoint(s: &mut Scratch, vaddr: usize, paddr: usize, flags: Flags) {
        s.root
            .map_with(vaddr, paddr, flags, SAS_TABLES, &mut || None)
            .expect("fixture leaf");
    }
}

struct Pair {
    old: Scratch,
    new: Scratch,
    steps: usize,
    slots: usize,
    entries: usize,
}

impl Pair {
    fn new() -> Self {
        Self {
            old: Scratch::new(),
            new: Scratch::new(),
            steps: 0,
            slots: 0,
            entries: 0,
        }
    }

    fn check(&mut self, step: &str) {
        let (slots, entries) = same_tree(&self.old, &self.new, step);
        self.steps += 1;
        self.slots += slots;
        self.entries += entries;
    }

    /// One mapping through both paths; both succeed or both fail.
    fn map(&mut self, step: &str, va: usize, pa: usize, raw: u64, flags: Flags) {
        let old = old::map(&mut self.old, va, pa, raw);
        let new = new::map(&mut self.new, va, pa, flags);
        assert_eq!(old, new, "{step}: {va:#x} result");
        self.check(step);
    }

    fn map_range(&mut self, step: &str, start: usize, pages: usize, raw: u64, flags: Flags) {
        for page in 0..pages {
            let va = start + page * PAGE;
            self.map(step, va, va, raw, flags);
        }
    }

    /// One map with the old-path flags derived the way `map_page` derived
    /// them (`pte_bits_from_flags`).
    fn map_flags(&mut self, step: &str, va: usize, pa: usize, flags: Flags) {
        self.map(step, va, pa, old::pte_bits_from_flags(flags), flags);
    }

    fn protect(&mut self, step: &str, va: usize, flags: Flags) {
        let old = old::protect(&mut self.old, va, flags);
        let new = new::protect(&mut self.new, va, flags);
        assert_eq!(old, new, "{step}: {va:#x} result");
        self.check(step);
    }

    fn unmap(&mut self, step: &str, va: usize) -> bool {
        let old = old::unmap(&mut self.old, va);
        let new = new::unmap(&mut self.new, va);
        assert_eq!(old, new, "{step}: {va:#x} cleared");
        self.check(step);
        new
    }

    fn leaf(&self, va: usize) -> Option<u64> {
        self.new.root.leaf_entry(va)
    }
}

fn flags(bits: usize) -> Flags {
    Flags::from_bits(bits)
}

#[test]
fn shared_root_edits_write_the_raw_paths_entries_at_every_level() {
    use hal::paging::{pte_flags_mmio, pte_flags_mmio_user, pte_flags_user_rw};

    let mut pair = Pair::new();

    // init_kernel_paging_x86: IOAPIC, HPET, LAPIC, then an MCFG window.
    for (va, pages) in [
        (0xFEC0_0000, 1),
        (0xFED0_0000, 1),
        (0xFEE0_0000, 1),
        (0xB000_0000, 16),
    ] {
        pair.map_range(
            "boot MMIO",
            va,
            pages,
            pte_flags_mmio(),
            paging::MMIO_KERNEL_X86,
        );
    }
    assert_eq!(
        pair.leaf(0xFEC0_0000),
        Some(0xFEC0_0000 | PTE_PRESENT | PTE_WRITABLE | PTE_PCD)
    );

    // map_mmio_x86: a BAR window, then the same pages again (overwrite).
    for _ in 0..2 {
        pair.map_range(
            "BAR MMIO",
            0xFEBF_0000,
            4,
            pte_flags_mmio(),
            paging::MMIO_KERNEL_X86,
        );
    }
    // map_mmio_user_x86: a RequestMmio grant over mapped and fresh pages.
    pair.map_range(
        "MMIO user",
        0xFEBF_2000,
        4,
        pte_flags_mmio_user(),
        paging::MMIO_USER_X86,
    );
    assert_eq!(
        pair.leaf(0xFEBF_3000),
        Some(0xFEBF_3000 | PTE_PRESENT | PTE_WRITABLE | PTE_USER | PTE_PCD | PTE_NX)
    );
    // clear_mmio_user: the revoke keeps the frame and PCD, drops USER.
    for page in 0..4 {
        pair.protect(
            "MMIO revoke",
            0xFEBF_2000 + page * PAGE,
            paging::mmio_kernel_flags(),
        );
    }
    assert_eq!(
        pair.leaf(0xFEBF_3000),
        Some(0xFEBF_3000 | PTE_PRESENT | PTE_WRITABLE | PTE_PCD | PTE_NX)
    );

    // map_page: kernel and loader flag sets, a supervisor leaf first and a
    // user leaf in the same page table after it, and a high-index user leaf.
    use crate::loader::wx::page_flags;
    let kernel_rwx = flags(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );
    let kernel_rw = flags(Flags::VALID | Flags::READ | Flags::WRITE);
    let kernel_rx = flags(Flags::VALID | Flags::READ | Flags::EXECUTE);
    let kernel_ro = flags(Flags::VALID | Flags::READ);
    let user_rw = page_flags(true, false, false);
    let user_rx = page_flags(false, true, false);
    let user_ro = page_flags(false, false, false);
    let user_load = flags(user_rx.bits() | Flags::WRITE);
    let mut frame = 0x0200_0000;
    for (va, f) in [
        (0x4020_0000, kernel_rwx),
        (0x4020_1000, user_rw),
        (0x4020_2000, kernel_rw),
        (0x4020_3000, user_rx),
        (0x4020_4000, kernel_rx),
        (0x4020_5000, user_ro),
        (0x4020_6000, kernel_ro),
        (0x4020_7000, user_load),
        (0x1_B000_0000, user_rx),
        (
            (255 << 39) | (511 << 30) | (511 << 21) | (511 << 12),
            user_ro,
        ),
    ] {
        pair.map_flags("map_page", va, frame, f);
        frame += PAGE;
    }
    assert_eq!(
        pair.leaf(0x4020_1000),
        Some(0x0200_1000 | PTE_PRESENT | PTE_WRITABLE | PTE_USER | PTE_NX)
    );

    // protect_page / protect_pages: the loader's W^X lowering, a protect
    // without VALID (P stays set on x86_64), a page that is not mapped.
    pair.protect("W^X", 0x4020_7000, user_rx);
    pair.protect("W^X", 0x4020_1000, user_ro);
    pair.protect("W^X", 0x4020_2000, kernel_ro);
    pair.protect("protect without VALID", 0x4020_5000, flags(Flags::READ));
    pair.protect("protect unmapped page", 0x4020_8000, user_ro);
    pair.protect("protect unmapped table", 0x7_0000_0000, user_ro);
    assert_eq!(
        pair.leaf(0x4020_7000),
        Some(0x0200_7000 | PTE_PRESENT | PTE_USER)
    );

    // map_display_framebuffer_user and remap_range_user.
    pair.map_range(
        "framebuffer",
        0x8000_0000,
        4,
        pte_flags_user_rw(),
        paging::USER_RW_X86,
    );
    pair.map_range(
        "remap_range_user",
        0x0700_0000,
        4,
        pte_flags_user_rw(),
        paging::USER_RW_X86,
    );

    // The BIOS area: map_bios_area's 4 KiB leaf (kernel RW, NX) through
    // map_page's kernel-RW flags.
    for page in 0x9F..=0xFF_usize {
        pair.map_flags("BIOS area", page * PAGE, page * PAGE, kernel_rw);
        assert_eq!(
            pair.leaf(page * PAGE),
            Some((page as u64 * 0x1000) | PTE_PRESENT | PTE_WRITABLE | PTE_NX)
        );
    }

    // The remote-shootdown fixture: a global leaf, re-pointed in place twice.
    let probe = 0x0600_0000;
    let global_raw = PTE_PRESENT | PTE_WRITABLE | PTE_NX | (1 << 8);
    let global = flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::GLOBAL);
    pair.map("shootdown fixture", probe, 0x0300_0000, global_raw, global);
    for target in [0x0300_1000, 0x0300_0000] {
        old::repoint(&mut pair.old, probe, target, global_raw);
        new::repoint(&mut pair.new, probe, target, global);
        pair.check("shootdown fixture re-point");
    }

    // unmap_page: a present leaf, the same leaf again (idempotent), a page
    // never mapped, a path with no tables.
    assert!(pair.unmap("unmap", probe));
    assert!(!pair.unmap("unmap again", probe));
    assert!(!pair.unmap("unmap never mapped", 0x4020_9000));
    assert!(!pair.unmap("unmap no tables", 0x7_0000_0000));
    assert!(pair.unmap("unmap", 0x4020_3000));

    // A failed walk publishes the same intermediate tables on both paths
    // (the caller prunes them).
    pair.old.budget = 1;
    pair.new.budget = 1;
    pair.map_flags("out of memory", 0x60_0000_0000, frame, user_rw);

    // Every intermediate entry of the shared root is P|RW|US.
    let mut table = pair.new.root.as_ref() as *const PageTable as usize;
    for shift in [39, 30, 21] {
        let e = entry(table, (0x4020_1000 >> shift) & 0x1FF);
        assert_eq!(e & !PTE_ADDR_MASK, TABLE_BITS);
        table = (e & PTE_ADDR_MASK) as usize;
    }

    std::println!(
        "x86 shared-root PTE equivalence: {} steps, {} slots compared, {} non-zero entries compared",
        pair.steps,
        pair.slots,
        pair.entries
    );
}
