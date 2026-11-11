//! Heap allocator for ViCell kernel.
//!
//! Wraps `linked_list_allocator::LockedHeap` in a `QuotaAlloc` that charges
//! every allocation to the currently-executing Cell's quota.  The kernel
//! itself (CellId = 0) is unlimited.  A Cell that exceeds its quota receives
//! a null pointer from `alloc()` — no panic, no system halt.  Each block
//! records the quota owner it was charged to, and `dealloc` refunds that
//! owner rather than whichever Cell is current at free time.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use linked_list_allocator::LockedHeap;

#[cfg(cellos_p01_trace)]
#[path = "heap/p01_owner.rs"]
pub(crate) mod p01_owner;

/// Why the allocator last handed back null: the Cell's quota, or the heap itself.
///
/// Counted here rather than logged: the log path may allocate, and an allocator
/// that logs on failure recurses. The counters are read outside `alloc` (the
/// spawn-refusal paths) so a capacity run can say *which* resource bound.
static NULL_FROM_QUOTA: AtomicUsize = AtomicUsize::new(0);
static NULL_FROM_HEAP: AtomicUsize = AtomicUsize::new(0);

#[cfg(cellos_p01_trace)]
static P01_EPOCH: AtomicUsize = AtomicUsize::new(1);
#[cfg(cellos_p01_trace)]
static P01_PEAK: AtomicUsize = AtomicUsize::new(0);

// Called only with the existing heap lock held; never calls usage or allocates.
#[cfg(cellos_p01_trace)]
fn p01_transition(used: usize) {
    if p01_owner::tracking() {
        let epoch = P01_EPOCH.load(Ordering::Relaxed);
        if let Some(next) = epoch.checked_add(1) {
            P01_EPOCH.store(next, Ordering::Relaxed);
            P01_PEAK.store(
                P01_PEAK.load(Ordering::Relaxed).max(used),
                Ordering::Relaxed,
            );
        } else {
            crate::p01_measurement::invalidate(1 << 12);
        }
    }
}

/// Return addresses captured inside `alloc` at the moment it failed.
///
/// The alloc error handler's own scan sees stale frames from earlier calls; the
/// allocator's frame is the live one, so the callers it records are the ones that
/// actually asked for the layout.
static FAIL_SITE: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];

/// Record the live call chain (`.text` words only) when an allocation fails.
///
/// # Safety
/// Called from `alloc` with a live stack; the scan only reads it. The scan
/// never crosses the page above `sp`: a shallow frame can sit just below a
/// stack top, and the page past a top is unmapped (often another stack's
/// guard), so reading it would fault inside the allocator.
#[cfg(target_arch = "riscv64")]
unsafe fn capture_fail_site() {
    let sp: usize;
    // SAFETY: reading `sp` has no side effects.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    let mut index = 0usize;
    let mut offset = 0usize;
    let mut last = 0usize;
    let page = crate::memory::paging::PAGE_SIZE;
    let limit = 2048.min(page - (sp & (page - 1)));
    while offset < limit && index < FAIL_SITE.len() {
        // SAFETY: `sp`'s own page is mapped and the scan stays inside it.
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

    static ALLOC_BYTES: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
    static FREE_BYTES: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
    static ALLOC_COUNT: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];

    fn bucket(size: usize) -> usize {
        BOUNDS
            .iter()
            .position(|&bound| size <= bound)
            .unwrap_or(BUCKETS - 1)
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

/// Every heap block carries the quota owner token it was charged to (see
/// `cell_quota::charge_owner`) in a trailing word just past the payload, so
/// `dealloc` refunds that exact row incarnation — not the Cell current at free
/// time, and not a later incarnation of a reused CellId.
///
/// A suffix keeps the returned pointer exactly the inner allocator's, so
/// over-aligned layouts pay no alignment padding. Uncharged (kernel-context)
/// blocks carry `UNCHARGED_OWNER` so the rule is uniform: `dealloc` never has
/// to guess whether a block is tagged. The tag is read/written unaligned.
const OWNER_TAG_BYTES: usize = core::mem::size_of::<usize>();

/// Inner-heap layout for a payload `layout`: same alignment, owner tag appended.
fn tagged_layout(layout: Layout) -> Option<Layout> {
    Layout::from_size_align(layout.size().checked_add(OWNER_TAG_BYTES)?, layout.align()).ok()
}

struct QuotaAlloc {
    inner: LockedHeap,
}

impl QuotaAlloc {
    unsafe fn alloc_impl(
        &self,
        layout: Layout,
        #[cfg(cellos_p01_trace)] origin: Option<p01_owner::Provenance>,
    ) -> *mut u8 {
        let cell = crate::sched::scheduler::current_cell_id();
        let Some(owner) = crate::memory::cell_quota::charge_owner(cell, layout.size()) else {
            // Cell quota exceeded — return null, no panic.
            NULL_FROM_QUOTA.fetch_add(1, Ordering::Relaxed);
            #[cfg(target_arch = "riscv64")]
            // SAFETY: called with a live stack; the scan only reads it.
            unsafe {
                capture_fail_site()
            };
            return core::ptr::null_mut();
        };
        // A payload too large to carry the tag cannot be backed by the heap
        // either; it takes the inner-heap failure path below.
        let tagged = tagged_layout(layout);
        #[cfg(not(cellos_p01_trace))]
        let ptr = tagged.map_or(core::ptr::null_mut(), |tagged| self.inner.alloc(tagged));
        #[cfg(cellos_p01_trace)]
        let ptr = {
            let mut heap = self.inner.lock();
            let before = heap.used();
            let result = tagged
                .map_or(Err(()), |tagged| heap.allocate_first_fit(tagged))
                .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr());
            if !result.is_null() {
                p01_owner::allocated(
                    result,
                    layout,
                    origin,
                    heap.used() - before,
                    P01_EPOCH.load(Ordering::Relaxed).saturating_add(1) as u64,
                );
            }
            p01_transition(heap.used());
            result
        };
        if ptr.is_null() {
            // Inner heap OOM — refund the charge we already applied.
            NULL_FROM_HEAP.fetch_add(1, Ordering::Relaxed);
            #[cfg(target_arch = "riscv64")]
            // SAFETY: called with a live stack; the scan only reads it.
            unsafe {
                capture_fail_site()
            };
            crate::memory::cell_quota::refund_owner(owner, layout.size());
        } else {
            // SAFETY: the block spans the tagged layout, so the tag word just
            // past the payload is inside it.
            ptr.add(layout.size())
                .cast::<usize>()
                .write_unaligned(owner);
            #[cfg(feature = "cell-scale-experiment")]
            size_hist::record_alloc(layout.size());
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::heap_alloc(layout.size());
        }
        ptr
    }
}

/// Local interrupts masked for one heap transaction, restored on drop (the
/// same save/disable/restore as `crate::sync::Spinlock`). `LockedHeap`'s own
/// lock does not mask interrupts, while the timer/IPI ISRs and the #PF funnel
/// allocate and free: a same-hart interrupt landing while an IF=1 kernel
/// context held the heap lock would spin on it forever.
struct HeapIrqMask {
    #[cfg(not(all(test, not(target_os = "none"))))]
    saved: bool,
}

impl HeapIrqMask {
    fn enter() -> Self {
        #[cfg(not(all(test, not(target_os = "none"))))]
        {
            use crate::hal::Arch;
            let saved = crate::hal::ARCH.interrupts_enabled();
            crate::hal::ARCH.disable_interrupts();
            Self { saved }
        }
        #[cfg(all(test, not(target_os = "none")))]
        Self {}
    }
}

impl Drop for HeapIrqMask {
    fn drop(&mut self) {
        #[cfg(not(all(test, not(target_os = "none"))))]
        if self.saved {
            use crate::hal::Arch;
            crate::hal::ARCH.enable_interrupts();
        }
    }
}

// SAFETY: allocations delegate to LockedHeap with the owner tag appended to the
// payload layout; dealloc rebuilds the same tagged layout and refunds the
// recorded owner. Private provenance is serialized by that same heap guard.
// Every entry masks local interrupts first (`HeapIrqMask`), so the quota charge,
// the heap lock and the refund form one non-interruptible transaction.
unsafe impl GlobalAlloc for QuotaAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _irq = HeapIrqMask::enter();
        self.alloc_impl(
            layout,
            #[cfg(cellos_p01_trace)]
            None,
        )
    }

    #[cfg(cellos_p01_trace)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let _irq = HeapIrqMask::enter();
        let ptr = self.alloc_impl(layout, None);
        if !ptr.is_null() {
            ptr.write_bytes(0, layout.size());
        }
        ptr
    }

    #[cfg(cellos_p01_trace)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Match GlobalAlloc's default allocate/copy/free behavior exactly.
        // Only diagnostic provenance differs: it follows the old object rather
        // than the current charging Cell. Failure leaves the old pointer live.
        let _irq = HeapIrqMask::enter();
        let origin = {
            let _heap = self.inner.lock();
            p01_owner::realloc_origin(ptr, layout)
        };
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new_ptr = self.alloc_impl(new_layout, Some(origin));
        if !new_ptr.is_null() {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            self.dealloc(ptr, layout);
        }
        new_ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _irq = HeapIrqMask::enter();
        #[cfg(feature = "cell-scale-experiment")]
        size_hist::record_free(layout.size());
        // SAFETY: `alloc_impl` returned `ptr` for `layout`, so the block holds
        // the owner tag just past the payload and `tagged_layout` succeeded.
        let owner = ptr.add(layout.size()).cast::<usize>().read_unaligned();
        crate::memory::cell_quota::refund_owner(owner, layout.size());
        let tagged =
            Layout::from_size_align_unchecked(layout.size() + OWNER_TAG_BYTES, layout.align());
        #[cfg(not(cellos_p01_trace))]
        self.inner.dealloc(ptr, tagged);
        #[cfg(cellos_p01_trace)]
        {
            let mut heap = self.inner.lock();
            let before = heap.used();
            heap.deallocate(core::ptr::NonNull::new_unchecked(ptr), tagged);
            p01_owner::freed(ptr, layout, before - heap.used());
            p01_transition(heap.used());
        }
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
    let _irq = HeapIrqMask::enter();
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
    let _irq = HeapIrqMask::enter();
    let heap = ALLOCATOR.inner.lock();
    (heap.used(), heap.free())
}

#[cfg(cellos_p01_trace)]
pub(crate) fn p01_snapshot() -> (usize, usize, usize, usize) {
    let _irq = HeapIrqMask::enter();
    let heap = ALLOCATOR.inner.lock();
    (
        heap.used(),
        heap.free(),
        P01_EPOCH.load(Ordering::Relaxed),
        P01_PEAK.load(Ordering::Relaxed),
    )
}

#[cfg(cellos_p01_trace)]
pub(crate) fn p01_reset_peak() {
    let _irq = HeapIrqMask::enter();
    let heap = ALLOCATOR.inner.lock();
    if let Some(_observation) = crate::p01_measurement::observation() {
        P01_PEAK.store(heap.used(), Ordering::Relaxed);
    }
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
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    ))]
    let mapping_entry = core::mem::size_of::<crate::memory::address_space::MappingEntry>();
    #[cfg(not(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    )))]
    let mapping_entry = 0;
    log::error!(
        "  sizes: Task={} LoadedPage={} MappingEntry={} Stack={} MeasureEntry={}",
        core::mem::size_of::<crate::task::tcb::Task>(),
        core::mem::size_of::<crate::loader::elf::LoadedPage>(),
        mapping_entry,
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

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::memory::cell_quota::{self, MAX_CELLS};
    use crate::sched::hart_local::{current_cell_id, set_current_cell_id};
    use types::CellId;

    const OWNER: usize = MAX_CELLS - 1;
    const OTHER: usize = MAX_CELLS - 2;

    fn in_use(cell: usize) -> usize {
        cell_quota::in_use(CellId(cell as u64))
    }

    #[test]
    fn dealloc_refunds_the_charged_owner_incarnation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let previous = current_cell_id();
        let mut arena = alloc::vec![0u64; 1024];
        let heap = QuotaAlloc {
            inner: LockedHeap::empty(),
        };
        // SAFETY: the arena outlives `heap` and is used by nothing else.
        unsafe {
            heap.inner
                .lock()
                .init(arena.as_mut_ptr().cast(), arena.len() * 8)
        };
        cell_quota::register(CellId(OWNER as u64), 4096);
        cell_quota::register(CellId(OTHER as u64), 4096);
        let small = Layout::from_size_align(24, 8).unwrap();
        let aligned = Layout::from_size_align(40, 64).unwrap();

        // SAFETY: every block below is freed once with the layout it was allocated with.
        unsafe {
            // Freed while another Cell is current: the original owner is refunded.
            set_current_cell_id(OWNER);
            let owned = heap.alloc(small);
            set_current_cell_id(OTHER);
            let other = heap.alloc(aligned);
            assert!(!owned.is_null() && !other.is_null());
            assert_eq!(
                other as usize % 64,
                0,
                "payload keeps the requested alignment"
            );
            assert_eq!((in_use(OWNER), in_use(OTHER)), (24, 40));
            heap.dealloc(owned, small);
            assert_eq!((in_use(OWNER), in_use(OTHER)), (0, 40));

            // Charged to (OWNER, gen1); OWNER re-registered as gen2 before the free.
            set_current_cell_id(OWNER);
            let stale = heap.alloc(small);
            cell_quota::register(CellId(OWNER as u64), 4096);
            let fresh = heap.alloc(aligned);
            assert_eq!(in_use(OWNER), 40);
            heap.dealloc(stale, small);
            assert_eq!(in_use(OWNER), 40, "gen1 free must not refund gen2");

            // Kernel-context blocks stay uncharged wherever they are freed.
            set_current_cell_id(0);
            let kernel = heap.alloc(small);
            set_current_cell_id(OWNER);
            heap.dealloc(kernel, small);
            assert_eq!(in_use(OWNER), 40);

            heap.dealloc(fresh, aligned);
            heap.dealloc(other, aligned);
        }
        assert_eq!((in_use(OWNER), in_use(OTHER)), (0, 0));
        assert_eq!(
            heap.inner.lock().used(),
            0,
            "tagged layouts round-trip exactly"
        );

        set_current_cell_id(previous);
        cell_quota::deregister(CellId(OWNER as u64));
        cell_quota::deregister(CellId(OTHER as u64));
    }
}
