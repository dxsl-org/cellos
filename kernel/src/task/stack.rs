//! Stack Management for Tasks.
//!
//! Handles allocation, deallocation, and guard pages for Kernel and User stacks.
//! Complies with Rule 2 (Owned Buffers / Memory Safety) and Rule 8 (Resource Management).

use crate::memory::frame::{FrameAllocator, FRAME_ALLOCATOR};
use crate::memory::paging::{self, Flags, PAGE_SIZE};
use log::{error, trace};
use types::{VAddr, ViError};

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
    /// This includes the guard page at the bottom if present.
    pub base: VAddr,
    /// Number of usable pages (excluding guard page).
    pub pages: usize,
    /// Whether this stack has a guard page.
    pub has_guard: bool,
    /// Top of the stack (initial SP).
    pub top: VAddr,
}

impl Stack {
    /// Allocate a new Kernel Stack of `pages` usable pages, plus a guard page
    /// below it.
    ///
    /// # Errors
    /// - `OutOfMemory` — no contiguous run of `pages + 1` frames exists, or a
    ///   page-table mapping could not be installed.
    /// - `NotSupported` — the guard page could not be established. No stack is
    ///   returned in that case; see [`Self::allocate`].
    pub fn new_kernel(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, true, false)
    }

    /// Allocate a new User Stack of `pages` usable pages, plus a guard page below
    /// it. Usable pages are mapped USER RW.
    ///
    /// # Errors
    /// Same as [`Self::new_kernel`].
    pub fn new_user(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, true, true)
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
    fn allocate(pages: usize, guard: bool, user_mode: bool) -> Result<Self, ViError> {
        let total_pages = if guard { pages + 1 } else { pages };

        let mut frame_guard = FRAME_ALLOCATOR.lock();
        let allocator = frame_guard.as_mut().ok_or(ViError::OutOfMemory)?;

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
        let base_frame = allocator
            .allocate_contiguous(total_pages)
            .ok_or(ViError::OutOfMemory)?;

        let base_addr = base_frame; // Identity Map

        // 2. Map Pages
        // If Guard Page is requested, the bottom page (base_addr) is NOT mapped (or mapped as invalid).
        // Ideally unmapped.

        let usable_start_idx = if guard { 1 } else { 0 };

        // SAS identity map: all RAM is already identity-mapped RWX by
        // init_kernel_paging. The usable pages are re-mapped below, then the guard
        // frame (base_addr) is unmapped after the loop so an overflow traps.

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

        // Guard page: drop the bottom frame's pre-existing identity mapping so a
        // stack overflow (a write below base_addr+PAGE_SIZE) faults instead of
        // silently corrupting the neighbouring frame. The spawn paths zero only
        // the usable pages (skipping base_addr), so nothing legitimately writes to
        // the guard frame. The frame stays owned by this Stack (freed in Drop);
        // only its PTE is cleared. unmap_page locks KERNEL_ROOT (not FRAME_ALLOCATOR,
        // which we still hold) — no deadlock.
        //
        // The guard is verified by translation, not by the unmap's return code: on
        // some arches `unmap_page` reports success for a page it never touched
        // (paging root absent, or the PTE was not a 4 KiB leaf). Asking the page
        // tables whether the frame still resolves is the only answer that matches
        // what the hardware will do on overflow.
        if guard {
            let unmap_ok = paging::unmap_page(base_addr).is_ok();
            paging::tlb_flush_all();
            if !unmap_ok || paging::virt_to_phys(base_addr).is_some() {
                error!(
                    "Stack alloc refused: guard frame 0x{:X} still mapped (unmap_ok={})",
                    base_addr, unmap_ok
                );
                release_frames(allocator, base_addr, total_pages);
                return Err(ViError::NotSupported);
            }
        }

        // Calculate Top (Stack grows down)
        // Top is at the END of the allocated range.
        let top = base_addr + (total_pages * PAGE_SIZE);

        trace!(
            "Allocated Stack: Base=0x{:X}, Top=0x{:X}, Pages={}, User={}",
            base_addr,
            top,
            pages,
            user_mode
        );

        Ok(Stack {
            base: base_addr,
            pages,
            has_guard: guard,
            top,
        })
    }

    /// Total physical bytes reserved for this stack, including the guard page.
    pub fn allocated_bytes(&self) -> usize {
        let total_pages = if self.has_guard {
            self.pages + 1
        } else {
            self.pages
        };
        total_pages * PAGE_SIZE
    }

    /// Task-owned usable stack bytes, excluding any kernel-only guard reservation.
    pub fn usable_bytes(&self) -> usize {
        self.pages * PAGE_SIZE
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        trace!("Dropping Stack at 0x{:X}", self.base);

        let total_pages = if self.has_guard {
            self.pages + 1
        } else {
            self.pages
        };

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
    /// VA base allocated by `va_alloc::alloc_cell_va` for PIE cells; `0` for
    /// fixed-VA cells.  Returned to the allocator's free list on drop.
    pie_va_base: usize,
}

impl CellSegments {
    pub fn new(
        pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
        pie_va_base: usize,
    ) -> Self {
        Self { pages, pie_va_base }
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
        let mut unmapped_any = false;
        for &(vaddr, frame) in &self.pages {
            // Only unmap a VA that still resolves to OUR frame (it won't if a
            // respawn already re-pointed it — leave the new mapping intact).
            if paging::virt_to_phys(vaddr) == Some(frame) {
                let _ = paging::unmap_page(vaddr);
                unmapped_any = true;
            }
        }
        if unmapped_any {
            paging::tlb_flush_all();
        }
    }
}

impl Drop for CellSegments {
    fn drop(&mut self) {
        let mut frame_guard = FRAME_ALLOCATOR.lock();
        if let Some(allocator) = frame_guard.as_mut() {
            for &(vaddr, frame) in &self.pages {
                // Only unmap if this VA still resolves to OUR frame. Cells load at
                // fixed VAs, so a supervised cell respawned at the same VA before we
                // are reaped will have re-pointed this VA at the NEW instance's frame
                // — unmapping it then would crash the new cell. Skip the unmap in
                // that case; the old frame is still ours to free either way.
                if paging::virt_to_phys(vaddr) == Some(frame) {
                    let _ = paging::unmap_page(vaddr);
                }
                allocator.deallocate_frame(frame);
            }
        }
        // Return the PIE VA slot to the allocator so it can be reused.
        if self.pie_va_base != 0 {
            crate::loader::va_alloc::free_cell_va(self.pie_va_base);
        }
    }
}
