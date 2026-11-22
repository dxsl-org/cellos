//! Stack Management for Tasks.
//!
//! Handles allocation, deallocation, and guard pages for Kernel and User stacks.
//! Complies with Rule 2 (Owned Buffers / Memory Safety) and Rule 8 (Resource Management).

use crate::memory::cell_quota::{Charge, Reservation};
use crate::memory::frame::FrameAllocator;
use crate::memory::frame_run::{Batch, FrameRun, KernelAlias, RunBill, RunKind, VaHold};
use crate::memory::paging::{self, Flags, PAGE_SIZE};
use log::{error, trace};
use types::{VAddr, ViError};

#[path = "stack-reservations.rs"]
pub(crate) mod reservations;
use reservations::STACK_RESERVATIONS;

/// Does `[start, end)` overlap a live stack run, guard pages included? The ELF
/// loader refuses such pages: an unmapped guard is not a free VA.
pub(crate) fn overlaps_stack_reservation(start: usize, end: usize) -> bool {
    STACK_RESERVATIONS.overlaps(start, end)
}

#[cfg(feature = "test-hooks")]
const STACK_WATERMARK_PATTERN: u8 = 0xA5;

/// Bottom guard reservation for every kernel and user stack.
pub const STACK_GUARD_PAGES: usize = 2;

/// Hand a stack run mapped at `va` to `batch`: unmap each of its pages
/// (recording the shared-root invalidation), detach the tables only the run
/// needed where no alias is restored (x86_64, and every user stack, whose
/// frames keep their kernel alias), and retire the run, which the batch frees
/// only once that invalidation is confirmed — restoring its kernel alias first
/// where the run says so.
///
/// `hold`: the stack's VA, released only with the frames (or after them, once
/// a quarantined run is freed): its `STACK_RESERVATIONS` entry, and for a user
/// stack its arena slot. `None` before either was taken.
/// Takes only `KERNEL_ROOT` (a leaf); the caller may hold `FRAME_ALLOCATOR`.
fn retire_run(batch: &mut Batch, mut run: FrameRun, va: VAddr, hold: Option<VaHold>) {
    let pages = run.pages();
    for i in 0..pages {
        let _ = paging::unmap_page(batch, va + i * PAGE_SIZE);
    }
    if run.alias() == KernelAlias::Intact {
        for i in 0..pages {
            paging::prune_empty_tables(va + i * PAGE_SIZE).retire_into(batch);
        }
    }
    // Every leaf of the run is gone from the kernel root. A run in any other
    // state than `Mapped` cannot reach here; the batch would leak it (counted),
    // never free it.
    let unpublished = run.unpublish();
    debug_assert!(unpublished.is_ok(), "stack run retired twice");
    batch.retire(match hold {
        Some(hold) => run.with_va_hold(hold),
        None => run,
    });
}

/// [`VaHold`] release for a kernel stack's run: only now are its guard VAs
/// identity-mapped again, so the loader's collision check takes over.
fn release_stack_reservation(start: VAddr, end: VAddr) {
    STACK_RESERVATIONS.remove(start, end);
}

/// [`VaHold`] release for a user stack: nothing translates any page of its
/// arena slot on any hart, so the slot may be handed out again.
fn release_stack_slot(start: VAddr, end: VAddr) {
    STACK_RESERVATIONS.remove(start, end);
    crate::loader::va_alloc::free_stack_va(start);
}

/// Retire `pages` user-published pages at VA `base` (a stack, or a grant at its
/// physical address) from the shared root before their frames return to the
/// allocator.
///
/// A Sas user copy that probed one of these pages earlier must not land after
/// its frame is freed (and, where the page was identity-mapped, re-mapped
/// kernel-RW, which the kernel writes without faulting): the copy would hit
/// whatever the frame becomes next (page table, another cell). Every PTE is
/// cleared here and recorded in `batch`,
/// whose finish invalidates every hart and then waits out any Sas commit
/// window still open (see `user_copy::sas_lease`) before it frees a run. The
/// frames stay UNMAPPED; the caller retires them into the same batch.
///
/// Lock order: KERNEL_ROOT (leaf) per page. The batch flushes only when it is
/// finished, after the caller released `FRAME_ALLOCATOR`.
pub(crate) fn unpublish_user_frames(batch: &mut Batch, base: VAddr, pages: usize) {
    // Before paging is live no translation (and no user copy) can exist.
    if pages == 0 || paging::KERNEL_ROOT.lock().is_none() {
        return;
    }
    for i in 0..pages {
        let _ = paging::unmap_page(batch, base + i * PAGE_SIZE);
    }
    batch.invalidate_range(base, pages * PAGE_SIZE);
    batch.drain_user_copies();
}

/// Represents an allocated Stack.
/// Implements Drop to automatically free pages.
///
/// Placement: a kernel stack is one contiguous run identity-mapped at its
/// physical address (VA == PA), guard frames included and unmapped. A user
/// stack is a contiguous run of only its usable frames, mapped USER RW at
/// `base + guard_pages * PAGE_SIZE` inside a stack-arena slot
/// (`loader::va_alloc::alloc_stack_va`); its guards are VA with neither a leaf
/// nor a frame behind them, and its frames keep their kernel alias. Either way
/// the run backs the top `run.pages()` pages of `[base, top)`.
#[derive(Debug)]
pub struct Stack {
    /// Base address (lowest address) of the allocated range.
    /// This includes the guard pages at the bottom.
    pub base: VAddr,
    /// Number of usable pages (excluding guard pages).
    pub pages: usize,
    /// Number of verified-unmapped pages below the usable range.
    pub guard_pages: usize,
    /// Top of the stack (initial SP).
    pub top: VAddr,
    /// The frames (a kernel stack's guards included); `None` only once retired
    /// (or for a host test's stack, which owns no frames).
    run: Option<FrameRun>,
    /// `base` is a stack-arena slot (a user stack), returned with the frames.
    in_arena: bool,
    #[cfg(feature = "p01-trace")]
    pub(crate) p01_obligation: Option<crate::p01_resources::Obligation>,
    /// Test-only lifetime registration for a kernel stack selected by a
    /// native-domain private root.
    #[cfg(all(
        feature = "native-domains",
        feature = "test-hooks",
        target_arch = "riscv64"
    ))]
    supervisor_registration: Option<crate::memory::domain_supervisor_registry::SupervisorRangeId>,
}

impl Stack {
    /// Allocate a new Kernel Stack of `pages` usable pages plus the configured
    /// bottom guard reservation, owned by the kernel and billed to nobody.
    ///
    /// # Errors
    /// - `OutOfMemory` — no contiguous run of usable plus guard frames exists, or a
    ///   page-table mapping could not be installed.
    /// - `NotSupported` — every guard page could not be established. No stack is
    ///   returned in that case; see [`Self::allocate`].
    pub fn new_kernel(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, false, Charge::KERNEL.into())
    }

    /// Allocate a new User Stack of `pages` usable pages below `STACK_GUARD_PAGES`
    /// unmapped guard pages, in a stack-arena slot (see [`Stack`]), owned by the
    /// kernel and billed to nobody. Usable pages are mapped USER RW.
    ///
    /// # Errors
    /// Same as [`Self::new_kernel`], plus `OutOfMemory` when every arena slot
    /// is held or the stack does not fit one.
    pub fn new_user(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, true, Charge::KERNEL.into())
    }

    /// [`Self::new_kernel`] billed with `charge`, which covers exactly
    /// [`Self::kernel_reserved_bytes`]`(pages)` and was taken before this call:
    /// the stack refunds it when its frames are freed, and a failed allocation
    /// refunds it at once.
    pub(crate) fn new_kernel_billed(pages: usize, charge: Reservation) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, false, charge.into())
    }

    /// [`Self::new_user`] billed as [`Self::new_kernel_billed`], with
    /// [`Self::user_reserved_bytes`]`(pages)`.
    pub(crate) fn new_user_billed(pages: usize, charge: Reservation) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, true, charge.into())
    }

    /// Bytes of frames a kernel stack of `pages` usable pages takes, guard
    /// frames included: what a billed kernel stack's reservation must cover.
    pub const fn kernel_reserved_bytes(pages: usize) -> usize {
        (pages + STACK_GUARD_PAGES) * PAGE_SIZE
    }

    /// Bytes of frames a user stack of `pages` usable pages takes: its guards
    /// have no frame.
    pub const fn user_reserved_bytes(pages: usize) -> usize {
        pages * PAGE_SIZE
    }

    /// Internal allocation logic.
    ///
    /// Contract: this either returns a stack whose guard page is *verified* absent
    /// from the page tables, or it returns an error having released every frame it
    /// took. There is deliberately no third outcome. An unguarded stack is not a
    /// degraded stack — in a single address space the frame below it belongs to
    /// another cell, so an overflow that should have trapped instead corrupts a
    /// neighbour with no fault and no log, and the victim dies later somewhere
    /// unrelated. A log line at allocation time is not a mitigation: nothing reads
    /// it in the microseconds before the overflow.
    fn allocate(
        pages: usize,
        guard_pages: usize,
        user_mode: bool,
        bill: RunBill,
    ) -> Result<Self, ViError> {
        let total_pages = pages.checked_add(guard_pages).ok_or(ViError::OutOfMemory)?;

        // Remapped usable pages and unmapped guards are invalidated on every
        // hart before the stack is handed out (or, on failure, before its run
        // is freed): the batch finishes after the frame allocator is released.
        let mut batch = Batch::new();
        let allocated = crate::memory::frame::with(|allocator| {
            if user_mode {
                Self::allocate_user_in(allocator, &mut batch, pages, guard_pages, total_pages, bill)
            } else {
                Self::allocate_kernel_in(
                    allocator,
                    &mut batch,
                    pages,
                    guard_pages,
                    total_pages,
                    bill,
                )
            }
        })
        .unwrap_or_else(|| {
            error!(
                "Stack alloc failed: frame allocator unavailable (user={}, pages={})",
                user_mode, total_pages
            );
            Err(ViError::OutOfMemory)
        });
        let flushed = batch.complete();
        let stack = allocated?;
        if !flushed.is_confirmed() {
            // A remote hart may still translate a guard or the old rights of a
            // usable page: no stack, and its own teardown quarantines the run.
            error!(
                "Stack alloc refused: invalidation of run 0x{:X} unconfirmed",
                stack.base
            );
            drop(stack);
            return Err(ViError::NotSupported);
        }
        Ok(stack)
    }

    /// A kernel stack, with the frame allocator held: one contiguous run of
    /// `total_pages` frames identity-mapped kernel RW at its physical address,
    /// its bottom `guard_pages` frames unmapped. Every changed translation, and
    /// on failure the whole run, goes to `batch`.
    fn allocate_kernel_in(
        allocator: &mut FrameAllocator,
        batch: &mut Batch,
        pages: usize,
        guard_pages: usize,
        total_pages: usize,
        bill: RunBill,
    ) -> Result<Self, ViError> {
        // A kernel stack stays ONE contiguous run at VA == PA: a secondary RV64
        // hart runs `smp_hart_entry` on its kernel stack with translation off
        // (`_secondary_entry` takes the stack top from the SBI opaque), so that
        // address must be valid before and after paging is on. User stacks map
        // from the stack arena instead (`allocate_user_in`).
        //
        // The cost: this can fail on a fragmented allocator while plenty of memory is
        // free, and no amount of lazy commit helps — a guard page cannot make VA != PA.
        // So callers must read `OutOfMemory` here as "no run this long exists right
        // now", which is recoverable, and never as grounds to panic.
        let base_frame = allocator.allocate_contiguous(total_pages).ok_or_else(|| {
            error!(
                "Stack alloc failed: no contiguous run (user=false, pages={}, bytes={})",
                total_pages,
                total_pages * PAGE_SIZE
            );
            ViError::OutOfMemory
        })?;

        let base_addr = base_frame; // Identity Map
                                    // Owned from here: every failure below retires it into `batch`, and a
                                    // run dropped un-retired is leaked (counted), never freed.
        let run = FrameRun::mapped(
            base_frame,
            total_pages,
            bill,
            RunKind::Leaf,
            KernelAlias::of_identity_remap(base_frame),
        );

        // 2. Map Pages
        // Bottom guard frames are left unmapped; usable frames begin after them.

        let usable_start_idx = guard_pages;

        // SAS identity map: all RAM is already identity-mapped RW+NX by
        // init_kernel_paging. The usable pages are re-mapped below, then every
        // guard frame is unmapped after the loop so an overflow traps.
        let flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );

        for i in usable_start_idx..total_pages {
            let addr = base_addr + (i * PAGE_SIZE);
            if paging::map_page(allocator, batch, addr, addr, flags).is_err() {
                // The run is not yet owned by a Stack, so nothing will drop it —
                // release it here or the frames are lost until reboot.
                error!("Stack alloc failed: cannot map page 0x{:X}", addr);
                retire_run(batch, run, base_addr, None);
                return Err(ViError::OutOfMemory);
            }
        }

        // Guard pages: drop the bottom frames' pre-existing identity mappings so a
        // stack overflow (a write below base_addr+PAGE_SIZE) faults instead of
        // silently corrupting the neighbouring frame. The spawn paths zero only
        // the usable pages (skipping base_addr), so nothing legitimately writes to
        // the guard frames. They stay owned by this Stack (freed in Drop); only
        // their PTEs are cleared. unmap_page locks KERNEL_ROOT (not FRAME_ALLOCATOR,
        // which we still hold) — no deadlock.
        //
        // The guard is verified by translation, not by the unmap's return code: on
        // some arches `unmap_page` reports success for a page it never touched
        // (paging root absent, or the PTE was not a 4 KiB leaf). Asking the page
        // tables whether the frame still resolves is the only answer that matches
        // what the hardware will do on overflow.
        for guard_index in 0..guard_pages {
            let guard_addr = base_addr + (guard_index * PAGE_SIZE);
            let unmap_ok = paging::unmap_page(batch, guard_addr).is_ok();
            paging::tlb_flush_all();
            if !unmap_ok || paging::virt_to_phys(guard_addr).is_some() {
                error!(
                    "Stack alloc refused: guard frame 0x{:X} still mapped (unmap_ok={})",
                    guard_addr, unmap_ok
                );
                retire_run(batch, run, base_addr, None);
                return Err(ViError::NotSupported);
            }
        }

        // Calculate Top (Stack grows down)
        // Top is at the END of the allocated range.
        let top = base_addr + (total_pages * PAGE_SIZE);

        // The guard VAs are now unmapped but owned: record the run so no cell
        // image can be placed over them (see `reservations`). FRAME_ALLOCATOR,
        // held since before the unmap, also excludes the ELF loader until then.
        if let Err(error) = STACK_RESERVATIONS.insert(base_addr, top) {
            error!("Stack alloc refused: run 0x{:X} not recorded", base_addr);
            retire_run(batch, run, base_addr, None);
            return Err(error);
        }

        #[cfg(all(
            feature = "native-domains",
            feature = "test-hooks",
            target_arch = "riscv64"
        ))]
        let supervisor_registration = if !crate::memory::domain_supervisor_registry::is_active() {
            None
        } else {
            match crate::memory::domain_supervisor_registry::register(
                base_addr + (guard_pages * PAGE_SIZE),
                top,
                crate::memory::domain_supervisor_registry::SupervisorRangeKind::KernelStack,
                crate::memory::domain_supervisor_registry::SupervisorRangeOwner::TaskStack,
            ) {
                Ok(id) => Some(id),
                Err(()) => {
                    let hold = VaHold {
                        release: release_stack_reservation,
                        start: base_addr,
                        end: top,
                    };
                    retire_run(batch, run, base_addr, Some(hold));
                    return Err(ViError::NotSupported);
                }
            }
        };
        trace!(
            "Allocated Stack: Base=0x{:X}, Top=0x{:X}, Pages={}, Guards={}, User=false",
            base_addr,
            top,
            pages,
            guard_pages
        );

        Ok(Stack {
            base: base_addr,
            pages,
            guard_pages,
            top,
            run: Some(run),
            in_arena: false,
            #[cfg(feature = "p01-trace")]
            p01_obligation: None,
            #[cfg(all(
                feature = "native-domains",
                feature = "test-hooks",
                target_arch = "riscv64"
            ))]
            supervisor_registration,
        })
    }

    /// A user stack, with the frame allocator held: a stack-arena slot, and one
    /// contiguous run of `pages` frames mapped USER RW right above the slot's
    /// `guard_pages` guard pages. The guards are never mapped and have no
    /// frame; the frames' kernel alias is never touched (`KernelAlias::Intact`).
    /// Every new translation, and on failure the run and the slot, goes to
    /// `batch`; the slot returns only after the batch's flush (`VaHold`).
    fn allocate_user_in(
        allocator: &mut FrameAllocator,
        batch: &mut Batch,
        pages: usize,
        guard_pages: usize,
        total_pages: usize,
        bill: RunBill,
    ) -> Result<Self, ViError> {
        use crate::loader::va_alloc;

        if total_pages > va_alloc::LARGE_STACK_VA_STRIDE / PAGE_SIZE {
            error!(
                "Stack alloc refused: {} pages exceed the largest ({}-byte) stack slot",
                total_pages,
                va_alloc::LARGE_STACK_VA_STRIDE
            );
            return Err(ViError::OutOfMemory);
        }
        let base = va_alloc::alloc_stack_va(total_pages).ok_or_else(|| {
            error!("Stack alloc failed: every stack slot of this size is held");
            ViError::OutOfMemory
        })?;
        let top = base + total_pages * PAGE_SIZE;
        // Recorded before anything is mapped, so every failure below returns
        // the slot through the run's `VaHold`, after the invalidation.
        if let Err(error) = STACK_RESERVATIONS.insert(base, top) {
            error!("Stack alloc refused: slot 0x{:X} not recorded", base);
            va_alloc::free_stack_va(base);
            return Err(error);
        }
        // Physically contiguous only to keep Phase 16's one-run release contract.
        let Some(base_frame) = allocator.allocate_contiguous(pages) else {
            error!(
                "Stack alloc failed: no contiguous run (user=true, pages={}, bytes={})",
                pages,
                pages * PAGE_SIZE
            );
            // Nothing was mapped in the slot: no translation to wait out.
            release_stack_slot(base, top);
            return Err(ViError::OutOfMemory);
        };
        // Owned from here: every failure below retires it into `batch`.
        let run = FrameRun::mapped(base_frame, pages, bill, RunKind::Leaf, KernelAlias::Intact);
        let hold = VaHold {
            release: release_stack_slot,
            start: base,
            end: top,
        };
        let usable = base + guard_pages * PAGE_SIZE;
        let flags = Flags::from_bits(
            Flags::VALID
                | Flags::READ
                | Flags::WRITE
                | Flags::USER
                | Flags::ACCESSED
                | Flags::DIRTY,
        );
        for i in 0..pages {
            let (va, frame) = (usable + i * PAGE_SIZE, base_frame + i * PAGE_SIZE);
            if paging::map_page(allocator, batch, va, frame, flags).is_err() {
                error!("Stack alloc failed: cannot map page 0x{:X}", va);
                retire_run(batch, run, usable, Some(hold));
                return Err(ViError::OutOfMemory);
            }
        }
        // The guards were never mapped: verify by translation, as for a kernel
        // stack, so a stale leaf in a reused slot can never pass as a guard.
        for guard_index in 0..guard_pages {
            let guard_addr = base + guard_index * PAGE_SIZE;
            if paging::virt_to_phys(guard_addr).is_some() {
                error!("Stack alloc refused: guard 0x{:X} is mapped", guard_addr);
                retire_run(batch, run, usable, Some(hold));
                return Err(ViError::NotSupported);
            }
        }
        trace!(
            "Allocated Stack: Base=0x{:X}, Top=0x{:X}, Frames=0x{:X}, Pages={}, Guards={}, User=true",
            base,
            top,
            base_frame,
            pages,
            guard_pages
        );

        Ok(Stack {
            base,
            pages,
            guard_pages,
            top,
            run: Some(run),
            in_arena: true,
            #[cfg(feature = "p01-trace")]
            p01_obligation: None,
            #[cfg(all(
                feature = "native-domains",
                feature = "test-hooks",
                target_arch = "riscv64"
            ))]
            supervisor_registration: None,
        })
    }

    /// Physical bytes this stack holds: its run, guard frames included for a
    /// kernel stack (a user stack's guards have no frame).
    pub fn allocated_bytes(&self) -> usize {
        self.run
            .as_ref()
            .map_or((self.pages + self.guard_pages) * PAGE_SIZE, |run| {
                run.pages() * PAGE_SIZE
            })
    }

    /// The run's physical base and frame count, while the stack holds it.
    pub fn frames(&self) -> Option<(usize, usize)> {
        self.run.as_ref().map(|run| (run.base(), run.pages()))
    }

    /// Physical address of the lowest usable byte: the run backs the top
    /// `run.pages()` pages of `[base, top)`.
    pub fn usable_phys(&self) -> Option<usize> {
        self.run
            .as_ref()
            .map(|run| run.base() + (run.pages() - self.pages) * PAGE_SIZE)
    }

    /// Zero the usable pages through the frames' kernel alias
    /// (`frame::phys_to_virt`), never through the stack's own VA: a user
    /// stack's VA is a USER mapping, which the kernel writes only inside a
    /// user-access window. Frames come back from the allocator holding their
    /// last owner's bytes, so every spawn path zeroes a stack before it runs.
    pub fn zero_usable(&self) {
        let Some(usable) = self.usable_phys() else {
            return;
        };
        // SAFETY: the run is this stack's alone and physically contiguous, and
        // its kernel alias is writable and contiguous: the identity map on
        // RV64/AArch64 (a user stack's frames keep the free-frame alias, a
        // kernel stack's usable pages are its own kernel RW leaves), the HHDM
        // on x86_64.
        unsafe {
            core::ptr::write_bytes(
                crate::memory::frame::phys_to_virt(usable) as *mut u8,
                0,
                self.usable_bytes(),
            );
        }
    }

    /// Task-owned usable stack bytes, excluding any kernel-only guard reservation.
    pub fn usable_bytes(&self) -> usize {
        self.pages * PAGE_SIZE
    }

    /// Are this stack's frames billed to a cell's quota (a thread's stacks)?
    pub fn is_billed(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.charge().account.is_some())
    }

    /// Lowest mapped byte in the usable stack range.
    pub fn usable_start(&self) -> usize {
        self.base + (self.guard_pages * PAGE_SIZE)
    }
    /// Whether `[ptr, ptr + len)` stays within this task's mapped stack bytes.
    pub fn contains_usable_range(&self, ptr: usize, len: usize) -> bool {
        let Some(end) = ptr.checked_add(len) else {
            return false;
        };
        ptr >= self.usable_start() && end <= self.top
    }

    #[cfg(feature = "test-hooks")]
    /// Prime the usable stack range with a sentinel pattern so later scans can
    /// estimate the deepest downward growth without adding a runtime ABI.
    /// Writes through the frames' kernel alias, never the stack's own VA: a
    /// user stack's VA is a USER mapping (SMAP on x86_64).
    pub fn test_hook_prime_watermark(&self) {
        assert!(paging::virt_to_phys(self.usable_start()).is_some());
        assert!(paging::virt_to_phys(self.top - 1).is_some());
        let Some(usable) = self.usable_phys() else {
            return;
        };
        // SAFETY: the run is this stack's alone and its kernel alias is
        // writable and contiguous (see `zero_usable`); this test hook owns the
        // stack before the task can run.
        unsafe {
            core::ptr::write_bytes(
                crate::memory::frame::phys_to_virt(usable) as *mut u8,
                STACK_WATERMARK_PATTERN,
                self.usable_bytes(),
            );
        }
    }

    #[cfg(feature = "test-hooks")]
    /// Return the deepest observed stack footprint in bytes since the sentinel
    /// pattern was primed, read through the frames' kernel alias.
    pub fn test_hook_watermark_bytes(&self) -> usize {
        assert!(paging::virt_to_phys(self.usable_start()).is_some());
        assert!(paging::virt_to_phys(self.top - 1).is_some());
        let Some(usable_pa) = self.usable_phys() else {
            return 0;
        };
        let usable = self.usable_bytes();
        let start = crate::memory::frame::phys_to_virt(usable_pa) as *const u8;
        let mut untouched_prefix = 0usize;
        while untouched_prefix < usable {
            // SAFETY: the scan stays within the run's usable bytes, read
            // through its kernel alias. Volatile reads observe the live
            // watermark.
            let byte = unsafe { core::ptr::read_volatile(start.add(untouched_prefix)) };
            if byte != STACK_WATERMARK_PATTERN {
                break;
            }
            untouched_prefix += 1;
        }
        usable.saturating_sub(untouched_prefix)
    }
}

#[cfg(feature = "test-hooks")]
pub fn stack_probe_self_test() -> bool {
    let stack = match Stack::new_kernel(2) {
        Ok(stack) => stack,
        Err(_) => return false,
    };
    if stack.guard_pages != STACK_GUARD_PAGES {
        return false;
    }
    for guard_index in 0..stack.guard_pages {
        let guard_addr = stack.base + (guard_index * PAGE_SIZE);
        if paging::virt_to_phys(guard_addr).is_some() {
            return false;
        }
    }
    let deliberate_overflow_target = stack.usable_start().saturating_sub(8);
    if paging::virt_to_phys(deliberate_overflow_target).is_some() {
        return false;
    }
    stack.test_hook_prime_watermark();
    if stack.test_hook_watermark_bytes() != 0 {
        return false;
    }
    let probe_bytes = 96usize;
    unsafe {
        core::ptr::write_bytes((stack.top - probe_bytes) as *mut u8, 0x11, probe_bytes);
    }
    stack.test_hook_watermark_bytes() == probe_bytes && user_stack_probe()
}

/// A user stack is mapped from the stack arena, not at its physical address:
/// its usable VA translates to its run but not to itself, both guards and the
/// overflow target below them are unmapped, and the stack's top word
/// (`top - 8`) translates to the run's last usable byte; a word written there
/// through the frames' kernel alias reads back through the same alias. The
/// kernel never dereferences the user stack VA (SMAP on x86_64).
#[cfg(feature = "test-hooks")]
fn user_stack_probe() -> bool {
    const MARK: u64 = 0x5354_4b41_5245_4e41;
    let Ok(stack) = Stack::new_user(2) else {
        return false;
    };
    let (Some(usable_pa), Some((_, frames))) = (stack.usable_phys(), stack.frames()) else {
        return false;
    };
    let usable = stack.usable_start();
    let off_identity = stack.in_arena
        && frames == stack.pages
        && paging::virt_to_phys(usable) == Some(usable_pa)
        && usable_pa != usable;
    let guarded = (0..stack.guard_pages)
        .all(|guard| paging::virt_to_phys(stack.base + guard * PAGE_SIZE).is_none())
        && paging::virt_to_phys(usable - 8).is_none();
    let top_pa = usable_pa + stack.usable_bytes() - 8;
    let alias = crate::memory::frame::phys_to_virt(top_pa);
    // SAFETY: the top 8 bytes of this stack's run, through its kernel alias;
    // the stack is owned here and runs no task.
    unsafe { core::ptr::write_volatile(alias as *mut u64, MARK) };
    // SAFETY: the same word through the same alias, still owned here.
    let landed = paging::virt_to_phys(stack.top - 8) == Some(top_pa)
        && unsafe { core::ptr::read_volatile(alias as *const u64) } == MARK;
    if !(off_identity && guarded && landed) {
        error!(
            "stack-probe: user stack base=0x{:X} usable_pa=0x{:X} off_identity={} guarded={} landed={}",
            stack.base, usable_pa, off_identity, guarded, landed
        );
    }
    off_identity && guarded && landed
}

/// Test-hooks: a free frame's kernel alias is `paging::FREE_FRAME_ALIAS`
/// (`valid && write && !exec && !user`, translating to itself) for a frame
/// fresh from the boot map, for the frames of a `Removed` run once freed (a
/// kernel stack, dropped: `KernelBackend::normalise` restored the alias), and
/// throughout the life of a user stack's frames, whose alias is never taken.
///
/// `None` where RAM has no identity alias (x86_64 reaches it through the HHDM).
#[cfg(feature = "test-hooks")]
pub fn free_frame_nx_self_test() -> Option<bool> {
    fn free_alias(frame: usize) -> bool {
        let Some(root) = *paging::KERNEL_ROOT.lock() else {
            return false;
        };
        // SAFETY: the live kernel root; its tables outlive this walk.
        unsafe { crate::hal::paging::leaf(root, frame) }.is_some_and(|leaf| {
            leaf.pa == frame && leaf.valid && leaf.write && !leaf.exec && !leaf.user
        })
    }
    fn alias_of(stack: &Stack) -> Option<KernelAlias> {
        stack.run.as_ref().map(FrameRun::alias)
    }

    let Some(fresh) = crate::memory::frame::allocate_frame() else {
        return Some(false);
    };
    if crate::memory::frame::phys_to_virt(fresh) != fresh {
        crate::memory::frame::deallocate_frame(fresh);
        return None;
    }
    let fresh_ok = free_alias(fresh);
    crate::memory::frame::deallocate_frame(fresh);

    let Ok(kernel) = Stack::new_kernel(1) else {
        return Some(false);
    };
    let removed = alias_of(&kernel) == Some(KernelAlias::Removed);
    let (guard, usable) = (kernel.base, kernel.usable_start());
    // While the stack lives its guard frame is unmapped.
    let live = paging::virt_to_phys(guard).is_none();
    drop(kernel);
    let restored = free_alias(guard) && free_alias(usable);

    let Ok(user) = Stack::new_user(1) else {
        return Some(false);
    };
    let Some((frame, _)) = user.frames() else {
        return Some(false);
    };
    let intact = alias_of(&user) == Some(KernelAlias::Intact) && free_alias(frame);
    drop(user);
    let kept = free_alias(frame);

    let ok = fresh_ok && removed && live && restored && intact && kept;
    if !ok {
        error!(
            "[selftest] FREE-FRAME-NX: fresh={} removed={} live={} restored={} intact={} kept={}",
            fresh_ok, removed, live, restored, intact, kept
        );
    }
    Some(ok)
}

impl Stack {
    /// The teardown up to its flush: unregister, unpublish every page, and
    /// retire the run (still holding its reservation) into `batch`.
    fn retire_run_into(&mut self, batch: &mut Batch) {
        #[cfg(all(
            feature = "native-domains",
            feature = "test-hooks",
            target_arch = "riscv64"
        ))]
        if let Some(id) = self.supervisor_registration.take() {
            let unregistered = crate::memory::domain_supervisor_registry::unregister(id);
            debug_assert!(unregistered);
        }
        trace!("Dropping Stack at 0x{:X}", self.base);

        let Some(run) = self.run.take() else {
            return;
        };
        let va = self.top - run.pages() * PAGE_SIZE;
        unpublish_user_frames(batch, va, run.pages());
        let release = if self.in_arena {
            release_stack_slot
        } else {
            release_stack_reservation
        };
        let hold = VaHold {
            release,
            start: self.base,
            end: self.top,
        };
        retire_run(batch, run, va, Some(hold));
    }

    /// Tear this stack down as part of a larger operation: its run joins
    /// `batch`, whose one shared flush covers every owner retired into it,
    /// and is freed — or quarantined, reservation included — when the batch
    /// finishes. A p01 obligation the stack still holds is dropped unfinished.
    #[cfg(any(feature = "test-hooks", all(feature = "native-domains", cellos_wide)))]
    fn retire_without_drop(self, batch: &mut Batch) {
        let mut stack = core::mem::ManuallyDrop::new(self);
        stack.retire_run_into(batch);
    }

    /// [`Self::retire_without_drop`] for the x86 silent-CPU fixture.
    #[cfg(feature = "test-hooks")]
    // reason: only the x86 silent-CPU fixture retires owners into a shared batch.
    #[allow(dead_code)]
    pub(crate) fn retire_into(self, batch: &mut Batch) {
        #[cfg(feature = "p01-trace")]
        debug_assert!(
            self.p01_obligation.is_none(),
            "a stack with a p01 obligation is retired only by its drop"
        );
        self.retire_without_drop(batch);
    }

    /// Tear a thread's stacks down after their unmap from a domain root could
    /// not confirm private tag `asid`: a remote hart may still resolve the
    /// stale private entry, so the runs are quarantined behind that tag (and a
    /// fresh epoch from every remote) whatever the shared flush says, and the
    /// reaper frees them — reservations with them — once it is confirmed.
    /// Before this phase they were forgotten and never reused (census F5).
    ///
    /// The caller settles any p01 obligation first; one still held is dropped
    /// unfinished, as the old `mem::forget` did.
    #[cfg(all(feature = "native-domains", cellos_wide))]
    pub(crate) fn retire_behind_tag(asid: usize, stacks: [Stack; 2]) {
        let mut batch = Batch::new();
        for stack in stacks {
            stack.retire_without_drop(&mut batch);
        }
        batch.await_tag(asid);
        let _ = batch.complete();
    }

    /// A stack descriptor that owns no frames, for host tests of range checks.
    #[cfg(test)]
    pub(crate) fn for_tests(base: VAddr, pages: usize, guard_pages: usize, top: VAddr) -> Self {
        Self {
            base,
            pages,
            guard_pages,
            top,
            run: None,
            in_arena: false,
            #[cfg(feature = "p01-trace")]
            p01_obligation: None,
            #[cfg(all(
                feature = "native-domains",
                feature = "test-hooks",
                target_arch = "riscv64"
            ))]
            supervisor_registration: None,
        }
    }

    /// A descriptor over synthetic frames billed with `charge`, for host tests
    /// that read [`Self::is_billed`]. The test must forget it: its frames are
    /// not the allocator's.
    #[cfg(test)]
    pub(crate) fn billed_for_tests(base: VAddr, pages: usize, charge: Reservation) -> Self {
        let mut stack = Self::for_tests(base, pages, 0, base + pages * PAGE_SIZE);
        stack.run = Some(FrameRun::mapped(
            base,
            pages,
            charge,
            RunKind::Leaf,
            KernelAlias::Intact,
        ));
        stack
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // One batch for the whole run: one shared invalidation, not one per
        // page. A sibling thread's Sas copy may still target this user stack,
        // so the batch drains copy windows after the flush and before the free.
        // An unconfirmed flush quarantines the run (and keeps its reservation)
        // until every hart acknowledges.
        let mut batch = Batch::new();
        #[cfg(feature = "p01-trace")]
        let frames = self.frames();
        self.retire_run_into(&mut batch);
        #[allow(unused_variables)] // reason: only the p01-trace ledger reads it
        let flushed = batch.complete();
        #[cfg(feature = "p01-trace")]
        if flushed.is_confirmed() {
            crate::p01_resources::finish(self.p01_obligation.take(), 1);
        }
        #[cfg(feature = "p01-trace")]
        if self.p01_obligation.is_some() {
            let owner = crate::p01_resources::obligation_owner(self.p01_obligation);
            if let (Some(_), Some((base, pages))) = (owner, frames) {
                crate::p01_retained::begin(
                    owner,
                    base,
                    pages,
                    crate::p01_retained::Reason::TaskStackForget,
                );
            }
            crate::p01_resources::finish(self.p01_obligation.take(), 6);
        }
    }
}
