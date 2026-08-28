//! Stack Management for Tasks.
//!
//! Handles allocation, deallocation, and guard pages for Kernel and User stacks.
//! Complies with Rule 2 (Owned Buffers / Memory Safety) and Rule 8 (Resource Management).

use crate::memory::frame::{FrameAllocator, FRAME_ALLOCATOR};
use crate::memory::paging::{self, Flags, PAGE_SIZE};
use log::{error, trace};
use types::{VAddr, ViError};

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
        const SUM_MASK: usize = 1usize << 18;
        let status: usize;
        // SAFETY: test hooks run in supervisor mode. SUM is changed only for
        // mapped stack sampling and restored by Drop before this scope exits.
        unsafe {
            core::arch::asm!("csrr {status}, sstatus", status = out(reg) status, options(nostack));
            if status & SUM_MASK == 0 {
                core::arch::asm!("csrs sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
            }
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
            const SUM_MASK: usize = 1usize << 18;
            // SAFETY: this reverses the matching supervisor CSR change made by
            // `enter`; the guard is not shared across harts or task switches.
            unsafe {
                core::arch::asm!("csrc sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
            }
        }
    }
}

/// Bottom guard reservation for every kernel and user stack.
pub const STACK_GUARD_PAGES: usize = 2;

/// Hand `total_pages` frames starting at `base` back to `allocator`, restoring each
/// to the boot identity mapping (kernel RWX) first.
///
/// Invariant this upholds: every frame on the allocator's free list is
/// identity-mapped kernel-RWX. The cell loader zeroes a freshly allocated frame
/// through its identity address, so a frame released while unmapped (as the guard
/// frame is) store-faults for its next owner, and one released carrying USER flags
/// hands that owner the wrong permissions. Unmap-then-map normalises the PTE
/// whatever state the frame was left in.
///
/// The caller must already hold `FRAME_ALLOCATOR`; only `KERNEL_ROOT` (a leaf) is
/// taken here, which is the documented order.
fn release_frames(allocator: &mut FrameAllocator, base: VAddr, total_pages: usize) {
    let kernel_rwx = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );
    for i in 0..total_pages {
        let frame = base + (i * PAGE_SIZE);
        let _ = paging::unmap_page(frame);
        let _ = paging::map_page(allocator, frame, frame, kernel_rwx);
        allocator.deallocate_frame(frame);
    }
    paging::tlb_flush_all();
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

        let mut frame_guard = FRAME_ALLOCATOR.lock();
        let allocator = frame_guard.as_mut().ok_or_else(|| {
            error!(
                "Stack alloc failed: frame allocator unavailable (user={}, pages={})",
                user_mode, total_pages
            );
            ViError::OutOfMemory
        })?;

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
            if paging::map_page(allocator, addr, addr, flags).is_err() {
                // The run is not yet owned by a Stack, so nothing will drop it —
                // release it here or the frames are lost until reboot.
                error!("Stack alloc failed: cannot map page 0x{:X}", addr);
                release_frames(allocator, base_addr, total_pages);
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
            let unmap_ok = paging::unmap_page(guard_addr).is_ok();
            paging::tlb_flush_all();
            if !unmap_ok || paging::virt_to_phys(guard_addr).is_some() {
                error!(
                    "Stack alloc refused: guard frame 0x{:X} still mapped (unmap_ok={})",
                    guard_addr, unmap_ok
                );
                release_frames(allocator, base_addr, total_pages);
                return Err(ViError::NotSupported);
            }
        }

        // Calculate Top (Stack grows down)
        // Top is at the END of the allocated range.
        let top = base_addr + (total_pages * PAGE_SIZE);

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
                        release_frames(allocator, base_addr, total_pages);
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

impl Drop for Stack {
    fn drop(&mut self) {
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

        let total_pages = self.pages + self.guard_pages;

        let mut frame_guard = FRAME_ALLOCATOR.lock();
        if let Some(allocator) = frame_guard.as_mut() {
            release_frames(allocator, self.base, total_pages);
        }
    }
}

/// Physical frames mapped for a cell's ELF segments, recorded as `(vaddr, frame)`
/// at load time so they can be reclaimed when the cell dies.
///
/// `Stack::drop` only frees stacks; without this a cell's code/data frames leak
/// on every death (a supervised service restarted repeatedly would grow to OOM).
/// Segment frames are allocated exclusively for the cell by `load_segments`
/// (IPC/shared buffers use separate frames), so freeing them on death is safe.
#[derive(Debug)]
pub struct CellSegments {
    pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
    /// ELF pages whose final mapping grants the Cell write permission.
    writable_pages: alloc::vec::Vec<types::VAddr>,
    /// VA base allocated by `va_alloc::alloc_cell_va` for PIE cells; `0` for
    /// fixed-VA cells.  Returned to the allocator's free list on drop.
    pie_va_base: usize,
}

impl CellSegments {
    pub fn new(
        pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
        pie_va_base: usize,
    ) -> Self {
        Self {
            pages,
            writable_pages: alloc::vec::Vec::new(),
            pie_va_base,
        }
    }

    pub fn with_writable_pages(
        pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
        writable_pages: alloc::vec::Vec<types::VAddr>,
        pie_va_base: usize,
    ) -> Self {
        Self {
            pages,
            writable_pages,
            pie_va_base,
        }
    }
    /// Return the exclusive end of the writable page containing `ptr`.
    pub fn writable_page_end_containing(&self, ptr: usize) -> Option<usize> {
        let page = ptr & !(PAGE_SIZE - 1);
        self.writable_pages
            .contains(&page)
            .then(|| page.checked_add(PAGE_SIZE))
            .flatten()
    }

    /// Whether every page touched by this range is writable in this Cell image.
    pub fn contains_writable_range(&self, ptr: usize, len: usize) -> bool {
        let Some(end) = ptr.checked_add(len) else {
            return false;
        };
        if len == 0 {
            return false;
        }
        let mut page = ptr & !(PAGE_SIZE - 1);
        while page < end {
            if !self.writable_pages.contains(&page) {
                return false;
            }
            page = match page.checked_add(PAGE_SIZE) {
                Some(next) => next,
                None => return false,
            };
        }
        true
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn unpublished_pages(&self) -> &[(types::VAddr, types::PhysAddr)] {
        &self.pages
    }

    /// Total physical bytes reserved for this cell's ELF segment frames.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.len() * PAGE_SIZE
    }

    /// Unmap this cell's segment VAs immediately at death — WITHOUT freeing the
    /// frames (those are freed lazily when the zombie is reaped, in `drop`).
    ///
    /// Frees the address space right away so (a) a respawn can reuse the fixed VA
    /// and (b) the load-time overwrite guard (`load_segments`) only ever observes
    /// LIVE cells' (and kernel MMIO) mappings, never a dead-but-unreaped cell's.
    /// Locks only `KERNEL_ROOT` (a leaf), so it is safe under the SCHEDULER lock.
    pub fn eager_unmap(&self) {
        for &(vaddr, frame) in &self.pages {
            // Only unmap a VA that still resolves to OUR frame (it won't if a
            // respawn already re-pointed it — leave the new mapping intact).
            if paging::virt_to_phys(vaddr) == Some(frame) {
                let _ = paging::unmap_page(vaddr);
            }
            // Flush even when a replacement mapping won the race: an old remote
            // translation for this VA can still point at this segment's frame.
            crate::memory::tlb_shootdown::flush_page(vaddr);
        }
    }
}

impl Drop for CellSegments {
    fn drop(&mut self) {
        // Leaf and parent invalidations must reach every hart before either
        // backing or page-table frames return to the allocator.
        self.eager_unmap();
        for &(vaddr, _frame) in &self.pages {
            let reclaimed = paging::prune_empty_tables(vaddr);
            if !reclaimed.is_empty() {
                crate::memory::tlb_shootdown::flush_page(vaddr);
                let mut frames = FRAME_ALLOCATOR.lock();
                if let Some(allocator) = frames.as_mut() {
                    reclaimed.release(allocator);
                }
            }
        }
        let mut frame_guard = FRAME_ALLOCATOR.lock();
        if let Some(allocator) = frame_guard.as_mut() {
            for &(_vaddr, frame) in &self.pages {
                allocator.deallocate_frame(frame);
            }
        }
        if self.pie_va_base != 0 {
            crate::loader::va_alloc::free_cell_va(self.pie_va_base);
        }
    }
}
