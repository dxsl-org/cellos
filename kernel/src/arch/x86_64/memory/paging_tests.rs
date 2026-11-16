//! The x86_64 shared kernel root's entries, call site by call site.
//!
//! Every call site that edits `KERNEL_ROOT` replays here on a scratch root
//! through the HAL page-table trait with the flags the kernel passes, and each
//! leaf must be the exact word the raw `walk_create` path wrote before the
//! trait carried the shared root; every intermediate entry must be `P|RW|US`,
//! as `walk_create` wrote it. (Phase 14 step 5 compared both paths entry by
//! entry at every level before the raw path was deleted.)
//!
//! Host-run: the HHDM offset is 0, so a boxed table's address is its frame.

use crate::memory::paging::{self, Flags, SAS_TABLES};
use alloc::boxed::Box;
use alloc::vec::Vec;
use hal::paging::{PTE_ADDR_MASK, PTE_NX, PTE_PCD, PTE_PRESENT, PTE_USER, PTE_WRITABLE};
use hal::{PageTable, PageTableTrait};

const PS: u64 = 1 << 7;
const PAGE: usize = 4096;
const P: u64 = PTE_PRESENT;
const RW: u64 = PTE_WRITABLE;
const US: u64 = PTE_USER;
const PCD: u64 = PTE_PCD;
const NX: u64 = PTE_NX;
const G: u64 = 1 << 8;
/// What every intermediate entry of the shared root carries besides its frame.
const TABLE_BITS: u64 = P | RW | US;

/// One scratch root and the tables allocated under it.
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

    /// `map_page` / the boot MMIO loop: one leaf, shared-root table policy.
    fn map(&mut self, va: usize, pa: usize, flags: Flags) -> Result<(), ()> {
        let Self {
            root,
            tables,
            budget,
        } = self;
        let mut allocate = move || {
            *budget = budget.checked_sub(1)?;
            let mut table = Box::new(PageTable::zero());
            let address = table.as_mut() as *mut PageTable as usize;
            tables.push(table);
            Some(address)
        };
        root.map_with(va, pa, flags, SAS_TABLES, &mut allocate)
            .map_err(drop)
    }

    /// `map_page` over identity pages, checking each leaf.
    fn map_identity(&mut self, start: usize, pages: usize, flags: Flags, bits: u64) {
        for page in 0..pages {
            let va = start + page * PAGE;
            self.map(va, va, flags).expect("map");
            assert_eq!(self.leaf(va), Some(va as u64 | bits), "{va:#x}");
        }
    }

    /// `protect_page`'s leaf rewrite.
    fn protect(&mut self, va: usize, flags: Flags) -> Result<(), ()> {
        self.root.protect(va, flags).map_err(drop)
    }

    /// `unmap_page` without its shootdown: whether a leaf was cleared.
    fn unmap(&mut self, va: usize) -> bool {
        self.root.leaf_entry(va).is_some() && self.root.unmap(va).is_ok()
    }

    fn leaf(&self, va: usize) -> Option<u64> {
        self.root.leaf_entry(va)
    }

    /// Every intermediate entry is `frame | P|RW|US`. Returns the number of
    /// entries checked.
    fn tables_are_user_reachable(&self) -> usize {
        fn level(table: usize, depth: u32) -> usize {
            let mut count = 0;
            for index in 0..512 {
                // SAFETY: `table` is a live boxed scratch table; `index` < 512.
                let e = unsafe { core::ptr::read_volatile((table as *const u64).add(index)) };
                if depth == 3 || e & P == 0 {
                    continue;
                }
                assert_eq!(e & PS, 0, "no huge leaf in the shared root's low half");
                assert_eq!(
                    e & !PTE_ADDR_MASK,
                    TABLE_BITS,
                    "level {depth} index {index}"
                );
                count += 1 + level((e & PTE_ADDR_MASK) as usize, depth + 1);
            }
            count
        }
        level(self.root.as_ref() as *const PageTable as usize, 0)
    }
}

fn flags(bits: usize) -> Flags {
    Flags::from_bits(bits)
}

#[test]
fn shared_root_edits_write_the_raw_paths_entries() {
    let mut root = Scratch::new();

    // init_kernel_paging_x86: IOAPIC, HPET, LAPIC, then an MCFG window.
    for (va, pages) in [
        (0xFEC0_0000, 1),
        (0xFED0_0000, 1),
        (0xFEE0_0000, 1),
        (0xB000_0000, 16),
    ] {
        root.map_identity(va, pages, paging::MMIO_KERNEL_X86, P | RW | PCD);
    }
    // init_kernel_paging_x86's ECAM alias and kernel_mmio_alias: the same
    // frames again, supervisor-only in the alias window, with the identity
    // MMIO leaves' cache attribute (PCD) but NX.
    for page in 0..16 {
        let va = paging::mmio_alias::MMIO_ALIAS_BASE + page * PAGE;
        let pa = 0xB000_0000 + page * PAGE;
        root.map(va, pa, paging::mmio_alias::MMIO_ALIAS_X86)
            .expect("alias");
        assert_eq!(
            root.leaf(va),
            Some(pa as u64 | P | RW | PCD | NX),
            "{va:#x}"
        );
    }
    // map_mmio_x86: a BAR window, then the same pages again (overwrite).
    for _ in 0..2 {
        root.map_identity(0xFEBF_0000, 4, paging::MMIO_KERNEL_X86, P | RW | PCD);
    }
    // map_mmio_user_x86: a RequestMmio grant over mapped and fresh pages.
    root.map_identity(
        0xFEBF_2000,
        4,
        paging::MMIO_USER_X86,
        P | RW | US | PCD | NX,
    );
    // clear_mmio_user: the revoke keeps the frame and PCD, drops USER.
    for page in 0..4 {
        let va = 0xFEBF_2000 + page * PAGE;
        root.protect(va, paging::mmio_kernel_flags())
            .expect("revoke");
        assert_eq!(root.leaf(va), Some(va as u64 | P | RW | PCD | NX));
    }

    // map_page: kernel and loader flag sets, a supervisor leaf first and a
    // user leaf in the same page table after it, and high-index leaves.
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
    let high = (255 << 39) | (511 << 30) | (511 << 21) | (511 << 12);
    let mut frame = 0x0200_0000;
    for (va, f, bits) in [
        (0x4020_0000, kernel_rwx, P | RW),
        (0x4020_1000, user_rw, P | RW | US | NX),
        (0x4020_2000, kernel_rw, P | RW | NX),
        (0x4020_3000, user_rx, P | US),
        (0x4020_4000, kernel_rx, P),
        (0x4020_5000, user_ro, P | US | NX),
        (0x4020_6000, kernel_ro, P | NX),
        (0x4020_7000, user_load, P | RW | US),
        (0x1_B000_0000, user_rx, P | US),
        (high, user_ro, P | US | NX),
    ] {
        root.map(va, frame, f).expect("map_page");
        assert_eq!(root.leaf(va), Some(frame as u64 | bits), "{va:#x}");
        frame += PAGE;
    }

    // protect_page / protect_pages: the loader's W^X lowering, a protect
    // without VALID (P stays set on x86_64), pages that are not mapped.
    for (va, f, leaf) in [
        (0x4020_7000, user_rx, 0x0200_7000 | P | US),
        (0x4020_1000, user_ro, 0x0200_1000 | P | US | NX),
        (0x4020_2000, kernel_ro, 0x0200_2000 | P | NX),
        (0x4020_5000, flags(Flags::READ), 0x0200_5000 | P | NX),
    ] {
        root.protect(va, f).expect("protect");
        assert_eq!(root.leaf(va), Some(leaf), "{va:#x}");
    }
    assert_eq!(root.protect(0x4020_8000, user_ro), Err(()));
    assert_eq!(root.protect(0x7_0000_0000, user_ro), Err(()));

    // map_display_framebuffer_user and remap_range_user.
    root.map_identity(0x8000_0000, 4, paging::USER_RW_X86, P | RW | US | NX);
    root.map_identity(0x0700_0000, 4, paging::USER_RW_X86, P | RW | US | NX);

    // The BIOS area: map_bios_area's 4 KiB leaf (kernel RW, NX) through
    // map_page's kernel-RW flags.
    root.map_identity(0x9F * PAGE, 0x100 - 0x9F, kernel_rw, P | RW | NX);

    // The remote-shootdown fixture: a global leaf, re-pointed in place twice
    // without allocating.
    let probe = 0x0600_0000;
    let global = flags(Flags::VALID | Flags::READ | Flags::WRITE | Flags::GLOBAL);
    root.map(probe, 0x0300_0000, global).expect("fixture map");
    assert_eq!(root.leaf(probe), Some(0x0300_0000 | P | RW | NX | G));
    for target in [0x0300_1000, 0x0300_0000] {
        root.root
            .map_with(probe, target, global, SAS_TABLES, &mut || None)
            .expect("fixture leaf");
        assert_eq!(root.leaf(probe), Some(target as u64 | P | RW | NX | G));
    }

    // unmap_page: a present leaf, the same leaf again (idempotent), a page
    // never mapped, a path with no tables.
    assert!(root.unmap(probe));
    assert_eq!(root.leaf(probe), None);
    assert!(!root.unmap(probe));
    assert!(!root.unmap(0x4020_9000));
    assert!(!root.unmap(0x7_0000_0000));
    assert!(root.unmap(0x4020_3000));

    // A walk that runs out of frames publishes its new tables (the caller
    // prunes them) and writes no leaf.
    root.budget = 1;
    assert_eq!(root.map(0x60_0000_0000, frame, user_rw), Err(()));
    assert_eq!(root.leaf(0x60_0000_0000), None);

    assert!(root.tables_are_user_reachable() > 0);
}
