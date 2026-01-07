//! Paging management for ViOS kernel.
//! 
//! Delegates to HAL for architecture-specific page table management (SV39/SV32).

use crate::*;
use crate::memory::frame::FrameAllocator;
use hal::{PageTableTrait, PageTable, PageFlags};

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTableError {
    OutOfMemory,
    InvalidAddress,
    NotSupported,
}

pub type PagingResult<T> = core::result::Result<T, PageTableError>;

// Re-export PageFlags from HAL for convenience
pub use hal::PageFlags as Flags;

use crate::sync::Spinlock;

/// Global Kernel Root Page Table Address
pub static KERNEL_ROOT: Spinlock<Option<PhysAddr>> = Spinlock::new(None);

/// Initialize the kernel page table
pub fn init_kernel_paging(allocator: &mut FrameAllocator, mmap: &[crate::boot::MemoryMapEntry]) -> PagingResult<PhysAddr> {
    // 1. Allocate root page table
    let root_frame = allocator.allocate_frame().ok_or(PageTableError::OutOfMemory)?;

    // Zero it out 
    unsafe {
        let ptr = root_frame as *mut u8;
        core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
    }
    
    // Create generic PageTable wrapper at the physical address
    let root_table = unsafe { &mut *(root_frame as *mut PageTable) };
    
    // 2. Identity map all usable memory and kernel sections
    for entry in mmap {
        let flags = match entry.ty {
            crate::boot::MemoryType::Usable => {
                PageFlags::from_bits(PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::DIRTY)
            },
            crate::boot::MemoryType::Kernel => {
                PageFlags::from_bits(PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::DIRTY)
            },
            crate::boot::MemoryType::Bootloader => {
                PageFlags::from_bits(PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY)
            },
            crate::boot::MemoryType::Framebuffer => {
                PageFlags::from_bits(PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY)
            },
            _ => continue,
        };

        // Identity map this region using HAL
        let mut alloc_closure = || allocator.allocate_frame();
        
        root_table.identity_map(entry.base, entry.base + entry.length, flags, &mut alloc_closure)
             .map_err(|_| PageTableError::OutOfMemory)?; 
    }



    // Store globally
    *KERNEL_ROOT.lock() = Some(root_frame);

    Ok(root_frame)
}

/// Helper to remap a range of memory with USER permissions.
/// Used for User Stacks which are allocated in Identity Map (Usable RAM).
pub fn remap_range_user(start: PhysAddr, pages: usize) {
    let mut root_guard = KERNEL_ROOT.lock();
    if let Some(root_addr) = root_guard.as_mut() {
        // We cast the physical address directly to the PageTable struct reference
        // This is valid in Identity Map which kernel uses
        let table = unsafe { &mut *(*root_addr as *mut hal::paging::PageTable) };
        
        let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        if let Some(allocator) = frame_guard.as_mut() {
            let mut alloc_closure = || allocator.allocate_frame();
            
            use hal::traits::PageTableTrait;
            // Add USER flag to allow U-mode access
            let flags = PageFlags::from_bits(PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::USER | PageFlags::ACCESSED | PageFlags::DIRTY);
            
            use hal::paging::PAGE_SIZE;
            for i in 0..pages {
                let addr = start + (i * PAGE_SIZE);
                // Identity map: Virt = Phys
                // We overwrite existing mapping with new flags
                let _ = table.map(addr, addr, flags, &mut alloc_closure).expect("Failed to map user stack page!");
            }
        }
    }
}

/// Activate virtual memory
/// 
/// # Safety
/// This function enables paging. The root table MUST contain a valid identity mapping.
pub unsafe fn activate_paging(root_table_phys: PhysAddr) {
    let root_table = &*(root_table_phys as *const PageTable);
    root_table.activate();
}

/// Map a page in the kernel address space
pub fn map_page(allocator: &mut FrameAllocator, vaddr: VAddr, paddr: PhysAddr, flags: Flags) -> PagingResult<()> {
    let root_lock = KERNEL_ROOT.lock();
    if let Some(root_phys) = *root_lock {
        let root_table = unsafe { &mut *(root_phys as *mut PageTable) };
        // Allocator is passed in, so we don't lock here.
        
        let mut alloc_closure = || allocator.allocate_frame();
        
        root_table.map(vaddr, paddr, flags, &mut alloc_closure)
             .map_err(|_| PageTableError::OutOfMemory)?;
        Ok(())
    } else {
        Err(PageTableError::NotSupported) // Paging not initialized
    }
}
