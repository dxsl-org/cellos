//! Boot protocol interfaces.

use crate::*;
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
use cellos_boards::MemoryRangeKind;

#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) mod dtb_memory;
#[cfg(any(target_arch = "aarch64", test))]
pub mod fdt_nops;
#[cfg(test)]
mod firmware_tree_tests;

pub mod limine;
mod platform_ready;
pub use platform_ready::*;

/// Select the firmware DTB once so every early-boot consumer sees the same tree.
///
/// AArch64 then compacts the tree's `FDT_NOP` holes and lets the board backend
/// read it (`crate::arch::boot::firmware::note_firmware_tree`), before its
/// interrupt controller is brought up and before the allocator can reuse the
/// tree's memory.
pub fn effective_dtb(entry_dtb: usize) -> usize {
    limine::get_dtb_ptr().unwrap_or(entry_dtb)
}

/// Reserve the warm-snapshot capture's scratch workspace.
///
/// The capture stages its own stack and buffers there, so they can never be
/// inside a captured run: the region is taken from the frame allocator, stays
/// allocated for the kernel's lifetime and is excluded from every inventory.
/// Call it once the frame allocator exists and before any capture can run.  It
/// is a no-op unless the build is `snapshot-qualified`, never panics, and
/// refuses (leaving no region, so every capture then refuses) rather than
/// degrading.
pub fn reserve_snapshot_scratch() {
    crate::snapshot::reserve_scratch_region_at_boot();
}

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
    MMIO,
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
static mut MEMORY_MAP_BUFFER: [MemoryMapEntry; MAX_MEMORY_MAP_ENTRIES] = [MemoryMapEntry {
    base: 0,
    length: 0,
    ty: MemoryType::Reserved,
}; MAX_MEMORY_MAP_ENTRIES];

/// Parse bootloader information from Limine
pub fn parse_bootloader_info() -> ViResult<LimineBootInfo> {
    // Only check for memory map presence here, conversion happens below
    let _ = limine::get_memory_map().ok_or(ViError::NotFound)?;

    let kernel_addr = limine::get_kernel_address().ok_or(ViError::NotFound)?;

    let hhdm_offset = limine::get_hhdm_offset().ok_or(ViError::NotFound)?;

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
                0 => MemoryType::Usable,          // USABLE
                1 => MemoryType::Reserved,        // RESERVED
                2 => MemoryType::AcpiReclaimable, // ACPI_RECLAIMABLE
                3 => MemoryType::AcpiNvs,         // ACPI_NVS
                4 => MemoryType::BadMemory,       // BAD_MEMORY
                5 => MemoryType::Bootloader,      // BOOTLOADER_RECLAIMABLE
                6 => MemoryType::Kernel,          // KERNEL_AND_MODULES
                7 => MemoryType::Framebuffer,     // FRAMEBUFFER
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
            let first_entry_ptr = *entries_array.first().unwrap_or(&core::ptr::null());

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
    pub(crate) memory_map: &'static [MemoryMapEntry],
    pub(crate) kernel_phys_base: u64,
    pub(crate) hhdm_offset: u64,
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

#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) const fn fallback_memory_type(kind: MemoryRangeKind) -> MemoryType {
    match kind {
        MemoryRangeKind::Bootloader => MemoryType::Bootloader,
        MemoryRangeKind::Kernel => MemoryType::Kernel,
        MemoryRangeKind::Usable => MemoryType::Usable,
        MemoryRangeKind::Reserved => MemoryType::Reserved,
    }
}
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) const fn fallback_memory_entry(
    board: &cellos_boards::BoardDescriptor,
    index: usize,
) -> MemoryMapEntry {
    let range = board.fallback_memory[index];
    MemoryMapEntry {
        base: range.base as usize,
        length: range.size as usize,
        ty: fallback_memory_type(range.kind),
    }
}

#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) static mut DTB_MEMORY_MAP: [MemoryMapEntry; MAX_MEMORY_MAP_ENTRIES] = [MemoryMapEntry {
    base: 0,
    length: 0,
    ty: MemoryType::Reserved,
};
    MAX_MEMORY_MAP_ENTRIES];
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) static mut DTB_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &[],
    kernel_phys_base: 0,
    hhdm_offset: 0,
};

/// The firmware tree's CPU description, as plain data: what
/// `firmware_smp::note` (aarch64) records, kept free of boot state so host tests can
/// run real firmware trees (`tests/fixtures/dtb/`) through the same parser.
#[cfg(any(target_arch = "aarch64", test))]
pub mod firmware_cpus {
    use crate::sched::smp::mpidr;

    /// One `/cpus/cpu@…` node, in tree order.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CpuNode {
        /// The CPU's `MPIDR_EL1` affinity, from `reg` (`None`: missing or
        /// malformed; see [`mpidr::reg_affinity`]).
        pub affinity: Option<u64>,
        /// Spin-table `cpu-release-addr` (`None`: absent, malformed, or the 0
        /// a tree carries until its loader fills the address in).
        pub release_addr: Option<u64>,
    }

    /// `/psci/method` (`"smc"` / `"hvc"`), or `None` when the tree has no PSCI.
    pub fn psci_method<'a>(tree: &fdt::Fdt<'a>) -> Option<&'a str> {
        tree.find_node("/psci")?
            .property("method")
            .and_then(|method| method.as_str())
    }

    /// The `/cpus/cpu@…` nodes in tree order; none when `/cpus` is missing
    /// (`fdt::Fdt::cpus` would panic on such a tree).
    pub fn cpus<'t>(tree: &'t fdt::Fdt<'_>) -> impl Iterator<Item = CpuNode> + 't {
        let cpus = tree.find_node("/cpus");
        // Devicetree default when `/cpus` omits `#address-cells`.
        let address_cells = cpus
            .and_then(|cpus| cpus.property("#address-cells"))
            .and_then(|cells| cells.as_usize())
            .unwrap_or(2);
        cpus.into_iter()
            .flat_map(|cpus| cpus.children())
            .filter(|node| node.name.split('@').next() == Some("cpu"))
            .map(move |cpu| CpuNode {
                affinity: cpu
                    .property("reg")
                    .and_then(|reg| mpidr::reg_affinity(reg.value, address_cells)),
                release_addr: cpu
                    .property("cpu-release-addr")
                    .and_then(|prop| match <[u8; 8]>::try_from(prop.value) {
                        Ok(cells) => Some(u64::from_be_bytes(cells)),
                        Err(_) => <[u8; 4]>::try_from(prop.value)
                            .ok()
                            .map(|cell| u64::from(u32::from_be_bytes(cell))),
                    })
                    .filter(|&addr| addr != 0),
            })
    }
}
