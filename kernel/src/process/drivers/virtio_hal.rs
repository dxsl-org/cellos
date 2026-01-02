use virtio_drivers::{BufferDirection, Hal, PhysAddr};
use core::alloc::Layout;
use core::ptr::NonNull;

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let layout = Layout::from_size_align(pages * 4096, 4096).unwrap();
        unsafe {
            let ptr = alloc::alloc::alloc(layout);
            if ptr.is_null() {
                // In generic HAL, failure might panic or return 0? 
                // NonNull cannot be null. This assumes alloc succeeds.
                // For now, we panic on OOM.
                panic!("VirtIO HAL: DMA Allocation Failed (OOM)");
            }
            core::ptr::write_bytes(ptr, 0, layout.size()); // Zero memory
            
            let paddr = ptr as usize; // Identity mapping
            (paddr, NonNull::new_unchecked(ptr))
        }
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let layout = Layout::from_size_align(pages * 4096, 4096).unwrap();
        unsafe {
            alloc::alloc::dealloc(paddr as *mut u8, layout);
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("MMIO Address is 0")
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        // Identity mapping: Virtual Address IS Physical Address
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        vaddr
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        // Nothing to do for identity mapping
    }
}
