//! Heap allocator for ViCell kernel.
//!
//! Wraps `linked_list_allocator::LockedHeap` in a `QuotaAlloc` that charges
//! every allocation to the currently-executing Cell's quota.  The kernel
//! itself (CellId = 0) is unlimited.  A Cell that exceeds its quota receives
//! a null pointer from `alloc()` — no panic, no system halt.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use linked_list_allocator::LockedHeap;

/// Why the allocator last handed back null: the Cell's quota, or the heap itself.
///
/// Counted here rather than logged: the log path may allocate, and an allocator
/// that logs on failure recurses. The counters are read outside `alloc` (the
/// spawn-refusal paths) so a capacity run can say *which* resource bound.
static NULL_FROM_QUOTA: AtomicUsize = AtomicUsize::new(0);
static NULL_FROM_HEAP: AtomicUsize = AtomicUsize::new(0);

/// Return addresses captured inside `alloc` at the moment it failed.
///
/// The alloc error handler's own scan sees stale frames from earlier calls; the
/// allocator's frame is the live one, so the callers it records are the ones that
/// actually asked for the layout.
static FAIL_SITE: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];

/// Record the live call chain (`.text` words only) when an allocation fails.
///
/// # Safety
/// Called from `alloc` with a live stack; the scan only reads it.
#[cfg(target_arch = "riscv64")]
unsafe fn capture_fail_site() {
    let sp: usize;
    // SAFETY: reading `sp` has no side effects.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    let mut index = 0usize;
    let mut offset = 0usize;
    let mut last = 0usize;
    while offset < 2048 && index < FAIL_SITE.len() {
        // SAFETY: the stack is mapped; the scan stays inside the live frames.
        let candidate = unsafe { core::ptr::read_volatile((sp + offset) as *const usize) };
        if (0x8020_0000..0x802c_9000).contains(&candidate) && candidate != last {
            FAIL_SITE[index].store(candidate, Ordering::Relaxed);
            index += 1;
            last = candidate;
        }
        offset += 8;
    }
}

/// The captured failure chain, in stack order (innermost first).
pub fn fail_site() -> [usize; 8] {
    let mut out = [0usize; 8];
    for (slot, value) in out.iter_mut().zip(FAIL_SITE.iter()) {
        *slot = value.load(Ordering::Relaxed);
    }
    out
}

/// Net bytes retained per allocation-size class (experiment builds only).
///
/// A capacity run needs to know *which sizes* hold the per-cell memory, not just
/// the total: `alloc`/`dealloc` add to and subtract from these buckets, and the
/// difference is what a parked cell retains. Counters only — no logging inside
/// the allocator.
#[cfg(feature = "cell-scale-experiment")]
pub mod size_hist {
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Upper bounds in bytes; the last bucket is everything above the previous one.
    pub const BOUNDS: [usize; 7] = [64, 256, 1024, 4096, 16 * 1024, 64 * 1024, 256 * 1024];
    pub const BUCKETS: usize = 8;

    static ALLOC_BYTES: [AtomicUsize; BUCKETS] =
        [const { AtomicUsize::new(0) }; BUCKETS];
    static FREE_BYTES: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
    static ALLOC_COUNT: [AtomicUsize; BUCKETS] =
        [const { AtomicUsize::new(0) }; BUCKETS];

    fn bucket(size: usize) -> usize {
        BOUNDS.iter().position(|&bound| size <= bound).unwrap_or(BUCKETS - 1)
    }

    pub(super) fn record_alloc(size: usize) {
        let index = bucket(size);
        ALLOC_BYTES[index].fetch_add(size, Ordering::Relaxed);
        ALLOC_COUNT[index].fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_free(size: usize) {
        FREE_BYTES[bucket(size)].fetch_add(size, Ordering::Relaxed);
    }

    /// `(upper_bound, alloc_count, net_bytes)` per bucket.
    pub fn snapshot() -> [(usize, usize, isize); BUCKETS] {
        let mut out = [(0usize, 0usize, 0isize); BUCKETS];
        for index in 0..BUCKETS {
            let bound = BOUNDS.get(index).copied().unwrap_or(usize::MAX);
            let alloc = ALLOC_BYTES[index].load(Ordering::Relaxed) as isize;
            let freed = FREE_BYTES[index].load(Ordering::Relaxed) as isize;
            out[index] = (
                bound,
                ALLOC_COUNT[index].load(Ordering::Relaxed),
                alloc - freed,
            );
        }
        out
    }
}

struct QuotaAlloc {
    inner: LockedHeap,
}

// SAFETY: QuotaAlloc delegates to LockedHeap which handles its own
// interior mutability safely.  cell_quota operations are also thread-safe.
unsafe impl GlobalAlloc for QuotaAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cell = crate::task::scheduler::current_cell_id();
        if !crate::memory::cell_quota::charge(cell, layout.size()) {
            // Cell quota exceeded — return null, no panic.
            NULL_FROM_QUOTA.fetch_add(1, Ordering::Relaxed);
            #[cfg(target_arch = "riscv64")]
            // SAFETY: called with a live stack; the scan only reads it.
            unsafe {
                capture_fail_site()
            };
            return core::ptr::null_mut();
        }
        let ptr = self.inner.alloc(layout);
        if ptr.is_null() {
            // Inner heap OOM — refund the charge we already applied.
            NULL_FROM_HEAP.fetch_add(1, Ordering::Relaxed);
            #[cfg(target_arch = "riscv64")]
            // SAFETY: called with a live stack; the scan only reads it.
            unsafe {
                capture_fail_site()
            };
            crate::memory::cell_quota::refund(cell, layout.size());
        } else {
            #[cfg(feature = "cell-scale-experiment")]
            size_hist::record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(feature = "cell-scale-experiment")]
        size_hist::record_free(layout.size());
        crate::memory::cell_quota::refund(crate::task::scheduler::current_cell_id(), layout.size());
        self.inner.dealloc(ptr, layout);
    }
}

#[cfg_attr(not(all(test, not(target_os = "none"))), global_allocator)]
static ALLOCATOR: QuotaAlloc = QuotaAlloc {
    inner: LockedHeap::empty(),
};

/// Initialise the kernel heap.
///
/// # Safety
/// Must be called exactly once after physical memory is mapped.
pub unsafe fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR
        .inner
        .lock()
        .init(heap_start as *mut u8, heap_size);
}

/// Kernel-heap bytes in use and still free.
///
/// Read from the spawn path so a capacity run reports the *retained* per-cell
/// cost (the difference between successive spawns) instead of only the ceiling.
pub fn usage() -> (usize, usize) {
    let heap = ALLOCATOR.inner.lock();
    (heap.used(), heap.free())
}

/// Fallible `Box::new`.
///
/// `Box::new` routes through the infallible allocator, so a full heap halts the
/// kernel instead of refusing the spawn. Every `Box` on the spawn path uses this.
pub fn try_box<T>(value: T) -> Result<alloc::boxed::Box<T>, types::ViError> {
    let layout = core::alloc::Layout::new::<T>();
    // SAFETY: `layout` describes `T`; a non-null pointer from the global allocator
    // is a valid uninitialised `T` slot, written exactly once here and owned by the
    // returned `Box` (which frees it with the same layout).
    unsafe {
        let raw = alloc::alloc::alloc(layout) as *mut T;
        if raw.is_null() {
            return Err(types::ViError::OutOfMemory);
        }
        raw.write(value);
        Ok(alloc::boxed::Box::from_raw(raw))
    }
}

/// `(null_from_quota, null_from_heap)` — which resource refused allocations.
pub fn oom_sources() -> (usize, usize) {
    (
        NULL_FROM_QUOTA.load(Ordering::Relaxed),
        NULL_FROM_HEAP.load(Ordering::Relaxed),
    )
}

/// Allocator error handler
#[cfg(not(all(test, not(target_os = "none"))))]
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    log::error!("allocation error: {:?}", layout);
    // Structure sizes, so the fatal layout above can be matched to what asked for
    // it (the spawn path's per-cell structures are the recurring candidates).
    log::error!(
        "  sizes: Task={} LoadedPage={} MappingEntry={} Stack={} MeasureEntry={}",
        core::mem::size_of::<crate::task::tcb::Task>(),
        core::mem::size_of::<crate::loader::elf::LoadedPage>(),
        core::mem::size_of::<crate::memory::address_space::MappingEntry>(),
        core::mem::size_of::<crate::task::stack::Stack>(),
        core::mem::size_of::<crate::measurement_log::MeasureEntry>(),
    );
    // The callers captured inside `alloc` — the live chain, unlike a scan here.
    let site = fail_site();
    for (depth, address) in site.iter().enumerate() {
        if *address != 0 {
            log::error!("  alloc caller[{}] = {:#x}", depth, address);
        }
    }
    // The kernel has no frame pointers and no unwinder, but the return addresses
    // of the live frames are still on the stack. Naming the call site is the
    // difference between "an allocation failed" and "this allocation failed", so
    // print the stack words that point into .text (bounded, deduped).
    #[cfg(target_arch = "riscv64")]
    {
        let sp: usize;
        // SAFETY: reading `sp` is side-effect free.
        unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
        let mut shown = 0usize;
        let mut offset = 0usize;
        while offset < 4096 && shown < 12 {
            // SAFETY: the stack is mapped; the scan stays inside the live frames.
            let candidate = unsafe { core::ptr::read_volatile((sp + offset) as *const usize) };
            if (0x8020_0000..0x802c_9000).contains(&candidate) {
                log::error!("  stack return-address candidate: {:#x}", candidate);
                shown += 1;
            }
            offset += 8;
        }
    }
    // Panic recovery is not possible for OOM, but we loop to avoid double-panics
    // if the panic handler tries to allocate.
    loop {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack))
        };
        #[cfg(not(target_arch = "x86_64"))]
        unsafe {
            core::arch::asm!("wfi")
        };
    }
}
