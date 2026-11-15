//! Heap allocator for ViCell kernel.
//!
//! Wraps `linked_list_allocator::LockedHeap` in a `QuotaAlloc` that charges
//! every allocation to the hart's explicit allocation charge, set only by a
//! [`ChargeScope`]. Outside every scope it is the kernel, which is unlimited.
//! A Cell that exceeds its quota receives a null pointer from `alloc()` — no
//! panic, no system halt. Each block records the quota owner it was charged
//! to, and `dealloc` refunds that owner rather than whichever charge is open
//! at free time.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use linked_list_allocator::LockedHeap;

#[cfg(feature = "p01-trace")]
#[path = "heap/p01_owner.rs"]
pub(crate) mod p01_owner;

/// Why the allocator last handed back null: the Cell's quota, or the heap itself.
///
/// Counted here rather than logged: the log path may allocate, and an allocator
/// that logs on failure recurses. The counters are read outside `alloc` (the
/// spawn-refusal paths) so a capacity run can say *which* resource bound.
static NULL_FROM_QUOTA: AtomicUsize = AtomicUsize::new(0);
static NULL_FROM_HEAP: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "p01-trace")]
static P01_EPOCH: AtomicUsize = AtomicUsize::new(1);
#[cfg(feature = "p01-trace")]
static P01_PEAK: AtomicUsize = AtomicUsize::new(0);

// Called only with the existing heap lock held; never calls usage or allocates.
#[cfg(feature = "p01-trace")]
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
    let sp = crate::arch::cpu::oom_stack_pointer();
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
        #[cfg(feature = "p01-trace")] origin: Option<p01_owner::Provenance>,
    ) -> *mut u8 {
        let charge = crate::sched::hart_local::alloc_charge();
        let Some(owner) = crate::memory::cell_quota::charge_owner(charge, layout.size()) else {
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
        #[cfg(not(feature = "p01-trace"))]
        let ptr = tagged.map_or(core::ptr::null_mut(), |tagged| self.inner.alloc(tagged));
        #[cfg(feature = "p01-trace")]
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
            #[cfg(feature = "p01-trace")]
            None,
        )
    }

    #[cfg(feature = "p01-trace")]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let _irq = HeapIrqMask::enter();
        let ptr = self.alloc_impl(layout, None);
        if !ptr.is_null() {
            ptr.write_bytes(0, layout.size());
        }
        ptr
    }

    #[cfg(feature = "p01-trace")]
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
        #[cfg(not(feature = "p01-trace"))]
        self.inner.dealloc(ptr, tagged);
        #[cfg(feature = "p01-trace")]
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

/// The allocation charge open on this hart, restored on drop.
///
/// Only three kinds of code open one: syscall entry (`Charge::dispatched()`),
/// an owner-directed allocation (the incarnation that owns the object), and a
/// kernel scope (`Charge::KERNEL`) for kernel bookkeeping. Every interrupt
/// entry opens [`ChargeScope::interrupt`] first, and `yield_cpu` holds a
/// kernel scope across each switch, so no scope crosses a switch or reaches an
/// interrupt handler. A scope dropped on another hart than it was opened on
/// restores that hart: the activation, not the hart, owns it.
#[must_use = "a charge scope ends when it is dropped"]
pub(crate) struct ChargeScope {
    saved: crate::memory::cell_quota::Charge,
}

impl ChargeScope {
    /// Charge every allocation on this hart to `charge` until drop.
    pub(crate) fn enter(charge: crate::memory::cell_quota::Charge) -> Self {
        Self {
            saved: crate::sched::hart_local::replace_alloc_charge(charge),
        }
    }

    /// The first statement of every interrupt entry: the handler allocates as
    /// the kernel, never as the interrupted context's charge.
    pub(crate) fn interrupt() -> Self {
        Self::enter(crate::memory::cell_quota::Charge::KERNEL)
    }
}

impl Drop for ChargeScope {
    fn drop(&mut self) {
        crate::sched::hart_local::replace_alloc_charge(self.saved);
    }
}

/// Run `f` with every allocation on this hart charged to `charge`.
pub(crate) fn alloc_for<R>(charge: crate::memory::cell_quota::Charge, f: impl FnOnce() -> R) -> R {
    let _scope = ChargeScope::enter(charge);
    f()
}

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

#[cfg(feature = "p01-trace")]
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

#[cfg(feature = "p01-trace")]
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

/// The owner token recorded for a live block whose payload is `size` bytes.
///
/// # Safety
/// `payload` must be a live block from this allocator, allocated with a
/// layout of exactly `size` bytes.
#[cfg(any(feature = "test-hooks", all(test, not(target_os = "none"))))]
unsafe fn owner_word(payload: *const u8, size: usize) -> usize {
    // SAFETY: the caller's contract; the tag word follows the payload.
    unsafe { payload.add(size).cast::<usize>().read_unaligned() }
}

/// Boot fixture `IRQ-ALLOC-CHARGE` (census §8.6(a)): an allocation made in
/// interrupt context while Cell A runs is not charged to A.
#[cfg(feature = "test-hooks")]
pub(crate) mod irq_charge_selftest {
    use crate::memory::cell_quota::{self, Charge, Limits, QuotaReservation};
    use core::sync::atomic::{AtomicUsize, Ordering};

    const NO_HART: usize = usize::MAX;
    const PROBE_BYTES: usize = 64;
    const PENDING: usize = 0;
    const ALLOCATED: usize = 1;
    const REFUSED: usize = 2;

    /// Hart whose next timer tick takes the probe, or `NO_HART`.
    static ARMED_HART: AtomicUsize = AtomicUsize::new(NO_HART);
    /// Hart whose ticks must not preempt the boot context until the fixture's
    /// verdict is logged, or `NO_HART`.
    static HELD_HART: AtomicUsize = AtomicUsize::new(NO_HART);
    static PROBE: AtomicUsize = AtomicUsize::new(PENDING);
    static PROBE_OWNER: AtomicUsize = AtomicUsize::new(0);

    /// Called on the boot hart before interrupts are first enabled. On AArch64
    /// the first tick after the enable switches straight into the cells, and the
    /// boot context (which runs `run`) is not scheduled again before a test
    /// image may exit; holding preemption on this hart keeps `run` ahead of them.
    pub(crate) fn hold_boot_hart() {
        HELD_HART.store(
            crate::sched::hart_local::current_hart_id(),
            Ordering::Release,
        );
    }

    /// Called by `vi_timer_tick` right after its interrupt scope opens. Takes the
    /// probe when this hart is armed. Returns whether the tick must return to the
    /// interrupted context instead of preempting it (the boot hart while held).
    pub(crate) fn observe_tick() -> bool {
        let hart = crate::sched::hart_local::current_hart_id();
        let held = HELD_HART.load(Ordering::Acquire) == hart;
        if ARMED_HART
            .compare_exchange(hart, NO_HART, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return held;
        }
        match super::try_box([0u8; PROBE_BYTES]) {
            Ok(probe) => {
                // SAFETY: `probe` is a live block of exactly PROBE_BYTES.
                let owner = unsafe { super::owner_word(probe.as_ptr(), PROBE_BYTES) };
                PROBE_OWNER.store(owner, Ordering::Relaxed);
                PROBE.store(ALLOCATED, Ordering::Release);
            }
            Err(_) => PROBE.store(REFUSED, Ordering::Release),
        }
        true
    }

    /// Run on the boot hart with interrupts enabled and the timer armed. The
    /// boot context stands in for Cell A inside a syscall: A's identity and
    /// scope are open, A's limit is 0, and the next tick on this hart takes a
    /// 64-byte probe. Nothing between opening and closing A's scope logs or
    /// allocates infallibly.
    ///
    /// The verdict on the interrupt (the lane's required `PASS`) is decided and
    /// logged before the housekeeping pass, because that `yield_cpu` may not
    /// return to this context until every cell has run. The housekeeping check
    /// logs its own line when it does return.
    pub(crate) fn run() {
        let generation = crate::task::tcb::next_cell_generation();
        let Ok(quota) = QuotaReservation::reserve_next(
            generation,
            Limits {
                memory: 0,
                guest: 0,
            },
        ) else {
            HELD_HART.store(NO_HART, Ordering::Release);
            log::error!("[selftest] IRQ-ALLOC-CHARGE: FAIL no free CellId for cell A");
            return;
        };
        let cell = Charge::new(quota.cell_id(), generation);
        let hart = crate::sched::hart_local::current_hart_id();
        let saved = (
            crate::sched::hart_local::current_cell_id(),
            crate::sched::hart_local::current_cell_generation(),
        );
        crate::sched::hart_local::set_current_cell_context(cell.cell().0 as usize, generation);
        let scope = super::ChargeScope::enter(Charge::dispatched());
        let billed_to_a = super::try_box([0u8; PROBE_BYTES]).is_err();
        PROBE.store(PENDING, Ordering::Relaxed);
        ARMED_HART.store(hart, Ordering::Release);
        let start = crate::task::system_ticks();
        let mut spins = 0u64;
        while ARMED_HART.load(Ordering::Acquire) == hart
            && crate::task::system_ticks().wrapping_sub(start) < 500
            && spins < 1 << 32
        {
            core::hint::spin_loop();
            spins += 1;
        }
        let fired = ARMED_HART.swap(NO_HART, Ordering::AcqRel) != hart;
        let restored = super::try_box([0u8; PROBE_BYTES]).is_err();
        let in_use = cell_quota::in_use(cell.cell());

        let probe = PROBE.load(Ordering::Acquire);
        let owner = PROBE_OWNER.load(Ordering::Relaxed);
        let failure = if !billed_to_a {
            Some("cell A's scope did not bill A")
        } else if !fired {
            Some("no timer tick took the probe")
        } else if probe == REFUSED {
            Some("the interrupt-context probe was refused")
        } else if probe != ALLOCATED {
            Some("the probe did not record")
        } else if owner != cell_quota::UNCHARGED_OWNER {
            Some("the probe's owner word is not the kernel's")
        } else if in_use != 0 {
            Some("cell A was charged")
        } else if !restored {
            Some("cell A's scope was not restored after the interrupt")
        } else {
            None
        };
        // Logging allocates; A's limit is 0, so it runs under a kernel scope.
        super::alloc_for(Charge::KERNEL, || match failure {
            None => log::info!(
                "[selftest] IRQ-ALLOC-CHARGE: PASS owner={:#x} in_use=0 spins={}",
                owner,
                spins
            ),
            Some(reason) => log::error!(
                "[selftest] IRQ-ALLOC-CHARGE: FAIL {} (probe={} owner={:#x} in_use={})",
                reason,
                probe,
                owner,
                in_use
            ),
        });

        HELD_HART.store(NO_HART, Ordering::Release);

        // One housekeeping pass while A's scope is open: `yield_cpu` must run it
        // as the kernel and hand A's scope back when this context resumes.
        crate::task::yield_cpu();
        let resumed_as_a = super::try_box([0u8; PROBE_BYTES]).is_err();
        drop(scope);
        crate::sched::hart_local::set_current_cell_context(saved.0, saved.1);
        let after = cell_quota::in_use(cell.cell());
        drop(quota);
        if after == 0 && resumed_as_a {
            log::info!("[selftest] IRQ-ALLOC-CHARGE housekeeping: PASS in_use=0");
        } else {
            log::error!(
                "[selftest] IRQ-ALLOC-CHARGE housekeeping: FAIL in_use={} resumed_as_a={}",
                after,
                resumed_as_a
            );
        }
    }
}

/// Allocator error handler
#[cfg(not(all(test, not(target_os = "none"))))]
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    log::error!("allocation error: {:?}", layout);
    // Structure sizes, so the fatal layout above can be matched to what asked for
    // it (the spawn path's per-cell structures are the recurring candidates).
    #[cfg(all(feature = "native-domains", cellos_wide))]
    let mapping_entry = core::mem::size_of::<crate::memory::address_space::MappingEntry>();
    #[cfg(not(all(feature = "native-domains", cellos_wide)))]
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
        let sp = crate::arch::cpu::oom_stack_pointer();
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
        crate::arch::cpu::idle();
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::memory::cell_quota::{self, Charge, MAX_CELLS};
    use crate::sched::hart_local::{
        current_cell_generation, current_cell_id, set_current_cell_context,
    };
    use types::CellId;

    const OWNER: usize = MAX_CELLS - 1;
    const OTHER: usize = MAX_CELLS - 2;

    fn charge(cell: usize, generation: u64) -> Charge {
        Charge::new(CellId(cell as u64), generation)
    }

    fn in_use(cell: usize) -> usize {
        cell_quota::in_use(CellId(cell as u64))
    }

    #[test]
    fn dealloc_refunds_the_charged_owner_incarnation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
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
        let limits = cell_quota::Limits {
            memory: 4096,
            guest: 0,
        };
        cell_quota::register(CellId(OWNER as u64), 1, limits);
        cell_quota::register(CellId(OTHER as u64), 1, limits);
        let small = Layout::from_size_align(24, 8).unwrap();
        let aligned = Layout::from_size_align(40, 64).unwrap();

        // SAFETY: every block below is freed once with the layout it was allocated with.
        unsafe {
            // Freed under another charge: the original owner is refunded.
            let owned = alloc_for(charge(OWNER, 1), || heap.alloc(small));
            let other = alloc_for(charge(OTHER, 1), || heap.alloc(aligned));
            assert!(!owned.is_null() && !other.is_null());
            assert_eq!(
                other as usize % 64,
                0,
                "payload keeps the requested alignment"
            );
            assert_eq!((in_use(OWNER), in_use(OTHER)), (24, 40));
            alloc_for(charge(OTHER, 1), || heap.dealloc(owned, small));
            assert_eq!((in_use(OWNER), in_use(OTHER)), (0, 40));

            // Charged to (OWNER, gen1); OWNER re-registered as gen2 before the free.
            let stale = alloc_for(charge(OWNER, 1), || heap.alloc(small));
            cell_quota::register(CellId(OWNER as u64), 2, limits);
            assert!(
                alloc_for(charge(OWNER, 1), || heap.alloc(small)).is_null(),
                "a retired incarnation is refused"
            );
            let fresh = alloc_for(charge(OWNER, 2), || heap.alloc(aligned));
            assert_eq!(in_use(OWNER), 40);
            heap.dealloc(stale, small);
            assert_eq!(in_use(OWNER), 40, "gen1 free must not refund gen2");

            // Kernel blocks stay uncharged wherever they are freed.
            let kernel = heap.alloc(small);
            alloc_for(charge(OWNER, 2), || heap.dealloc(kernel, small));
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

        cell_quota::deregister(CellId(OWNER as u64));
        cell_quota::deregister(CellId(OTHER as u64));
    }

    /// Census §8.6(a): an allocation in interrupt context while Cell A runs a
    /// syscall is not charged to A, and A's charge is back once it ends.
    #[test]
    fn interrupt_scope_does_not_charge_the_interrupted_cell() {
        let _guard = crate::TEST_STATE_LOCK.lock();
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
        cell_quota::register(
            CellId(OWNER as u64),
            7,
            cell_quota::Limits {
                memory: 0,
                guest: 0,
            },
        );
        let saved = (current_cell_id(), current_cell_generation());
        set_current_cell_context(OWNER, 7);
        let syscall = ChargeScope::enter(Charge::dispatched());
        let probe_layout = Layout::from_size_align(64, 8).unwrap();

        // SAFETY: the probe is freed once with the layout it was allocated with.
        unsafe {
            assert!(heap.alloc(probe_layout).is_null(), "A is at its limit");
            let probe = {
                let _interrupt = ChargeScope::interrupt();
                heap.alloc(probe_layout)
            };
            assert!(!probe.is_null(), "the interrupt allocates as the kernel");
            assert_eq!(owner_word(probe, 64), cell_quota::UNCHARGED_OWNER);
            assert_eq!(in_use(OWNER), 0);
            assert!(
                heap.alloc(probe_layout).is_null(),
                "A's scope is restored when the interrupt scope ends"
            );
            heap.dealloc(probe, probe_layout);
        }

        drop(syscall);
        set_current_cell_context(saved.0, saved.1);
        cell_quota::deregister(CellId(OWNER as u64));
    }
}
