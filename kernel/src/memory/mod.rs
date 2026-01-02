



pub mod frame;
pub mod paging;

#[cfg(not(feature = "std"))]
use linked_list_allocator::LockedHeap;

#[cfg(not(feature = "std"))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[cfg(not(feature = "std"))]
pub fn init_heap() {
    unsafe {
        extern "C" {
            static mut __heap_start: u8;
            static mut __heap_end: u8;
        }
        
        let heap_start = &mut __heap_start as *mut u8;
        let heap_end = &mut __heap_end as *mut u8;
        let heap_size = heap_end as usize - heap_start as usize;
        
        ALLOCATOR.lock().init(heap_start, heap_size);
        log::info!("Memory: Heap Initialized (Start: {:p}, Size: {} bytes)", heap_start, heap_size);
    }
}

pub fn init() {
    log::info!("Memory Manager: Initializing...");
    #[cfg(not(feature = "std"))]
    init_heap();
    
    frame::init();
}
