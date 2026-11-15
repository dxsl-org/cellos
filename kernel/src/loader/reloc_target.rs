//! Relocation target validation against the newly mapped Cell pages.

use super::elf::LoadedPage;
use crate::memory::paging::PAGE_SIZE;
use types::{VAddr, ViError, ViResult};

/// Resolve one relative relocation without permitting arithmetic wraparound or
/// a write outside one page allocated for the Cell currently being prepared.
///
/// Returns the kernel alias of the patched word (its frame through
/// `phys_to_virt`, never the cell's own VA, which is a USER page: x86 SMAP and
/// AArch64 EL2 AP[1] refuse a kernel write there) and the value to store.
pub(crate) fn relative_word(
    base: VAddr,
    offset: u64,
    addend: i64,
    owned_pages: &[LoadedPage],
) -> ViResult<(VAddr, usize)> {
    let offset = usize::try_from(offset).map_err(|_| ViError::InvalidInput)?;
    let patch = base.checked_add(offset).ok_or(ViError::InvalidInput)?;
    let negative = addend.is_negative();
    let addend = usize::try_from(addend.unsigned_abs()).map_err(|_| ViError::InvalidInput)?;
    let value = if negative {
        base.checked_sub(addend)
    } else {
        base.checked_add(addend)
    }
    .ok_or(ViError::InvalidInput)?;
    let patch_end = patch
        .checked_add(core::mem::size_of::<usize>())
        .ok_or(ViError::InvalidInput)?;
    let page = owned_pages
        .iter()
        .find(|page| {
            page.va <= patch
                && page
                    .va
                    .checked_add(PAGE_SIZE)
                    .is_some_and(|page_end| patch_end <= page_end)
        })
        .ok_or(ViError::InvalidInput)?;
    let alias = crate::memory::frame::phys_to_virt(page.frame + (patch - page.va));
    Ok((alias, value))
}
