use alloc::vec::Vec;

use super::harness::failure_is_armed;

#[derive(Debug)]
struct MappingProbe {
    va: usize,
    frame: usize,
    pte: u64,
}

static AP02_MAPPINGS: crate::sync::Spinlock<Option<Vec<MappingProbe>>> =
    crate::sync::Spinlock::new(None);

pub(super) fn arm_ap02() {
    *AP02_MAPPINGS.lock() = None;
}

/// Snapshot unpublished segment translations while their rollback owner remains
/// live.
pub(super) fn observe_unpublished(segments: &crate::task::stack::CellSegments) {
    observe_pages(segments.unpublished_pages().iter().copied());
}

/// Capture live page translations at the loader's partial-image boundary. This
/// is deliberately called before the AP-02 checkpoint, because rejection there
/// unwinds `LoadedPage`s before they can become `CellSegments`.
pub(super) fn observe_unpublished_pages(pages: &[crate::loader::elf::LoadedPage]) {
    observe_pages(pages.iter().map(|page| (page.va, page.frame)));
}

fn observe_pages(pages: impl Iterator<Item = (usize, usize)>) {
    if !failure_is_armed("AP-02") {
        return;
    }
    let mappings = pages
        .map(|(va, frame)| {
            let translation = crate::memory::paging::translation_probe(va)?;
            (translation.phys == frame && translation.pte != 0).then_some(MappingProbe {
                va,
                frame,
                pte: translation.pte,
            })
        })
        .collect::<Option<Vec<_>>>();
    if mappings.is_some() {
        crate::memory::tlb_shootdown::begin_test_flush_observation();
    }
    *AP02_MAPPINGS.lock() = mappings;
}

/// The same hardware walk that saw each unpublished PTE must observe no leaf
/// after its rollback owner flushes and drops every mapped page.
pub(super) fn ap02_cleanup_complete() -> bool {
    let Some(mappings) = AP02_MAPPINGS.lock().take() else {
        return false;
    };
    let tlb_absent = mappings
        .iter()
        .all(|mapping| crate::memory::tlb_shootdown::test_flush_observed(mapping.va));
    crate::memory::tlb_shootdown::finish_test_flush_observation();
    !mappings.is_empty()
        && tlb_absent
        && mappings.into_iter().all(|mapping| {
            mapping.pte != 0
                && crate::memory::paging::translation_probe(mapping.va).is_none()
                && crate::memory::paging::virt_to_phys(mapping.va) != Some(mapping.frame)
        })
}
