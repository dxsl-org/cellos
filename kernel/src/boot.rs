//! Boot protocol interfaces.

use crate::*;
#[cfg(any(
    target_arch = "riscv64",
    all(
        target_arch = "aarch64",
        any(feature = "board-rpi3", feature = "board-rpi4")
    )
))]
use cellos_boards::MemoryRangeKind;
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "riscv64")]
mod dtb_memory;

pub mod limine;

/// Select the firmware DTB once so every early-boot consumer sees the same tree.
pub fn effective_dtb(entry_dtb: usize) -> usize {
    limine::get_dtb_ptr().unwrap_or(entry_dtb)
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

// RISC-V fallback RAM comes from the one selected board descriptor. Firmware
// DTB/Limine data remains authoritative whenever it is available.
#[cfg(target_arch = "riscv64")]
const SELECTED_RISCV64_BOARD: &cellos_boards::BoardDescriptor =
    crate::board::selected_riscv64_board();
#[cfg(any(
    target_arch = "riscv64",
    all(
        target_arch = "aarch64",
        any(feature = "board-rpi3", feature = "board-rpi4")
    )
))]
const fn fallback_memory_type(kind: MemoryRangeKind) -> MemoryType {
    match kind {
        MemoryRangeKind::Bootloader => MemoryType::Bootloader,
        MemoryRangeKind::Kernel => MemoryType::Kernel,
        MemoryRangeKind::Usable => MemoryType::Usable,
        MemoryRangeKind::Reserved => MemoryType::Reserved,
    }
}
#[cfg(any(
    target_arch = "riscv64",
    all(
        target_arch = "aarch64",
        any(feature = "board-rpi3", feature = "board-rpi4")
    )
))]
const fn fallback_memory_entry(
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
#[cfg(target_arch = "riscv64")]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 3] = [
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 0),
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 1),
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 2),
];
#[cfg(target_arch = "riscv64")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: SELECTED_RISCV64_BOARD.fallback_memory[1].base,
    hhdm_offset: 0x0,
};

#[cfg(target_arch = "riscv64")]
static mut DTB_MEMORY_MAP: [MemoryMapEntry; MAX_MEMORY_MAP_ENTRIES] = [MemoryMapEntry {
    base: 0,
    length: 0,
    ty: MemoryType::Reserved,
}; MAX_MEMORY_MAP_ENTRIES];
#[cfg(target_arch = "riscv64")]
static mut DTB_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &[],
    kernel_phys_base: 0,
    hhdm_offset: 0,
};

// RISC-V 32-bit QEMU virt (128MB at 0x8000_0000, OpenSBI at 0x8000_0000, kernel at 0x8020_0000):
// SATP=0 (bare physical); no paging in Phase-31 Nano.
#[cfg(target_arch = "riscv32")]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 3] = [
    MemoryMapEntry {
        base: 0x8000_0000,
        length: 0x0020_0000,
        ty: MemoryType::Bootloader,
    }, // OpenSBI 2 MB
    MemoryMapEntry {
        base: 0x8020_0000,
        length: 0x0040_0000,
        ty: MemoryType::Kernel,
    }, // Kernel  4 MB
    MemoryMapEntry {
        base: 0x8060_0000,
        length: 0x07A0_0000,
        ty: MemoryType::Usable,
    }, // Usable 122 MB
];
#[cfg(target_arch = "riscv32")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x8020_0000,
    hhdm_offset: 0x0,
};

// AArch64 RPi 3 (BCM2837): kernel at 0x80000, RAM below 0x3F000000 MMIO.
// VideoCore firmware loads kernel8.img at 0x80000; GPU reserves top 64 MiB but
// on QEMU raspi3b with -m 1G the full range below the peripheral base is usable.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
const DEFAULT_RPI3_BOARD: &cellos_boards::BoardDescriptor = crate::board::default_rpi3_board();
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    fallback_memory_entry(DEFAULT_RPI3_BOARD, 0),
    fallback_memory_entry(DEFAULT_RPI3_BOARD, 1),
];
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: DEFAULT_RPI3_BOARD.fallback_memory[0].base,
    hhdm_offset: 0x0,
};

#[cfg(all(target_arch = "aarch64", feature = "board-rpi4"))]
const DEFAULT_RPI4_BOARD: &cellos_boards::BoardDescriptor = crate::board::default_rpi4_board();
#[cfg(all(target_arch = "aarch64", feature = "board-rpi4"))]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    fallback_memory_entry(DEFAULT_RPI4_BOARD, 0),
    fallback_memory_entry(DEFAULT_RPI4_BOARD, 1),
];
#[cfg(all(target_arch = "aarch64", feature = "board-rpi4"))]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: DEFAULT_RPI4_BOARD.boot.kernel_load_base,
    hhdm_offset: 0x0,
};

// AArch64 QEMU virt (256MB at 0x4000_0000, kernel loaded at 0x4008_0000):
// MMIO below 0x4000_0000 is mapped by init_kernel_paging; RAM regions only here.
//
// The kernel span is measured at RUNTIME from the linker's `__stack_top`, not
// hardcoded: EMBEDDED_OVERRIDE images make the binary size unbounded (the
// hypervisor build embeds Alpine and reaches 60 MB). The previous fixed 32 MB
// boundary let the frame allocator re-issue everything past it as free RAM —
// the kernel then overwrote the tail of its own embedded VIFS1 image and died
// in a recursive same-EL sync-abort storm (PC pinned at vt_sync_spx) the
// moment the corrupted FAT was walked.
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
const QEMU_ARM_VIRT_BOARD: &cellos_boards::BoardDescriptor =
    crate::board::default_qemu_arm_virt_board();
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
static mut FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    MemoryMapEntry {
        base: cellos_boards::qemu_virt_aarch64::FALLBACK_KERNEL.base as usize,
        length: 0, // patched by fallback_boot_info()
        ty: MemoryType::Kernel,
    },
    MemoryMapEntry {
        base: 0,
        length: 0, // patched by fallback_boot_info()
        ty: MemoryType::Usable,
    },
];
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
static mut FALLBACK_BOOT_INFO_RUNTIME: SimpleBootInfo = SimpleBootInfo {
    memory_map: &[],
    kernel_phys_base: QEMU_ARM_VIRT_BOARD.boot.kernel_load_base,
    hhdm_offset: 0x0,
};
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
static FALLBACK_DTB_RAM_START: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
static FALLBACK_DTB_RAM_END: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub fn fallback_dtb_ram_range() -> (usize, usize) {
    (
        FALLBACK_DTB_RAM_START.load(Ordering::Relaxed),
        FALLBACK_DTB_RAM_END.load(Ordering::Relaxed),
    )
}

/// Fallback boot info with the kernel region sized from the linker end symbol.
///
/// # Panics
/// Never — falls back to a 32 MB span only if `__stack_top` resolves below the
/// RAM base (impossible with the current linker script).
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub fn fallback_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    use core::ptr::{addr_of, addr_of_mut};
    extern "C" {
        /// Highest kernel address (end of .bss.stack) — linker-aarch64.ld.
        static __stack_top: u8;
    }
    const RAM_BASE: usize = cellos_boards::qemu_virt_aarch64::FALLBACK_KERNEL.base as usize;
    #[cfg(feature = "qemu-virt-1g")]
    const FALLBACK_RAM_END: usize = 0x8000_0000;
    #[cfg(not(feature = "qemu-virt-1g"))]
    const FALLBACK_RAM_END: usize = (cellos_boards::qemu_virt_aarch64::FALLBACK_USABLE.base
        + cellos_boards::qemu_virt_aarch64::FALLBACK_USABLE.size)
        as usize;
    const ALIGN_2M: usize = 0x20_0000;
    let dtb_ram = if dtb != 0 {
        // SAFETY: QEMU/firmware passes a valid FDT pointer in x0; Fdt validates
        // its header before any node traversal.
        unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }
            .ok()
            .and_then(|tree| {
                tree.memory().regions().find_map(|region| {
                    let start = region.starting_address as usize;
                    region
                        .size
                        .and_then(|size| start.checked_add(size))
                        .map(|end| (start, end))
                })
            })
    } else {
        None
    };
    if let Some((start, end)) = dtb_ram {
        FALLBACK_DTB_RAM_START.store(start, Ordering::Relaxed);
        FALLBACK_DTB_RAM_END.store(end, Ordering::Relaxed);
    }
    let ram_end = match dtb_ram {
        Some((start, end)) if start <= RAM_BASE && end > RAM_BASE => end,
        _ => FALLBACK_RAM_END,
    };
    // SAFETY: single-hart early boot; the statics are written exactly once here
    // before any other reader exists, then only shared immutably.
    unsafe {
        let end = addr_of!(__stack_top) as usize;
        let kernel_len = if end > RAM_BASE {
            (end - RAM_BASE + ALIGN_2M - 1) & !(ALIGN_2M - 1)
        } else {
            0x200_0000
        };
        *addr_of_mut!(FALLBACK_MEMORY_MAP) = [
            MemoryMapEntry {
                base: RAM_BASE,
                length: kernel_len,
                ty: MemoryType::Kernel,
            },
            MemoryMapEntry {
                base: RAM_BASE + kernel_len,
                length: ram_end.saturating_sub(RAM_BASE + kernel_len),
                ty: MemoryType::Usable,
            },
        ];
        (*addr_of_mut!(FALLBACK_BOOT_INFO_RUNTIME)).memory_map =
            core::slice::from_raw_parts(addr_of!(FALLBACK_MEMORY_MAP) as *const MemoryMapEntry, 2);
        &*addr_of!(FALLBACK_BOOT_INFO_RUNTIME)
    }
}

/// Fallback boot info — static map on arches whose images stay small.
///
/// Only aarch64 QEMU-virt sizes its kernel region at runtime (see above);
/// the other fallbacks keep their audited static spans.
#[cfg(target_arch = "riscv64")]
pub fn fallback_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    use core::ptr::{addr_of, addr_of_mut};
    extern "C" {
        static __kernel_end: u8;
    }
    #[cfg(not(feature = "board-vf2"))]
    let _board = crate::board::selected();

    if dtb != 0 {
        // SAFETY: firmware supplies a mapped FDT pointer; the parser validates its header.
        if let Ok(tree) = unsafe { fdt::Fdt::from_ptr(dtb as *const u8) } {
            let kernel_base = FALLBACK_BOOT_INFO.kernel_phys_base as usize;
            let kernel_end = addr_of!(__kernel_end) as usize;
            // SAFETY: early boot publishes these statics once before other tasks exist.
            let result = unsafe {
                dtb_memory::build(
                    &tree,
                    kernel_base,
                    kernel_end,
                    &mut *addr_of_mut!(DTB_MEMORY_MAP),
                )
            };
            match result {
                Ok(count) => unsafe {
                    (*addr_of_mut!(DTB_BOOT_INFO)).memory_map = core::slice::from_raw_parts(
                        addr_of!(DTB_MEMORY_MAP) as *const MemoryMapEntry,
                        count,
                    );
                    (*addr_of_mut!(DTB_BOOT_INFO)).kernel_phys_base = kernel_base as u64;
                    return &*addr_of!(DTB_BOOT_INFO);
                },
                Err(error) => log::warn!("[boot] DTB memory map rejected: {:?}", error),
            }
        } else {
            log::warn!("[boot] invalid DTB memory map; using static fallback");
        }
    } else {
        log::warn!("[boot] no DTB memory map; using static fallback");
    }
    &FALLBACK_BOOT_INFO
}

#[cfg(not(any(
    target_arch = "riscv64",
    all(
        target_arch = "aarch64",
        not(feature = "board-rpi3"),
        not(feature = "board-rpi4")
    ),
    all(target_arch = "aarch64", feature = "board-rpi4")
)))]
pub fn fallback_boot_info(_dtb: usize) -> &'static SimpleBootInfo {
    &FALLBACK_BOOT_INFO
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi4"))]
pub fn fallback_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    if dtb == 0 {
        panic!("[boot] Raspberry Pi 4 requires a firmware DTB");
    }
    // SAFETY: VideoCore supplies the DTB pointer; the parser validates its header.
    if unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }.is_err() {
        panic!("[boot] Raspberry Pi 4 firmware DTB is invalid");
    }
    &FALLBACK_BOOT_INFO
}

// x86_64 QEMU q35 -m 256M: Limine always provides a real memory map;
// this fallback is unreachable in normal operation but must compile.
#[cfg(target_arch = "x86_64")]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    // Conventional low memory (first 640 KB)
    MemoryMapEntry {
        base: 0x0000_0000,
        length: 0x0009_FC00,
        ty: MemoryType::Usable,
    },
    // Extended memory (1 MB – 255 MB, MMIO gap excluded)
    MemoryMapEntry {
        base: 0x0010_0000,
        length: 0x0EF0_0000,
        ty: MemoryType::Usable,
    },
];
#[cfg(target_arch = "x86_64")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x0020_0000,
    hhdm_offset: 0x0,
};

// x86_32 QEMU pc -m 128M: multiboot1, kernel at 1 MiB (0x00100000).
// Bare physical (CR0.PG=0); no HHDM.
#[cfg(target_arch = "x86")]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    // Kernel region: 1 MiB → 5 MiB (4 MiB kernel window).
    MemoryMapEntry {
        base: 0x0010_0000,
        length: 0x0040_0000,
        ty: MemoryType::Kernel,
    },
    // Usable: 5 MiB → 128 MiB.
    MemoryMapEntry {
        base: 0x0050_0000,
        length: 0x07B0_0000,
        ty: MemoryType::Usable,
    },
];
#[cfg(target_arch = "x86")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x0010_0000,
    hhdm_offset: 0x0,
};

// AArch32 QEMU virt -m 256M: kernel at 0x40080000 (ARM virt load address).
// Bare physical (MMU off); PL011 UART at 0x09000000.
#[cfg(target_arch = "arm")]
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    // Kernel region: 0x4008_0000 → 0x4048_0000 (4 MiB).
    MemoryMapEntry {
        base: 0x4008_0000,
        length: 0x0040_0000,
        ty: MemoryType::Kernel,
    },
    // Usable: 0x4048_0000 → 0x5000_0000 (~188 MiB).
    MemoryMapEntry {
        base: 0x4048_0000,
        length: 0x0BB8_0000,
        ty: MemoryType::Usable,
    },
];
#[cfg(target_arch = "arm")]
pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x4008_0000,
    hhdm_offset: 0x0,
};
