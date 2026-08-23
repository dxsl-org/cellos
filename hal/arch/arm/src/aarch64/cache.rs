//! AArch64 cache-maintenance helpers for generated or freshly loaded code.

/// Make recently written bytes visible to instruction fetch across VA aliases.
///
/// Cleans the data cache to the Point of Unification, then invalidates the
/// instruction cache through the VA that will execute the code. The line sizes
/// come from `CTR_EL0` so the sequence works on Cortex-A53 and newer CPUs.
///
/// # Safety
/// `data_start` and `instruction_start` must map the same physical byte range;
/// both ranges must be valid and must not overflow. Callers must finish every
/// write through `data_start` before invoking this function.
pub unsafe fn sync_instruction_cache(data_start: usize, instruction_start: usize, len: usize) {
    if len == 0 {
        return;
    }

    let ctr: u64;
    // SAFETY: CTR_EL0 is readable at EL1/EL2 and has no side effects.
    unsafe {
        core::arch::asm!("mrs {ctr}, ctr_el0", ctr = out(reg) ctr, options(nomem, nostack));
    }

    let data_line = 4usize << ((ctr >> 16) & 0xF);
    let instruction_line = 4usize << (ctr & 0xF);
    let data_end = data_start + len;
    let instruction_end = instruction_start + len;

    let mut line = data_start & !(data_line - 1);
    while line < data_end {
        // SAFETY: the caller guarantees the range is mapped; aligning down to
        // its containing cache line is required by the architecture sequence.
        unsafe {
            core::arch::asm!("dc cvau, {line}", line = in(reg) line, options(nostack));
        }
        line += data_line;
    }
    // SAFETY: orders all data-cache clean operations before I-cache invalidation.
    unsafe {
        core::arch::asm!("dsb ish", options(nostack));
    }

    line = instruction_start & !(instruction_line - 1);
    while line < instruction_end {
        // SAFETY: same mapped-range contract as the data-cache pass.
        unsafe {
            core::arch::asm!("ic ivau, {line}", line = in(reg) line, options(nostack));
        }
        line += instruction_line;
    }
    // SAFETY: completes invalidation globally before any subsequent fetch.
    unsafe {
        core::arch::asm!("dsb ish", "isb", options(nostack));
    }
}
/// Clean data cache lines covering `[start, start + len)` to Point of Coherency (PoC).
///
/// Used before initiating a device DMA read from RAM.
pub fn clean_data_cache_range(start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let dline = 64usize;
    let end = start + len;
    let mut line = start & !(dline - 1);
    while line < end {
        unsafe {
            core::arch::asm!("dc cvac, {line}", line = in(reg) line, options(nostack));
        }
        line += dline;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
    }
}

/// Invalidate data cache lines covering `[start, start + len)` from Point of Coherency (PoC).
///
/// Used after a device finishes a DMA write into RAM, before CPU reads.
pub fn invalidate_data_cache_range(start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let dline = 64usize;
    let end = start + len;
    let mut line = start & !(dline - 1);
    while line < end {
        unsafe {
            core::arch::asm!("dc ivac, {line}", line = in(reg) line, options(nostack));
        }
        line += dline;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
    }
}

/// Clean and invalidate data cache lines covering `[start, start + len)` to/from PoC.
pub fn clean_invalidate_data_cache_range(start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let dline = 64usize;
    let end = start + len;
    let mut line = start & !(dline - 1);
    while line < end {
        unsafe {
            core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
        }
        line += dline;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
    }
}
