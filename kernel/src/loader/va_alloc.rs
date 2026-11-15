//! Virtual address allocators: cell image slots and the user-stack arena.
//!
//! Assigns each cell image one aligned VA slot within the cell VA region
//! [CELL_VA_START, CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE): a PIE takes
//! any free slot (`alloc_cell_va`); a fixed-VA image claims the one slot it is
//! linked into (`reserve_cell_va`). Every PT_LOAD must stay inside the slot
//! its spawn owns (see `loader::elf::image_va`). A fixed-VA image links into
//! one of the lowest [`FIXED_VA_SLOTS`] slots: randomised order hands those to
//! a PIE only once every other slot is held.
//!
//! Every user stack takes one `STACK_VA_STRIDE` slot of the stack arena right
//! above the cell region (`alloc_stack_va`), so it is never mapped at its
//! physical address; the slot returns (`free_stack_va`) only once its frames'
//! translations are gone on every hart.
//!
//! Both regions live entirely below the SV39 user-half boundary (256 GiB), so
//! the `USER_VADDR_MAX` guard is never triggered by any allocated slot.
//!
//! # Design
//! Each region is one atomic ownership bitmap: bit N is set iff slot N has a
//! holder. `alloc` probes from a start index ([`SlotOrder`]) for the first clear
//! bit, wrapping once through the region, and sets it with one compare-exchange;
//! `reserve` sets one named bit; `free` clears it. Slots are all one size, so
//! any free slot fits any request: the region is exhausted only when every slot
//! is held, whatever the start index. A probe reads at most `slots / 64 + 1`
//! bitmap words, one more for a region with a fixed-VA band (10 for the 512
//! cell slots, 513 for the 32 768 stack slots).
//!
//! The start index is random per allocation when the kernel has a hardware
//! entropy source at boot (x86 RDSEED/RDRAND), so a cell's image and stack
//! addresses differ between boots; it is then drawn above the region's
//! fixed-VA band ([`Region::fixed`]), and the band is probed last. Without one
//! (RV64, AArch64, an x86 CPU without RDRAND) every probe starts at slot 0:
//! lowest free slot first, band included, the historical order.
//!
//! # Identity-mapped RAM
//! Grants and SHM pages keep their physical address as their VA, and RV64 and
//! AArch64 also map all RAM at VA == PA, so RAM must end below the cell slots:
//! the frame allocator leaves RAM at or above [`IDENTITY_RAM_END`] unused
//! (logged once at boot), and the boot maps assert it
//! ([`assert_identity_ram_below_cells`]). MMIO is refused window by window
//! where it would reach the slots or the arena ([`overlaps_allocated_va`]).

// RV32 lacks native 64-bit atomics; portable-atomic polyfills AtomicU64 there
// via the critical-section impl hal/arch/riscv registers.
#[cfg(not(target_arch = "riscv32"))]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(target_arch = "riscv32")]
use portable_atomic::AtomicU64;

#[cfg(all(feature = "large-cell-va", target_pointer_width = "32"))]
compile_error!("large-cell-va requires a 64-bit paged target");
#[cfg(all(feature = "large-cell-va", feature = "cell-scale-experiment"))]
compile_error!(
    "large-cell-va cannot be combined with cell-scale-experiment: exceeds SV39 user half"
);

/// Cell VA region start — 64 GiB (0x10_0000_0000).
///
/// Must lie above ALL identity-mapped regions in the SAS, avoiding collision
/// when physical RAM extends to >= 4 GiB (e.g. 0x1_0000_0000..0x1_8000_0000):
///   - QEMU virt RISC-V: UART/VirtIO MMIO at 0x1000_0000–0x1001_0000
///   - PCIe ECAM at 0x3000_0000 (1 MiB); VirtIO BAR below 0x8000_0000
///   - RAM identity map: supports physical configurations up to 48 GiB
///   - QEMU virt AArch64: GIC/peripheral MMIO below 0x1000_0000
///     64 GiB clears all of the above with room to spare and remains well inside
///     the user window (`hal::paging::USER_LIMIT`, 256 GiB).
///
/// RISC-V medany: even the 256 MiB desktop slot stays within ±2 GiB.
///
/// RV32 Nano boots with SATP=0 (bare physical, no paging — see specs/04), so
/// PIE cells never occur there and `alloc_cell_va` is never actually called;
/// this constant only needs to type-check as a 32-bit `usize` on that target.
#[cfg(not(target_arch = "riscv32"))]
pub(crate) const CELL_VA_START: usize = 0x10_0000_0000;
#[cfg(target_arch = "riscv32")]
pub(crate) const CELL_VA_START: usize = 0x2000_0000;

/// Embedded keeps the historical 32 MiB spacing. Desktop images opt in to a
/// 256 MiB slot, sufficient for a 128 MiB static arena plus code and data.
#[cfg(not(feature = "large-cell-va"))]
pub(crate) const CELL_VA_STRIDE: usize = 32 * 1024 * 1024;
#[cfg(feature = "large-cell-va")]
pub(crate) const CELL_VA_STRIDE: usize = 256 * 1024 * 1024;

/// Maximum simultaneous PIE cell slots: normally 512 (16 GiB embedded or
/// 128 GiB desktop). Both regions, starting at 64 GiB, fit SV39's user half.
///
/// `cell-scale-experiment` raises this to 4096 slots (4096 × 32 MiB = 128 GiB of
/// VA, still inside the user half) so the D5 scale measurement measures memory
/// rather than this constant. See `memory::cell_quota::MAX_CELLS`.
#[cfg(not(feature = "cell-scale-experiment"))]
const MAX_SLOTS: usize = 512;
#[cfg(feature = "cell-scale-experiment")]
const MAX_SLOTS: usize = 4096;

#[cfg(cellos_wide)]
const _: () = assert!(CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE <= crate::hal::paging::USER_LIMIT);

/// Exclusive end of the RAM the kernel manages and maps at VA == PA: the
/// first cell slot. The frame allocator leaves usable RAM at or above it
/// unused (`memory::frame::plan_managed_ranges`), so no grant, SHM page or
/// RV64/AArch64 RAM alias can land on a cell slot or the stack arena. Bare
/// physical targets map no cell slots and keep all RAM.
#[cfg(cellos_wide)]
pub(crate) const IDENTITY_RAM_END: usize = CELL_VA_START;
#[cfg(not(cellos_wide))]
pub(crate) const IDENTITY_RAM_END: usize = usize::MAX;

/// The lowest cell slots, kept for fixed-VA (ET_EXEC) images: randomised
/// order hands one to a PIE only once every other slot is held, so a fixed
/// image linked into this band finds its slot free while any slot is. The
/// deterministic order (no entropy) still takes slots lowest-first, band
/// included, exactly as before randomisation. No shipped cell is fixed-VA.
pub(crate) const FIXED_VA_SLOTS: usize = 8;

/// A run of equal, aligned VA slots: `slots` slots of `stride` bytes from
/// `start`; the lowest `fixed` of them are the fixed-VA band (see
/// [`FIXED_VA_SLOTS`]; `0` for a region no fixed image links into).
#[derive(Clone, Copy)]
pub(super) struct Region {
    start: usize,
    stride: usize,
    slots: usize,
    fixed: usize,
}

impl Region {
    /// Index of the slot whose base is exactly `base`.
    fn index_of_base(&self, base: usize) -> Option<usize> {
        let offset = base.checked_sub(self.start)?;
        (offset.is_multiple_of(self.stride) && offset / self.stride < self.slots)
            .then_some(offset / self.stride)
    }

    fn base(&self, index: usize) -> usize {
        self.start + index * self.stride
    }
}

/// The cell image slots.
const CELLS: Region = Region {
    start: CELL_VA_START,
    stride: CELL_VA_STRIDE,
    slots: MAX_SLOTS,
    fixed: FIXED_VA_SLOTS,
};

/// Exclusive end of a valid reservation, shared by ELF admission and release.
pub(crate) fn slot_end(base: usize) -> Option<usize> {
    CELLS.index_of_base(base)?;
    base.checked_add(CELL_VA_STRIDE)
}

/// Base of the slot containing `va`, or `None` outside the cell VA region.
pub(crate) fn slot_containing(va: usize) -> Option<usize> {
    let index = va.checked_sub(CELL_VA_START)? / CELL_VA_STRIDE;
    (index < MAX_SLOTS).then(|| CELL_VA_START + index * CELL_VA_STRIDE)
}

/// Number of AtomicU64 words needed to cover MAX_SLOTS bits.
const BITMAP_WORDS: usize = MAX_SLOTS.div_ceil(64);

/// Where [`SlotAllocator::alloc`] starts probing.
///
/// Randomised iff [`seed`](Self::seed) found its source able to fill one
/// 64-bit word; it is then read again for every allocation. Otherwise every
/// start is 0 (lowest free slot first) and the source is never called again.
pub(super) struct SlotOrder {
    /// Fills a prefix of the buffer with hardware entropy and returns its length
    /// (`dev::virtio_rng::get_random`'s contract); never allocates or blocks.
    source: fn(&mut [u8]) -> usize,
    randomised: AtomicBool,
}

impl SlotOrder {
    pub(super) const fn new(source: fn(&mut [u8]) -> usize) -> Self {
        Self {
            source,
            randomised: AtomicBool::new(false),
        }
    }

    /// Probe the source once and fix the order for this boot: `true` (random
    /// starts) iff it filled a whole word.
    #[cfg(any(cellos_wide, test))]
    pub(super) fn seed(&self) -> bool {
        let randomised = self.word().is_some();
        // Only this flag is published: the source holds its own state.
        self.randomised.store(randomised, Ordering::Relaxed);
        randomised
    }

    /// The random start offset for one allocation over a span of `span`
    /// slots, below `span`; `None` in deterministic order. In randomised order
    /// a source that fails this once (RDSEED and RDRAND both exhausted after
    /// their bounded retries) starts that one allocation at offset 0; which
    /// slot it gets never affects ownership.
    fn random_start(&self, span: usize) -> Option<usize> {
        if !self.randomised.load(Ordering::Relaxed) {
            return None;
        }
        Some(self.word().map_or(0, |word| (word % span as u64) as usize))
    }

    fn word(&self) -> Option<u64> {
        let mut bytes = [0u8; 8];
        ((self.source)(&mut bytes) == bytes.len()).then(|| u64::from_le_bytes(bytes))
    }
}

/// The start order of both regions, fixed once at boot by [`seed_slot_order`].
static SLOT_ORDER: SlotOrder = SlotOrder::new(crate::dev::virtio_rng::get_random);

/// Boot, once, before any cell image or user stack takes a slot: random slot
/// starts if the kernel entropy source works, else lowest-first. Logs which.
#[cfg(cellos_wide)]
pub(crate) fn seed_slot_order() {
    if SLOT_ORDER.seed() {
        log::info!("[va] slot order: randomised");
    } else {
        log::info!("[va] slot order: deterministic (no entropy)");
    }
}

/// Slot ownership over one [`Region`] (`WORDS` bitmap words cover its slots).
/// Invariant: bit N of `owned` is set iff slot N has exactly one holder, and no
/// bit at or past `region.slots` is ever set. Only an atomic read-modify-write
/// that finds a bit clear sets it (`alloc`, `reserve`), so no slot is handed
/// out twice; only `free` clears one.
pub(super) struct SlotAllocator<const WORDS: usize = BITMAP_WORDS> {
    region: Region,
    /// Each repeat of the inline `const` block is a distinct AtomicU64 (an
    /// inline const sidesteps `clippy::declare_interior_mutable_const`).
    owned: [AtomicU64; WORDS],
    /// `free` calls that found their slot already clear (see [`Self::free`]).
    stale_frees: AtomicUsize,
}

static SLOTS: SlotAllocator = SlotAllocator::new();

impl SlotAllocator {
    /// The cell image slots.
    pub(super) const fn new() -> Self {
        Self::over(CELLS)
    }
}

impl<const WORDS: usize> SlotAllocator<WORDS> {
    pub(super) const fn over(region: Region) -> Self {
        assert!(region.slots > 0 && region.slots <= WORDS * 64);
        assert!(region.fixed < region.slots);
        Self {
            region,
            owned: [const { AtomicU64::new(0) }; WORDS],
            stale_frees: AtomicUsize::new(0),
        }
    }

    /// Take a free slot in `order`; `None` only when every slot is held.
    ///
    /// Deterministic order: the lowest free slot. Randomised order: the first
    /// free slot at or after a random start above the fixed-VA band, wrapping
    /// once to the band's end; the band only when every other slot is held.
    pub(super) fn alloc(&self, order: &SlotOrder) -> Option<usize> {
        let (fixed, slots) = (self.region.fixed, self.region.slots);
        match order.random_start(slots - fixed) {
            None => self.claim_span(0, slots),
            Some(offset) => {
                let start = fixed + offset;
                self.claim_span(start, slots)
                    .or_else(|| self.claim_span(fixed, start))
                    .or_else(|| self.claim_span(0, fixed))
            }
        }
    }

    /// Claim the first free slot at or after `start`, wrapping past the last
    /// slot to slot 0: every slot is probed once. `start == 0` is lowest-first.
    #[cfg(test)]
    fn claim_from(&self, start: usize) -> Option<usize> {
        self.claim_span(start, self.region.slots)
            .or_else(|| self.claim_span(0, start))
    }

    /// Claim the lowest free slot in `[from, to)` (`to <= region.slots`),
    /// reading each bitmap word the span touches once.
    fn claim_span(&self, from: usize, to: usize) -> Option<usize> {
        let mut next = from;
        while next < to {
            let index = next / 64;
            let word_end = ((index + 1) * 64).min(to);
            // Bits `next % 64 ..= (word_end - 1) % 64` of word `index`.
            let probed = (u64::MAX << (next % 64)) & (u64::MAX >> (64 - (word_end - index * 64)));
            let word = &self.owned[index];
            let mut seen = word.load(Ordering::Relaxed);
            loop {
                let open = probed & !seen;
                if open == 0 {
                    break;
                }
                let bit = open.trailing_zeros() as usize;
                // Acquire: the previous holder's `free` happens before this
                // holder's use of the slot.
                match word.compare_exchange_weak(
                    seen,
                    seen | 1u64 << bit,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return Some(self.region.base(index * 64 + bit)),
                    Err(now) => seen = now,
                }
            }
            next = word_end;
        }
        None
    }

    /// Claim the exact slot at `base`, or refuse when another holder owns it.
    pub(super) fn reserve(&self, base: usize) -> bool {
        let Some(slot) = self.region.index_of_base(base) else {
            return false;
        };
        let mask = 1u64 << (slot % 64);
        self.owned[slot / 64].fetch_or(mask, Ordering::AcqRel) & mask == 0
    }

    /// Release the slot at `base`. Not a slot base: ignored. A slot nobody
    /// holds stays clear, so the ownership invariant holds, but a second free
    /// means some holder's bookkeeping is wrong (a later one could clear a new
    /// holder's bit): it is logged and counted, and test builds fail on it.
    pub(super) fn free(&self, base: usize) {
        let Some(slot) = self.region.index_of_base(base) else {
            return;
        };
        let mask = 1u64 << (slot % 64);
        if self.owned[slot / 64].fetch_and(!mask, Ordering::Release) & mask == 0 {
            let stale = self.stale_frees.fetch_add(1, Ordering::Relaxed) + 1;
            log::error!("[va] free of unheld slot {base:#x} ignored ({stale} so far)");
            #[cfg(test)]
            debug_assert!(false, "free of unheld slot {base:#x}");
        }
    }

    /// `free` calls that found their slot already clear.
    #[cfg(test)]
    pub(super) fn stale_frees(&self) -> usize {
        self.stale_frees.load(Ordering::Relaxed)
    }

    /// Slots owned right now.
    #[cfg(test)]
    pub(super) fn owned(&self) -> usize {
        self.owned
            .iter()
            .map(|word| word.load(Ordering::Acquire).count_ones() as usize)
            .sum()
    }

    /// The ownership bitmap, word by word.
    #[cfg(feature = "test-hooks")]
    fn snapshot(&self) -> alloc::vec::Vec<u64> {
        self.owned
            .iter()
            .map(|word| word.load(Ordering::Acquire))
            .collect()
    }

    /// Does the slot at `base` have a holder? `false` for a non-slot address.
    #[cfg(all(cellos_wide, feature = "test-hooks"))]
    fn held(&self, base: usize) -> bool {
        self.region.index_of_base(base).is_some_and(|slot| {
            self.owned[slot / 64].load(Ordering::Acquire) & (1u64 << (slot % 64)) != 0
        })
    }
}

/// First byte of the user-stack arena: right above the cell slots, so it lies
/// inside the user window and above all identity-mapped RAM (which ends below
/// the cell slots, [`IDENTITY_RAM_END`]). 80 GiB by default, 192 GiB with
/// `large-cell-va` or `cell-scale-experiment`.
#[cfg(cellos_wide)]
pub(crate) const STACK_VA_START: usize = CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE;

/// One user stack per slot: its bottom guards, then its usable pages, then
/// unmapped VA up to the next slot. The largest stack the kernel allocates
/// (`task::STACK_PAGES` plus `task::stack::STACK_GUARD_PAGES`) rounded up to a
/// power of two: 512 KiB.
pub(crate) const STACK_VA_STRIDE: usize =
    (crate::task::STACK_PAGES + crate::task::stack::STACK_GUARD_PAGES).next_power_of_two()
        * crate::memory::paging::PAGE_SIZE;

/// Live user stacks at once (16 GiB of VA at the 512 KiB stride).
#[cfg(cellos_wide)]
const STACK_SLOTS: usize = 32 * 1024;

#[cfg(cellos_wide)]
const _: () =
    assert!(STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE <= crate::hal::paging::USER_LIMIT);

#[cfg(cellos_wide)]
const STACKS: Region = Region {
    start: STACK_VA_START,
    stride: STACK_VA_STRIDE,
    slots: STACK_SLOTS,
    fixed: 0,
};

#[cfg(cellos_wide)]
static STACK_ARENA: SlotAllocator<{ STACK_SLOTS / 64 }> = SlotAllocator::over(STACKS);

/// Take one stack slot (`STACK_VA_STRIDE` bytes of VA, nothing mapped) in
/// this boot's [`SlotOrder`], or `None` when every slot is held.
#[cfg(cellos_wide)]
pub(crate) fn alloc_stack_va() -> Option<usize> {
    STACK_ARENA.alloc(&SLOT_ORDER)
}

/// Paging targets only: the bare-physical ones never map a stack.
#[cfg(not(cellos_wide))]
pub(crate) fn alloc_stack_va() -> Option<usize> {
    None
}

/// Return a stack slot. Only once nothing translates any page of it on any
/// hart: the stack's `VaHold` calls this after its run's flush is confirmed.
pub(crate) fn free_stack_va(base: usize) {
    #[cfg(cellos_wide)]
    STACK_ARENA.free(base);
    #[cfg(not(cellos_wide))]
    let _ = base;
}

/// Boot check: RAM mapped at VA == PA (grants and SHM pages keep their
/// physical address as VA; RV64 and AArch64 also map all RAM there) ends at or
/// below `ram_end`, which must not reach the cell slots. The frame allocator
/// already leaves RAM at or above [`IDENTITY_RAM_END`] unused, so this holds
/// unless firmware placed the kernel or bootloader memory there, which cannot
/// be dropped. MMIO is not RAM: each boot or later MMIO window is refused on
/// its own when it reaches the slots or the arena ([`overlaps_allocated_va`]).
#[cfg(cellos_wide)]
pub(crate) fn assert_identity_ram_below_cells(ram_end: usize) {
    assert!(
        ram_end <= IDENTITY_RAM_END,
        "identity-mapped RAM ends at {ram_end:#x}, inside the cell VA slots at {CELL_VA_START:#x}"
    );
}

/// End of the VA the kernel hands out from its allocators: the cell slots
/// `[CELL_VA_START, STACK_VA_START)`, then the stack arena right above them.
#[cfg(cellos_wide)]
const ALLOCATED_VA_END: usize = STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE;

/// Does `[start, start + len)` reach the VA the kernel hands out from its
/// allocators, `[CELL_VA_START, ALLOCATED_VA_END)` (cell slots and stack
/// arena)? A physical range that does can never be mapped at VA == PA: its
/// leaves would alias a cell image or a user stack. A range whose end
/// overflows counts as overlapping; an empty one never does.
#[cfg(cellos_wide)]
pub(crate) fn overlaps_allocated_va(start: usize, len: usize) -> bool {
    match start.checked_add(len) {
        Some(end) => len != 0 && start < ALLOCATED_VA_END && CELL_VA_START < end,
        None => true,
    }
}

/// Paging targets only: the bare-physical ones allocate no VA.
#[cfg(not(cellos_wide))]
pub(crate) fn overlaps_allocated_va(_start: usize, _len: usize) -> bool {
    false
}

/// Does `va` lie in the stack arena?
#[cfg(all(cellos_wide, feature = "test-hooks"))]
pub(crate) fn in_stack_arena(va: usize) -> bool {
    (STACK_VA_START..STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE).contains(&va)
}

/// Does the stack slot at `base` have a holder right now?
#[cfg(all(cellos_wide, feature = "test-hooks"))]
pub(crate) fn stack_slot_held(base: usize) -> bool {
    STACK_ARENA.held(base)
}

/// Allocate one profile-sized VA slot for a new PIE cell, in this boot's
/// [`SlotOrder`].
///
/// Returns `None` only when every slot is held (`MAX_SLOTS` live images; a
/// dead cell's slot returns once its flush is confirmed).
pub fn alloc_cell_va() -> Option<usize> {
    SLOTS.alloc(&SLOT_ORDER)
}

/// Claim the slot at `base` for a fixed-VA (ET_EXEC) image that is linked
/// inside it. `false` when `base` is not a slot base or another cell owns it:
/// a fixed-VA image may never land in a slot a PIE (or another image) holds.
/// With randomised order a live PIE may hold any slot outside the lowest
/// [`FIXED_VA_SLOTS`], and those only once every other slot is held.
pub fn reserve_cell_va(base: usize) -> bool {
    SLOTS.reserve(base)
}

/// Return a VA slot so it can be reused by a future cell.
///
/// `base` must be a value previously returned by `alloc_cell_va` or claimed
/// by `reserve_cell_va`. A non-slot address is ignored; a slot nobody holds
/// is logged and counted (`SlotAllocator::free`).
pub fn free_cell_va(base: usize) {
    SLOTS.free(base)
}

/// The cell slots' ownership bitmap, word by word.
#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> alloc::vec::Vec<u64> {
    SLOTS.snapshot()
}

#[cfg(test)]
#[path = "va-alloc-tests.rs"]
mod tests;
