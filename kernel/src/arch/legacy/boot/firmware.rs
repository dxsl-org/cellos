// SPDX-License-Identifier: MPL-2.0
//! 32-bit boot memory: the bare-physical static maps.

use crate::boot::{MemoryMapEntry, MemoryType, SimpleBootInfo};

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

pub fn fallback_boot_info(_dtb: usize) -> &'static SimpleBootInfo {
    &FALLBACK_BOOT_INFO
}

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
