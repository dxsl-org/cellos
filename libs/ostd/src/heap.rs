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
//! ## Why a `static mut` and a yielding lock
//! A cell may run several tasks (`task::spawn`) that share this heap and can be
//! preempted, or run on other harts, mid-operation; [`CellHeap`] serialises every
//! allocator entry with a lock whose waiters yield so a preempted holder always
//! gets to finish (see `locked-heap.rs`). The state is a `static mut` in writable
//! `.bss`, never a plain `static`: a const-initialised allocator static once
//! landed in the ELF's RELRO (read-only) segment, and the lock's first atomic
//! write-back to a read-only page trapped (scause=0xf). A zero-sized
//! `CellAllocator` keeps the `#[global_allocator]` static free of writable data.

use crate::locked_heap::CellHeap;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{addr_of, addr_of_mut};

/// Default per-cell heap size: 512 KiB gives ample headroom for normal cells
/// while keeping footprint minimal. Cells needing larger heaps (e.g. VFS, compositor)
/// can initialize their own static arena via `declare_custom_heap!()`.
const HEAP_SIZE: usize = 1024 * 1024;
/// Backing arena for the cell heap. 16-byte aligned so the allocator can satisfy
/// the largest natural alignment without wasting the first bytes.
#[repr(align(16))]
#[allow(dead_code)] // reason: backing bytes are accessed via addr_of_mut!(HEAP_MEM) raw-pointer cast, not field syntax
struct HeapArena([u8; HEAP_SIZE]);
static mut HEAP_MEM: HeapArena = HeapArena([0; HEAP_SIZE]);

/// Free-list allocator state behind its lock; see the module note on placement.
static mut HEAP: CellHeap = CellHeap::empty();

/// Shared view of the heap. Only ever borrowed shared: all mutation goes through
/// the interior lock, so no `&mut` alias of the static is ever formed.
fn cell_heap() -> &'static CellHeap {
    // SAFETY: see above — the static is never reached by `&mut`.
    unsafe { &*addr_of!(HEAP) }
}

/// Lock waiter's relax step: give the hart to the (possibly preempted) holder.
/// Must not allocate, which `sys_yield` does not.
fn yield_to_holder() {
    crate::syscall::sys_yield();
}

/// Initialize the cell heap with a custom static arena before any allocation occurs.
/// If called before the first allocation, the default `HEAP_MEM` is never touched.
///
/// # Safety
///
/// `ptr` must point to `len` bytes of memory that stays mapped and exclusively owned by this cell
/// for the rest of the program: the allocator hands out sub-slices of exactly that region, and a
/// second call (or any earlier allocation) would leave live blocks outside the new arena.
pub unsafe fn init_custom_heap_raw(ptr: *mut u8, len: usize) {
    cell_heap().init(ptr, len, yield_to_holder);
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
// SAFETY: `CellHeap` serialises every entry under its lock, so tasks of one cell
// never touch the free list concurrently. Allocation correctness is delegated to
// the well-tested `linked_list_allocator::Heap`.
unsafe impl GlobalAlloc for CellAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: HEAP_MEM is a 'static, 16-byte-aligned arena owned solely by the
        // heap; `CellHeap::alloc` runs this lazy init once, under the lock.
        cell_heap().alloc(layout, yield_to_holder, || {
            (addr_of_mut!(HEAP_MEM) as *mut u8, HEAP_SIZE)
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        cell_heap().dealloc(ptr, layout, yield_to_holder);
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
