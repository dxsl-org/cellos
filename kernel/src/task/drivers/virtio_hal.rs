use virtio_drivers::{BufferDirection, Hal, PhysAddr};
use core::alloc::Layout;
use core::ptr::NonNull;

/// VirtIO HAL Implementation for ViOS.
///
/// CAUTION: This implementation assumes Identity Mapping (Virtual Address = Physical Address)
/// for DMA regions. This is valid for the current simplistic memory model but MUST be
/// revisited if IOMMU or higher-half kernel mapping is strictly enforced for drivers.
pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let layout = Layout::from_size_align(pages * 4096, 4096).unwrap();
        unsafe {
            let ptr = alloc::alloc::alloc(layout);
            if ptr.is_null() {
                {
                    let mut serial = crate::task::drivers::uart::SERIAL.lock();
                    let _ = core::fmt::Write::write_str(&mut *serial, "[ERROR] VirtIO HAL: DMA Allocation Failed (OOM). Driver will hang.\n");
                }
                loop { core::hint::spin_loop(); }
            }
            core::ptr::write_bytes(ptr, 0, layout.size()); // Zero memory
            
            let paddr = ptr as usize; // Identity mapping
            {
                let mut serial = crate::task::drivers::uart::SERIAL.lock();
                use core::fmt::Write;
                let _ = write!(serial, "[VIRTIO] DMA Alloc {} pages at V:{:p} P:0x{:X}\n", pages, ptr, paddr);
            }
            (paddr, NonNull::new_unchecked(ptr))
        }
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let layout = Layout::from_size_align(pages * 4096, 4096).unwrap();
        unsafe {
            alloc::alloc::dealloc(paddr as usize as *mut u8, layout);
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as usize as *mut u8).expect("MMIO Address is 0")
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        // Identity mapping: Virtual Address IS Physical Address
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        vaddr as usize
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        // Nothing to do for identity mapping
    }
}
