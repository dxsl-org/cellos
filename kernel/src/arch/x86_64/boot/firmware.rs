// SPDX-License-Identifier: MPL-2.0
//! x86_64 boot memory fallback: Limine always provides a real memory map, so
//! this static map is reached only when its response is missing.

use crate::boot::{MemoryMapEntry, MemoryType, SimpleBootInfo};

pub fn fallback_boot_info(_dtb: usize) -> &'static SimpleBootInfo {
    &FALLBACK_BOOT_INFO
}

// x86_64 QEMU q35 -m 256M: Limine always provides a real memory map;
// this fallback is unreachable in normal operation but must compile.
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

pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: 0x0020_0000,
    hhdm_offset: 0x0,
};
