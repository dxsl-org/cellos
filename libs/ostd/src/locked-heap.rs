//! Preemption-safe lock around the cell's free-list heap.
//!
//! `task::spawn` gives a cell several preemptible tasks that share one heap, so
//! two of them can be inside `allocate_first_fit`/`deallocate` at once. The free
//! list is a chain of in-place hole headers; an unsynchronised interleaving links
//! one hole into two lists and hands the same bytes to two owners. Every heap
//! operation therefore runs under the lock below.
//!
//! The lock is a test-and-set flag whose waiters give their hart away. A plain
//! spinlock deadlocks a single-hart cell: the holder can be preempted inside the
//! critical section and the spinner would burn every slice the holder needs to
//! release it. A waiter spins briefly (the holder may be running on another hart
//! and leave within a few hundred cycles), then calls `relax` — the yield syscall
//! in a cell — so the scheduler runs the holder. Uncontended, the cost is one
//! atomic swap and one release store per allocation.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::{null_mut, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};
use linked_list_allocator::Heap;

/// Spin iterations before a waiter yields: long enough to ride out a holder on
/// another hart, short enough that a preempted holder costs little.
const SPINS_BEFORE_RELAX: u32 = 64;

/// A free-list heap whose every access is serialised by a yielding lock.
///
/// Holds no pointers in its initial state (`Heap::empty()` is all zero), so a
/// static of this type is zero-initialised `.bss`.
pub(crate) struct CellHeap {
    locked: AtomicBool,
    heap: UnsafeCell<Heap>,
}

// SAFETY: `heap` is only reached through `lock`, which grants one holder at a
// time; the Acquire/Release pair on `locked` orders the holders' accesses.
unsafe impl Sync for CellHeap {}
// SAFETY: the raw pointers inside `Heap` name the arena the heap owns, not
// thread-local data, so the heap may be handed to another thread.
unsafe impl Send for CellHeap {}

/// Exclusive access to the heap; releases the lock when dropped.
struct HeapGuard<'a> {
    owner: &'a CellHeap,
}

impl HeapGuard<'_> {
    fn heap(&mut self) -> &mut Heap {
        // SAFETY: a live guard is the sole holder of `owner.locked`, so no other
        // reference to the heap exists until it drops.
        unsafe { &mut *self.owner.heap.get() }
    }
}

impl Drop for HeapGuard<'_> {
    fn drop(&mut self) {
        self.owner.locked.store(false, Ordering::Release);
    }
}

impl CellHeap {
    pub(crate) const fn empty() -> Self {
        Self {
            locked: AtomicBool::new(false),
            heap: UnsafeCell::new(Heap::empty()),
        }
    }

    /// Acquire the lock. `relax` runs whenever the holder outlasts a short spin;
    /// it must let the holder make progress (yield the hart) and must not
    /// allocate.
    fn lock(&self, relax: fn()) -> HeapGuard<'_> {
        loop {
            if !self.locked.swap(true, Ordering::Acquire) {
                return HeapGuard { owner: self };
            }
            let mut spins = 0;
            while self.locked.load(Ordering::Relaxed) {
                if spins < SPINS_BEFORE_RELAX {
                    spins += 1;
                    core::hint::spin_loop();
                } else {
                    relax();
                }
            }
        }
    }

    /// Hand the heap the arena `[ptr, ptr + len)`.
    ///
    /// # Safety
    /// The arena must stay mapped and owned solely by this heap for the rest of
    /// the program, and no block from an earlier arena may still be live.
    pub(crate) unsafe fn init(&self, ptr: *mut u8, len: usize, relax: fn()) {
        self.lock(relax).heap().init(ptr, len);
    }

    /// Allocate `layout`, first giving the heap `default_arena()` if nothing has
    /// initialised it yet. Null on exhaustion.
    ///
    /// # Safety
    /// The arena `default_arena` returns must satisfy [`Self::init`]'s contract.
    pub(crate) unsafe fn alloc(
        &self,
        layout: Layout,
        relax: fn(),
        default_arena: impl FnOnce() -> (*mut u8, usize),
    ) -> *mut u8 {
        let mut guard = self.lock(relax);
        let heap = guard.heap();
        // Under the lock, so exactly one task performs the lazy init.
        if heap.size() == 0 {
            let (ptr, len) = default_arena();
            heap.init(ptr, len);
        }
        heap.allocate_first_fit(layout)
            .map_or(null_mut(), |p| p.as_ptr())
    }

    /// Return a block to the heap; null is ignored.
    ///
    /// # Safety
    /// A non-null `ptr` must come from [`Self::alloc`] on this heap with the same
    /// `layout` and must not be freed twice.
    pub(crate) unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout, relax: fn()) {
        if let Some(p) = NonNull::new(ptr) {
            self.lock(relax).heap().deallocate(p, layout);
        }
    }

    /// Bytes currently handed out (test observation of leaks and double frees).
    #[cfg(test)]
    fn used(&self, relax: fn()) -> usize {
        self.lock(relax).heap().used()
    }
}

#[cfg(test)]
#[path = "locked-heap-tests.rs"]
mod tests;
