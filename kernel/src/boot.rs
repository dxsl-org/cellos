//! Boot protocol interfaces.

use crate::*;

pub mod limine;

// OpenSBI boot entry point is provided by HAL
// See hal/arch/riscv/src/rv64/boot.rs

/// Bootloader information interface.
pub trait BootInfo: Send + Sync {
    /// Get memory map entries.
    fn memory_map(&self) -> &[MemoryMapEntry];
    
    /// Get kernel physical base address.
    fn kernel_base(&self) -> PhysAddr;
    
    /// Get HHDM offset.
    fn hhdm_offset(&self) -> VAddr;
    
    /// Get framebuffer info (if available).
    fn framebuffer(&self) -> Option<FramebufferInfo>;
}

/// Memory map entry.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryMapEntry {
    /// Base physical address.
    pub base: PhysAddr,
    /// Length in bytes.
    pub length: usize,
    /// Memory type.
    pub ty: MemoryType,
}

/// Memory region type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum MemoryType {
    Usable,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    Bootloader,
    Kernel,
    Framebuffer,
}

/// Framebuffer information.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FramebufferInfo {
    /// Physical address.
    pub addr: PhysAddr,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pitch (bytes per row).
    pub pitch: u32,
    /// Bits per pixel.
    pub bpp: u16,
}

// Static buffer for converted memory map entries
pub const MAX_MEMORY_MAP_ENTRIES: usize = 64;
static mut MEMORY_MAP_BUFFER: [MemoryMapEntry; MAX_MEMORY_MAP_ENTRIES] = [
    MemoryMapEntry { base: 0, length: 0, ty: MemoryType::Reserved }; MAX_MEMORY_MAP_ENTRIES
];

/// Parse bootloader information from Limine
pub fn parse_bootloader_info() -> ViResult<LimineBootInfo> {
    // Only check for memory map presence here, conversion happens below
    let _ = limine::get_memory_map().ok_or(ViError::NotFound)?;
    
    let kernel_addr = limine::get_kernel_address()
        .ok_or(ViError::NotFound)?;
    
    let hhdm_offset = limine::get_hhdm_offset()
        .ok_or(ViError::NotFound)?;
    
    // Convert memory map entries immediately
    let limine_entries = get_limine_memory_entries();
    let mut count = 0;

    unsafe {
        for (i, entry) in limine_entries.iter().enumerate() {
            if i >= MAX_MEMORY_MAP_ENTRIES {
                log::warn!("Memory map truncated, too many entries!");
                break;
            }

            let ty = match entry.entry_type {
                0 => MemoryType::Usable,                  // USABLE
                1 => MemoryType::Reserved,                // RESERVED
                2 => MemoryType::AcpiReclaimable,         // ACPI_RECLAIMABLE
                3 => MemoryType::AcpiNvs,                 // ACPI_NVS
                4 => MemoryType::BadMemory,               // BAD_MEMORY
                5 => MemoryType::Bootloader,              // BOOTLOADER_RECLAIMABLE
                6 => MemoryType::Kernel,                  // KERNEL_AND_MODULES
                7 => MemoryType::Framebuffer,             // FRAMEBUFFER
                _ => MemoryType::Reserved,
            };

            MEMORY_MAP_BUFFER[i] = MemoryMapEntry {
                base: entry.base as usize,
                length: entry.length as usize,
                ty,
            };
            count += 1;
        }
    }
    
    Ok(LimineBootInfo {
        memory_map: unsafe { &MEMORY_MAP_BUFFER[..count] },
        kernel_phys_base: kernel_addr.physical_base,
        kernel_virt_base: kernel_addr.virtual_base,
        hhdm_offset,
    })
}

/// Limine-specific boot info implementation
pub struct LimineBootInfo {
    memory_map: &'static [MemoryMapEntry],
    kernel_phys_base: u64,
    #[allow(dead_code)]
    kernel_virt_base: u64,
    hhdm_offset: u64,
}

// SAFETY: LimineBootInfo contains only static references or processed static data
unsafe impl Send for LimineBootInfo {}
unsafe impl Sync for LimineBootInfo {}

impl BootInfo for LimineBootInfo {
    fn memory_map(&self) -> &[MemoryMapEntry] {
        self.memory_map
    }
    
    fn kernel_base(&self) -> PhysAddr {
        self.kernel_phys_base as usize
    }
    
    fn hhdm_offset(&self) -> VAddr {
        self.hhdm_offset as usize
    }
    
    fn framebuffer(&self) -> Option<FramebufferInfo> {
        limine::get_framebuffer().and_then(|fb_response| {
            if fb_response.framebuffer_count == 0 {
                return None;
            }
            
            unsafe {
                let fb_ptr = *fb_response.framebuffers;
                if fb_ptr.is_null() {
                    return None;
                }
                let fb = &*fb_ptr;
                
                Some(FramebufferInfo {
                    addr: fb.address as usize,
                    width: fb.width as u32,
                    height: fb.height as u32,
                    pitch: fb.pitch as u32,
                    bpp: fb.bpp,
                })
            }
        })
    }
}

/// Helper to get Limine memory map entries directly
pub fn get_limine_memory_entries() -> &'static [limine::LimineMemoryMapEntry] {
    if let Some(mmap) = limine::get_memory_map() {
        unsafe {
            let entries_ptr = mmap.entries;
            let count = mmap.entry_count as usize;
            if entries_ptr.is_null() || count == 0 {
                return &[];
            }
            
            // Create slice from pointer array
            let entries_array = core::slice::from_raw_parts(entries_ptr, count);
            let first_entry_ptr = *entries_array.get(0).unwrap_or(&core::ptr::null());
            
            if first_entry_ptr.is_null() {
                return &[];
            }
            
            // Return slice of actual entries
            core::slice::from_raw_parts(first_entry_ptr, count)
        }
    } else {
        &[]
    }
}

/// Simple boot info for QEMU/OpenSBI fallback
pub struct SimpleBootInfo {
    memory_map: &'static [MemoryMapEntry],
    kernel_phys_base: u64,
    hhdm_offset: u64,
}

unsafe impl Send for SimpleBootInfo {}
unsafe impl Sync for SimpleBootInfo {}

impl BootInfo for SimpleBootInfo {
    fn memory_map(&self) -> &[MemoryMapEntry] {
        self.memory_map
    }
    
    fn kernel_base(&self) -> PhysAddr {
        self.kernel_phys_base as usize
    }
    
    fn hhdm_offset(&self) -> VAddr {
        self.hhdm_offset as usize
    }
    
    fn framebuffer(&self) -> Option<FramebufferInfo> {
        None
    }
}

// Hardcoded memory map for QEMU Virt (256MB RAM)
// 0x1000_0000 - 0x1000_8000: MMIO (UART + VirtIO)
// 0x8000_0000 - 0x8020_0000: Bootloader/Reserved (2MB)
// 0x8020_0000 - 0x8040_0000: Kernel Code/Data (~2MB reserved)
// 0x8040_0000 - 0x9000_0000: Usable RAM (252MB)
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 4] = [
    MemoryMapEntry {
        base: 0x1000_0000,
        length: 0x100_000, // 1MB covers UART and all possible VirtIO slots
        ty: MemoryType::Bootloader, // Use Bootloader type for "system reserved" MMIO
    },
    MemoryMapEntry {
        base: 0x8000_0000,
        length: 0x200_000, // 2MB
        ty: MemoryType::Bootloader,
    },
    MemoryMapEntry {
        base: 0x8020_0000,
        length: 0x400_000, // 4MB (Kernel) - Increased to ensure Stack is covered
        ty: MemoryType::Kernel,
    },
    MemoryMapEntry {
        base: 0x8060_0000, // 0x8040_0000 + 2MB
        length: 0xFA0_0000, // 252MB - 2MB
        ty: MemoryType::Usable,
    },
];

pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x8020_0000,
    hhdm_offset: 0x0, // QEMU/OpenSBI usually runs in physical identity map
};
