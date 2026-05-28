//! Heap allocator for ViOS kernel.

use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the heap
///
/// # Safety
/// This function must be called only once and with a valid memory region.
pub unsafe fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
}

/// Allocator error handler
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    log::error!("allocation error: {:?}", layout);
    // Panic recovery is not possible for OOM, but we loop to avoid double-panics
    // if the panic handler tries to allocate.
    loop {
        // Halt CPU
        unsafe { core::arch::asm!("wfi") };
    }
}
