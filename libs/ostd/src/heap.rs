use core::alloc::{GlobalAlloc, Layout};

pub struct SimpleAllocator;

// Static heap memory (64KB)
// Using 64KB for now. Should be large enough for basic shell commands.
// Must be aligned to 16 bytes for safe allocation.
#[repr(align(16))]
struct HeapMemory([u8; 65536]);
static mut HEAP_MEM: HeapMemory = HeapMemory([0; 65536]);
static HEAP_OFFSET: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

unsafe impl GlobalAlloc for SimpleAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        
        // Simple atomic bump allocation (thread-safeish)
        loop {
            let offset = HEAP_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
            let heap_start = core::ptr::addr_of!(HEAP_MEM) as usize;
            let current_ptr = heap_start + offset;
            
            let aligned_ptr = (current_ptr + align - 1) & !(align - 1);
            let padding = aligned_ptr - current_ptr;
            let new_offset = offset + padding + size;
            
            if new_offset > 65536 {
                return core::ptr::null_mut();
            }
            
            if HEAP_OFFSET.compare_exchange(offset, new_offset, core::sync::atomic::Ordering::Relaxed, core::sync::atomic::Ordering::Relaxed).is_ok() {
                return aligned_ptr as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // No-op: Leak memory (Bump allocator)
    }
}

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator;

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    crate::io::println("OOM: Allocation failed!");
    crate::syscall::sys_yield();
    loop {}
}
