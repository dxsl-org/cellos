// SPDX-License-Identifier: MPL-2.0
//! riscv64 boot memory: the firmware DTB's memory map, else the selected
//! board descriptor's fallback spans.

use crate::boot::{
    dtb_memory, fallback_memory_entry, MemoryMapEntry, SimpleBootInfo, DTB_BOOT_INFO,
    DTB_MEMORY_MAP,
};

// RISC-V fallback RAM comes from the one selected board descriptor. Firmware
// DTB/Limine data remains authoritative whenever it is available.
const SELECTED_RISCV64_BOARD: &cellos_boards::BoardDescriptor =
    crate::board::selected_riscv64_board();

static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 3] = [
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 0),
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 1),
    fallback_memory_entry(SELECTED_RISCV64_BOARD, 2),
];

pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: SELECTED_RISCV64_BOARD.fallback_memory[1].base,
    hhdm_offset: 0x0,
};

/// Fallback boot info — static map on arches whose images stay small.
///
/// Only aarch64 QEMU-virt sizes its kernel region at runtime
/// (`arch::aarch64::boot::firmware`);
/// the other fallbacks keep their audited static spans.
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
