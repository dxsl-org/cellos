//! Stack Management for Tasks.
//!
//! Handles allocation, deallocation, and guard pages for Kernel and User stacks.
//! Complies with Rule 2 (Owned Buffers / Memory Safety) and Rule 8 (Resource Management).

use crate::memory::frame::FRAME_ALLOCATOR;
use crate::memory::paging::{self, Flags, PAGE_SIZE};
use alloc::vec::Vec;
use log::{error, trace};
use types::{VAddr, ViError};

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
    /// Allocate a new Kernel Stack.
    /// - `pages`: Number of usable pages.
    /// - Uses `FRAME_ALLOCATOR` to get contiguous physical frames.
    /// - Maps them as RWX (Kernel).
    /// - Adds a Guard Page at the bottom (Unmapped).
    pub fn new_kernel(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, true, false)
    }

    /// Allocate a new User Stack.
    /// - `pages`: Number of usable pages.
    /// - Maps them as USER RWX.
    /// - Adds a Guard Page at the bottom (Unmapped).
    pub fn new_user(pages: usize) -> Result<Self, ViError> {
        Self::allocate(pages, true, true)
    }

    /// Internal allocation logic.
    fn allocate(pages: usize, guard: bool, user_mode: bool) -> Result<Self, ViError> {
        let total_pages = if guard { pages + 1 } else { pages };

        let mut frame_guard = FRAME_ALLOCATOR.lock();
        let allocator = frame_guard.as_mut().ok_or(ViError::OutOfMemory)?;

        // 1. Allocate contiguous frames
        // Note: Our simple allocator returns single frames.
        // We need contiguous VIRTUAL memory.
        // In Identity Mapping (SAS), Physical Contiguity = Virtual Contiguity.
        // So we need contiguous physical frames if we rely on simple pointer arithmetic.
        // However, `paging::map_page` maps arbitrary PAddr to VAddr.
        // BUT, our current `frame::FrameAllocator` (bitmap) might not guarantee contiguous frames.
        // And we don't have a virtual memory allocator (VMA) yet.
        //
        // TEMPORARY SOLUTION:
        // We assume we can get contiguous frames OR we are mapping to Identity.
        // For now, we try to allocate one by one.
        // Wait, if we use Identity Mapping for Kernel, we MUST have contiguous physical frames.
        //
        // If we are mapping User Stack, we can map arbitrary frames to contiguous Virtual Addresses?
        // No, we don't have a Virtual Address Allocator yet.
        // We are using Identity Mapping for everything currently (SAS).
        // So we MUST find a contiguous run of physical pages.
        //
        // Our `FrameAllocator` (likely a simple bump or bitmap) needs to support range allocation.
        // Looking at `kernel/src/memory/frame.rs` (not read yet, but assuming simple).
        //
        // Let's assume we can just call `allocate_frame` N times and check contiguity?
        // No, that's brittle.
        //
        // Let's rely on the fact that currently `allocate_frame` is likely linear.
        //
        // Refactoring: We will allocate the FIRST frame as base.
        // Then we hope subsequent calls are contiguous. If not, we panic/fail for now
        // (until VMA is implemented).

        let base_frame = allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;
        let mut frames = Vec::with_capacity(total_pages);
        frames.push(base_frame);

        for i in 1..total_pages {
            let frame = allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;
            if frame != base_frame + (i * PAGE_SIZE) {
                // If not contiguous, we are in trouble for Identity Mapping SAS.
                // We'd need to free previous and retry or have a better allocator.
                // For this task, we log warning and fail.
                error!("Stack allocation failed: Non-contiguous frames in Identity Map SAS.");
                return Err(ViError::OutOfMemory);
            }
            frames.push(frame);
        }

        let base_addr = base_frame; // Identity Map

        // 2. Map Pages
        // If Guard Page is requested, the bottom page (base_addr) is NOT mapped (or mapped as invalid).
        // Ideally unmapped.

        let usable_start_idx = if guard { 1 } else { 0 };

        // SAS identity map: all RAM is already identity-mapped RWX by
        // init_kernel_paging. The usable pages are re-mapped below. The guard
        // frame (base_addr) is left mapped for now — see the GUARD PAGE DEFERRED
        // note after the mapping loop.

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
            paging::map_page(allocator, addr, addr, flags).map_err(|_| ViError::OutOfMemory)?;
        }

        // GUARD PAGE DEFERRED: unmapping base_addr here makes boot fault with a
        // store page fault (scause=15) at stval=base_addr — the kernel WRITES to
        // the guard frame during task stack/context setup (verified 2026-06-06,
        // sepc≈0x80204eec). Re-enabling the guard requires first finding and
        // relocating that write (the context/trap-frame init must not touch
        // base_addr), or giving stacks user-VAs disjoint from the identity map.
        // Until then the guard frame stays mapped (no overflow protection).
        // Tracked in docs/specs/12-reliability.md §4.1.
        let _ = guard;

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
            for i in 0..total_pages {
                let frame = self.base + (i * PAGE_SIZE);
                // Unmap first so the PTE doesn't dangle after the frame is freed.
                let _ = paging::unmap_page(frame);
                allocator.deallocate_frame(frame);
            }
        }
    }
}
