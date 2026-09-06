#![cfg(target_os = "none")]

//! Cell heap allocator — a real freeing allocator (linked-list free list).
//!
//! Replaces the former bump allocator whose `dealloc` was a no-op: that leaked
//! EVERY allocation, so any long-running cell (shell, services) inexorably
//! exhausted its 4 MiB arena and then store-faulted on the first failed
//! allocation — a guaranteed slow death, the opposite of "never-die". This wraps
//! `linked_list_allocator` (the same crate the kernel heap uses) so transient
//! `String`/`Vec` allocations are actually reclaimed and a cell can run forever.
//!
//! ## Why `static mut Heap` and not `LockedHeap`
//! Every ViCell cell is single-task / single-hart, so allocator entries never
//! overlap and no lock is needed. Crucially, `LockedHeap` embeds a spinlock whose
//! atomic write-back FAULTS in a cell: the const-initialised allocator static lands
//! in the ELF's RELRO (read-only) segment, and an `amoor.w` to a read-only page
//! traps (scause=0xf). A zero-sized `CellAllocator` plus mutable state in `.bss`
//! (`static mut`) sidesteps that entirely — there is no writable allocator static
//! to be placed read-only.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{addr_of_mut, null_mut, NonNull};
use linked_list_allocator::Heap;

/// Default per-cell heap size: 512 KiB gives ample headroom for normal cells
/// while keeping footprint minimal. Cells needing larger heaps (e.g. VFS, compositor)
/// can initialize their own static arena via `declare_custom_heap!()`.
const HEAP_SIZE: usize = 512 * 1024;
/// Backing arena for the cell heap. 16-byte aligned so the allocator can satisfy
/// the largest natural alignment without wasting the first bytes.
#[repr(align(16))]
#[allow(dead_code)] // reason: backing bytes are accessed via addr_of_mut!(HEAP_MEM) raw-pointer cast, not field syntax
struct HeapArena([u8; HEAP_SIZE]);
static mut HEAP_MEM: HeapArena = HeapArena([0; HEAP_SIZE]);

/// Free-list allocator state. `static mut` (writable `.bss`), serialized solely by
/// the single-task execution model — see the module note on why no lock is used.
static mut HEAP: Heap = Heap::empty();
static mut HEAP_INIT: bool = false;

/// Initialize the cell heap with a custom static arena before any allocation occurs.
/// If called before the first allocation, the default `HEAP_MEM` is never touched.
pub unsafe fn init_custom_heap_raw(ptr: *mut u8, len: usize) {
    let heap = &mut *addr_of_mut!(HEAP);
    heap.init(ptr, len);
    HEAP_INIT = true;
}

#[macro_export]
macro_rules! declare_custom_heap {
    ($bytes:expr) => {
        #[repr(align(16))]
        struct __CustomHeapArena([u8; $bytes]);
        static mut __CUSTOM_HEAP_ARENA: __CustomHeapArena = __CustomHeapArena([0; $bytes]);

        #[inline(always)]
        pub fn init_custom_heap() {
            unsafe {
                $crate::heap::init_custom_heap_raw(
                    core::ptr::addr_of_mut!(__CUSTOM_HEAP_ARENA.0) as *mut u8,
                    $bytes,
                );
            }
        }
    };
}

/// Zero-sized handle; all real state lives in the `static mut`s above so this
/// `#[global_allocator]` static carries no writable data that could land read-only.
struct CellAllocator;
// SAFETY: single-task cells never enter the allocator concurrently, so the
// unsynchronised access to the `static mut` heap state is race-free. Allocation
// correctness is delegated to the well-tested `linked_list_allocator::Heap`.
unsafe impl GlobalAlloc for CellAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap = &mut *addr_of_mut!(HEAP);
        if !HEAP_INIT {
            // SAFETY: HEAP_MEM is a 'static, 16-byte-aligned arena owned solely by the
            // heap; init runs exactly once (single-task → no race on HEAP_INIT).
            heap.init(addr_of_mut!(HEAP_MEM) as *mut u8, HEAP_SIZE);
            HEAP_INIT = true;
        }
        heap.allocate_first_fit(layout)
            .map_or(null_mut(), |p| p.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(p) = NonNull::new(ptr) {
            let heap = &mut *addr_of_mut!(HEAP);
            heap.deallocate(p, layout);
        }
    }
}

#[cfg(target_os = "none")]
#[global_allocator]
static ALLOCATOR: CellAllocator = CellAllocator;

/// Allocation-failure handler. We are genuinely out of heap (4 MiB of LIVE
/// objects), so in-process recovery is hopeless. Rather than hang — which would
/// leave a paralyzed-but-alive cell the supervisor cannot detect — log via the
/// raw syscall (no allocation) and exit abnormally so the supervisor restarts the
/// cell with a fresh heap. That is the "never-die" response to OOM.
#[cfg(all(not(test), target_os = "none"))]
#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    let _ = crate::syscall::sys_log("OOM: cell heap exhausted — size=");
    let mut buf = [0u8; 32];
    let mut n = layout.size();
    let mut i = 0;
    if n == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        let mut tmp = [0u8; 32];
        let mut j = 0;
        while n > 0 {
            tmp[j] = b'0' + (n % 10) as u8;
            n /= 10;
            j += 1;
        }
        while j > 0 {
            j -= 1;
            buf[i] = tmp[j];
            i += 1;
        }
    }
    buf[i] = b'\n';
    if let Ok(s) = core::str::from_utf8(&buf[..=i]) {
        let _ = crate::syscall::sys_log(s);
    }
    crate::syscall::sys_exit(0xEE); // non-zero = abnormal → supervisor restarts it
}
