// SPDX-License-Identifier: MPL-2.0
//! `icache`: publishing loaded code to instruction fetch.
//!
//! AArch64 has non-coherent data and instruction caches; the maintenance is
//! broadcast to the inner-shareable domain, so no per-PE request is served.

use crate::memory::paging::Flags;
use types::{VAddr, ViError, ViResult};

/// Nothing to serve: the code publisher's cache maintenance already reached
/// every PE.
pub(crate) fn service_sync_request(_hart_id: usize) {}

/// Publish the loader's final stores to the executable `pages` before WRITE is
/// removed and before another PE can dequeue the task. Relocations are the
/// final writes to executable pages.
///
/// # Errors
/// `ViError::InvalidInput` when an executable page is no longer mapped.
pub(crate) fn sync_loaded_code(pages: &[(VAddr, Flags)], cell: &str) -> ViResult<()> {
    for &(va, flags) in pages {
        if flags.bits() & Flags::EXECUTE != 0 {
            let phys = crate::memory::paging::virt_to_phys(va).ok_or_else(|| {
                log::error!(
                    "[wx] '{}' executable page 0x{:X} disappeared before cache sync",
                    cell,
                    va
                );
                ViError::InvalidInput
            })?;
            let data_va = crate::memory::frame::phys_to_virt(phys);
            // SAFETY: `data_va` is the physical alias used by segment copy and
            // relocation; `va` maps that same page for the cell's instruction
            // fetch. Both page-sized ranges are mapped and cannot overflow.
            unsafe {
                hal::sync_instruction_cache(data_va, va, crate::memory::paging::PAGE_SIZE);
            }
        }
    }
    Ok(())
}
