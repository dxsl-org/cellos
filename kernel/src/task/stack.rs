//! Stack Management for Tasks.
//!
//! Handles allocation, deallocation, and guard pages for Kernel and User stacks.
//! Complies with Rule 2 (Owned Buffers / Memory Safety) and Rule 8 (Resource Management).

use crate::memory::frame::FrameAllocator;
use crate::memory::frame_run::{Batch, FrameRun, KernelAlias, RunKind, RunOwner, VaHold};
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

#[cfg(all(
    feature = "test-hooks",
    any(target_arch = "riscv64", target_arch = "riscv32")
))]
struct SumAccessGuard {
    restore: bool,
}

#[cfg(all(
    feature = "test-hooks",
    any(target_arch = "riscv64", target_arch = "riscv32")
))]
impl SumAccessGuard {
    fn enter() -> Self {
        // Test hooks run in supervisor mode. SUM is changed only for mapped
        // stack sampling and restored by Drop before this scope exits.
        const SUM_MASK: usize = 1usize << 18;
        let status = crate::arch::user_access::stack_sample_status();
        if status & SUM_MASK == 0 {
            crate::arch::user_access::sum_mask_set();
        }
        Self {
            restore: status & SUM_MASK == 0,
        }
    }
}

#[cfg(all(
    feature = "test-hooks",
    any(target_arch = "riscv64", target_arch = "riscv32")
))]
impl Drop for SumAccessGuard {
    fn drop(&mut self) {
        if self.restore {
            // This reverses the matching supervisor CSR change made by `enter`;
            // the guard is not shared across harts or task switches.
            crate::arch::user_access::sum_mask_clear();
        }
    }
}

/// Bottom guard reservation for every kernel and user stack.
pub const STACK_GUARD_PAGES: usize = 2;

/// Hand a stack run to `batch`: unmap each page (recording the shared-root
/// invalidation), detach the tables only the run needed where no alias is
/// restored (x86_64), and retire the run, which the batch frees only once that
/// invalidation is confirmed — restoring its kernel alias first where the run
/// says so.
///
/// `reserved`: the run is in `STACK_RESERVATIONS`, which keeps its VAs from the
/// loader until the frames are free (removed in the same allocator hold).
/// Takes only `KERNEL_ROOT` (a leaf); the caller may hold `FRAME_ALLOCATOR`.
fn retire_run(batch: &mut Batch, mut run: FrameRun, reserved: bool) {
    let (base, total_pages) = (run.base(), run.pages());
    for i in 0..total_pages {
        let _ = paging::unmap_page(batch, base + i * PAGE_SIZE);
    }
    if run.alias() == KernelAlias::Intact {
        for i in 0..total_pages {
            let page = base + i * PAGE_SIZE;
            paging::prune_empty_tables(page).retire_into(batch);
        }
    }
    // Every leaf of the run is gone from the kernel root. A run in any other
    // state than `Mapped` cannot reach here; the batch would leak it (counted),
    // never free it.
    let unpublished = run.unpublish();
    debug_assert!(unpublished.is_ok(), "stack run retired twice");
    batch.retire(if reserved {
        run.with_va_hold(VaHold {
            release: release_stack_reservation,
            start: base,
            end: base + total_pages * PAGE_SIZE,
        })
    } else {
        run
    });
}

/// [`VaHold`] release for a stack run: only now are its guard VAs
/// identity-mapped again, so the loader's collision check takes over.
fn release_stack_reservation(start: VAddr, end: VAddr) {
    STACK_RESERVATIONS.remove(start, end);
}

/// Retire `pages` identity-mapped frames at `base` from the shared root before
/// they return to the allocator.
///
/// A Sas user copy that probed one of these pages earlier must not land after
/// the frame is re-mapped kernel-RWX: the kernel writes that mapping without
/// faulting, so the copy would hit whatever the frame becomes next (page
/// table, another cell). Every PTE is cleared here and recorded in `batch`,
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
    /// The frames, guards included; `None` only once retired (or for a host
    /// test's stack, which owns no frames).
    run: Option<FrameRun>,
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
    /// bottom guard reservation.
    ///
    /// # Errors
    /// - `OutOfMemory` — no contiguous run of usable plus guard frames exists, or a
    ///   page-table mapping could not be installed.
    /// - `NotSupported` — every guard page could not be established. No stack is
    ///   returned in that case; see [`Self::allocate`].
    pub fn new_kernel(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, false)
    }

    /// Allocate a new User Stack of `pages` usable pages plus the configured
    /// bottom guards. Usable pages are mapped USER RW.
    ///
    /// # Errors
    /// Same as [`Self::new_kernel`].
    pub fn new_user(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, STACK_GUARD_PAGES, true)
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
    fn allocate(pages: usize, guard_pages: usize, user_mode: bool) -> Result<Self, ViError> {
        let total_pages = pages.checked_add(guard_pages).ok_or(ViError::OutOfMemory)?;

        // Remapped usable pages and unmapped guards are invalidated on every
        // hart before the stack is handed out (or, on failure, before its run
        // is freed): the batch finishes after the frame allocator is released.
        let mut batch = Batch::new();
        let allocated = crate::memory::frame::with(|allocator| {
            Self::allocate_in(
                allocator,
                &mut batch,
                pages,
                guard_pages,
                total_pages,
                user_mode,
            )
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

    /// [`Self::allocate`] with the frame allocator held. Every changed
    /// translation, and on failure the whole run, goes to `batch`.
    fn allocate_in(
        allocator: &mut FrameAllocator,
        batch: &mut Batch,
        pages: usize,
        guard_pages: usize,
        total_pages: usize,
        user_mode: bool,
    ) -> Result<Self, ViError> {
        // A stack must be ONE contiguous run of frames, and that is a consequence of
        // SAS rather than a shortcut: everything is identity-mapped (VA == PA) and
        // there is no virtual-address allocator, so virtual contiguity can only come
        // from physical contiguity.
        //
        // The cost: this can fail on a fragmented allocator while plenty of memory is
        // free, and no amount of lazy commit helps — a guard page cannot make VA != PA.
        // So callers must read `OutOfMemory` here as "no run this long exists right
        // now", which is recoverable, and never as grounds to panic. Lifting the
        // constraint means adding a VA allocator so stack pages can be scattered.
        let base_frame = allocator.allocate_contiguous(total_pages).ok_or_else(|| {
            error!(
                "Stack alloc failed: no contiguous run (user={}, pages={}, bytes={})",
                user_mode,
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
            RunOwner::Kernel,
            RunKind::Leaf,
            KernelAlias::of_identity_remap(base_frame),
        );

        // 2. Map Pages
        // Bottom guard frames are left unmapped; usable frames begin after them.

        let usable_start_idx = guard_pages;

        // SAS identity map: all RAM is already identity-mapped RWX by
        // init_kernel_paging. The usable pages are re-mapped below, then every
        // guard frame is unmapped after the loop so an overflow traps.

        // Usable Pages
        let flags = if user_mode {
            // User Read/Write (Exec?)
            Flags::from_bits(
                Flags::VALID
                    | Flags::READ
                    | Flags::WRITE
                    | Flags::USER
                    | Flags::ACCESSED
                    | Flags::DIRTY,
            )
        } else {
            // Kernel Read/Write
            Flags::from_bits(
                Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
            )
        };

        for i in usable_start_idx..total_pages {
            let addr = base_addr + (i * PAGE_SIZE);
            if paging::map_page(allocator, batch, addr, addr, flags).is_err() {
                // The run is not yet owned by a Stack, so nothing will drop it —
                // release it here or the frames are lost until reboot.
                error!("Stack alloc failed: cannot map page 0x{:X}", addr);
                retire_run(batch, run, false);
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
                retire_run(batch, run, false);
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
            retire_run(batch, run, false);
            return Err(error);
        }

        #[cfg(all(
            feature = "native-domains",
            feature = "test-hooks",
            target_arch = "riscv64"
        ))]
        let supervisor_registration =
            if user_mode || !crate::memory::domain_supervisor_registry::is_active() {
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
                        retire_run(batch, run, true);
                        return Err(ViError::NotSupported);
                    }
                }
            };
        trace!(
            "Allocated Stack: Base=0x{:X}, Top=0x{:X}, Pages={}, Guards={}, User={}",
            base_addr,
            top,
            pages,
            guard_pages,
            user_mode
        );

        Ok(Stack {
            base: base_addr,
            pages,
            guard_pages,
            top,
            run: Some(run),
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

    /// Total physical bytes reserved for this stack, including guard pages.
    pub fn allocated_bytes(&self) -> usize {
        (self.pages + self.guard_pages) * PAGE_SIZE
    }

    /// Task-owned usable stack bytes, excluding any kernel-only guard reservation.
    pub fn usable_bytes(&self) -> usize {
        self.pages * PAGE_SIZE
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
    pub fn test_hook_prime_watermark(&self) {
        assert!(paging::virt_to_phys(self.usable_start()).is_some());
        assert!(paging::virt_to_phys(self.top - 1).is_some());
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        let _sum_guard = SumAccessGuard::enter();
        // SAFETY: Stack allocation maps the asserted usable range contiguously;
        // this test hook owns the stack before the task can run.
        unsafe {
            core::ptr::write_bytes(
                self.usable_start() as *mut u8,
                STACK_WATERMARK_PATTERN,
                self.usable_bytes(),
            );
        }
    }

    #[cfg(feature = "test-hooks")]
    /// Return the deepest observed stack footprint in bytes since the sentinel
    /// pattern was primed.
    pub fn test_hook_watermark_bytes(&self) -> usize {
        assert!(paging::virt_to_phys(self.usable_start()).is_some());
        assert!(paging::virt_to_phys(self.top - 1).is_some());
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        let _sum_guard = SumAccessGuard::enter();

        let usable = self.usable_bytes();
        let start = self.usable_start() as *const u8;
        let mut untouched_prefix = 0usize;
        while untouched_prefix < usable {
            // SAFETY: Stack allocation maps the asserted usable range, and the
            // scan stays within it. Volatile reads observe the live watermark.
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
    stack.test_hook_watermark_bytes() == probe_bytes
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
        unpublish_user_frames(batch, run.base(), run.pages());
        retire_run(batch, run, true);
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
}

impl Drop for Stack {
    fn drop(&mut self) {
        // One batch for the whole run: one shared invalidation, not one per
        // page. A sibling thread's Sas copy may still target this user stack,
        // so the batch drains copy windows after the flush and before the free.
        // An unconfirmed flush quarantines the run (and keeps its reservation)
        // until every hart acknowledges.
        let mut batch = Batch::new();
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
            if owner.is_some() {
                crate::p01_retained::begin(
                    owner,
                    self.base,
                    self.pages + self.guard_pages,
                    crate::p01_retained::Reason::TaskStackForget,
                );
            }
            crate::p01_resources::finish(self.p01_obligation.take(), 6);
        }
    }
}
