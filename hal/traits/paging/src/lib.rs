#![no_std]

use types::*; // use VAddr, PhysAddr, ViResult

/// Page Table Entry Flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(pub usize);

impl PageFlags {
    // Standard flags (mapping to typical HW bits)
    pub const VALID: usize = 1 << 0;
    pub const READ: usize = 1 << 1;
    pub const WRITE: usize = 1 << 2;
    pub const EXECUTE: usize = 1 << 3;
    pub const USER: usize = 1 << 4;
    /// Global mapping: kept in every translation context.
    ///
    /// x86_64 sets `G` (bit 8); RISC-V copies it into the Sv39 `G` bit;
    /// AArch64 ignores it (its leaves are global unless `NON_GLOBAL`).
    pub const GLOBAL: usize = 1 << 5;
    pub const ACCESSED: usize = 1 << 6;
    pub const DIRTY: usize = 1 << 7;
    /// Device MMIO mapping — use non-cacheable Device-nGnRnE attributes (AArch64 MAIR index 0).
    /// On RISC-V and x86_64 this flag is ignored; x86_64 MMIO asks for
    /// [`Self::UNCACHED`] instead.
    pub const DEVICE: usize = 1 << 8;
    /// Address-space-scoped (non-global) mapping: the leaf belongs to the
    /// architectural tag in use, not to every root.
    ///
    /// AArch64 translates this to `PTE_nG` (bit 11). Without it the leaf is
    /// **global**, which has two consequences that matter for private roots:
    /// an ASID-targeted invalidation (`tlbi aside1is`) never clears it, and the
    /// translation stays usable after `TTBR0_EL1` is reprogrammed to a different
    /// root. Private-root-only leaves therefore carry this bit so the tag-release
    /// contract can be satisfied by targeted invalidation alone.
    ///
    /// RISC-V and x86_64 ignore it. Bit 9 is free in both: the x86_64 leaf
    /// translator composes only `P`/`RW`/`US`/`NX`/`G`/`PCD` from the bits
    /// documented for it and never reads any other bit, and on RISC-V the
    /// software bitfield is copied
    /// verbatim into the Sv39 leaf where bit 9 is `RSW[1]` — reserved for
    /// supervisor software, ignored by the hardware for permission checks and
    /// never consulted by `translate`/`unmap` (which mask the output address).
    pub const NON_GLOBAL: usize = 1 << 9;

    /// AArch64 BTI guarded page: the stage-1 leaf carries `GP` (bit 50), so an
    /// indirect branch into it must land on a `BTI` landing pad (FEAT_BTI).
    ///
    /// Only meaningful together with `EXECUTE`, and only set by the loader for
    /// executable pages of an image whose `GNU_PROPERTY_AARCH64_FEATURE_1_AND`
    /// note declares BTI, on a CPU that implements FEAT_BTI (bit 50 is RES0
    /// without it). x86_64 ignores it; RISC-V clears it, because bit 10 of an
    /// Sv39 leaf is the low bit of the PPN.
    pub const GUARDED: usize = 1 << 10;

    /// Uncached mapping: x86_64 sets `PCD` (cache-disable, bit 4) on the leaf.
    ///
    /// The cache attribute of x86_64 MMIO on the shared kernel root. AArch64
    /// ignores it (device memory is [`Self::DEVICE`]); RISC-V clears it,
    /// because bit 11 of an Sv39 leaf is a PPN bit.
    pub const UNCACHED: usize = 1 << 11;

    pub const R_W_X: usize = Self::READ | Self::WRITE | Self::EXECUTE;

    pub fn from_bits(bits: usize) -> Self {
        Self(bits)
    }

    pub fn bits(&self) -> usize {
        self.0
    }

    pub fn is_valid(&self) -> bool {
        (self.0 & Self::VALID) != 0
    }
    pub fn is_writeable(&self) -> bool {
        (self.0 & Self::WRITE) != 0
    }
    // ... add more as needed
}

/// One leaf translation as the hardware walker sees it, in neutral terms.
///
/// `pa` is the leaf's output address. For a 4 KiB leaf it is the frame on
/// every architecture. For a larger (block/huge) leaf, RISC-V adds the offset
/// of the walked address inside the leaf, x86_64 reports bits 51:12 of the
/// entry and AArch64 bits 47:12, as the walkers this type replaced did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaf {
    pub pa: PhysAddr,
    /// The entry is valid (present).
    pub valid: bool,
    /// Loads are permitted. Always true on x86_64 and AArch64.
    pub read: bool,
    /// Stores are permitted.
    pub write: bool,
    /// User mode may access the page.
    pub user: bool,
    /// Instruction fetch is permitted at the privilege the leaf grants
    /// (AArch64: `UXN` for a user leaf, `PXN` otherwise).
    pub exec: bool,
}

/// Which intermediate tables a new mapping makes user-reachable.
///
/// Only x86_64 carries a user bit on intermediate entries (U/S is ANDed down
/// the walk). RISC-V non-leaf entries have none and AArch64 leaves `APTable`
/// zero, so both treat the two policies alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePolicy {
    /// A new intermediate table is user-reachable only under a `USER` leaf,
    /// and a `USER` leaf upgrades every existing table on its walk, so a
    /// supervisor leaf is also refused at the table level. Private roots.
    FollowLeaf,
    /// Every new intermediate table is user-reachable and an existing one is
    /// left as it is: the leaf's own U/S bit is the only privilege check. The
    /// shared kernel root.
    UserReachable,
}

/// Generic Page Table Trait
pub trait PageTableTrait {
    /// Initialize paging (e.g., allocate root table).
    /// Typically returns the Physical Address of the root table.
    fn init(&mut self) -> ViResult<PhysAddr>;

    /// Map a virtual page to a physical frame.
    /// `alloc_fn` is a closure to allocate new frames for intermediate tables.
    fn map(
        &mut self,
        virt: VAddr,
        phys: PhysAddr,
        flags: PageFlags,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()>;

    /// [`map`](Self::map) with an explicit intermediate-table policy; `map`
    /// itself is [`TablePolicy::FollowLeaf`].
    fn map_with(
        &mut self,
        virt: VAddr,
        phys: PhysAddr,
        flags: PageFlags,
        tables: TablePolicy,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()> {
        // Only x86_64 has a table-level user bit (see `TablePolicy`).
        let _ = tables;
        self.map(virt, phys, flags, alloc_fn)
    }

    /// Rewrite the permissions of the 4 KiB page mapped at `virt`, keeping the
    /// frame it maps. `Err(NotFound)` when nothing maps `virt`.
    ///
    /// The default re-maps the translated frame with `flags`, which replace
    /// the old flags wholesale: without `VALID` the page is unmapped. x86_64
    /// keeps the frame and the cache and global bits of the old leaf and
    /// always sets `P`. Never allocates. No TLB invalidation beyond what
    /// [`map`](Self::map) does; the caller owns it.
    fn protect(&mut self, virt: VAddr, flags: PageFlags) -> ViResult<()> {
        let phys = self.translate(virt).ok_or(ViError::NotFound)? & !0xFFF;
        self.map(virt, phys, flags, &mut || None)
    }

    /// Unmap a virtual page.
    fn unmap(&mut self, virt: VAddr) -> ViResult<()>;

    /// Translate virtual address to physical address.
    fn translate(&self, virt: VAddr) -> Option<PhysAddr>;

    /// The leaf translating `virt`, or `None` when no valid leaf does.
    fn leaf(&self, virt: VAddr) -> Option<Leaf>;

    /// Identity map a region (Virtual = Physical).
    fn identity_map(
        &mut self,
        start: PhysAddr,
        end: PhysAddr,
        flags: PageFlags,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()> {
        let mut addr = start;
        // Assume generic page size 4KB for default identity map loop,
        // implementation can override for huge pages optimization.
        while addr < end {
            self.map(addr, addr, flags, alloc_fn)?;
            addr += 4096;
        }
        Ok(())
    }

    /// Activate this page table (load into CR3/SATP).
    /// # Safety
    /// Must ensure code/stack is mapped.
    unsafe fn activate(&self);
}
