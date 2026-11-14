// SPDX-License-Identifier: MPL-2.0
//! `tlb`: TLB flush and ordering page-table stores before a shootdown.

/// Broadcast flush of every stage-1 entry: `dsb ishst; tlbi vmalle1is` (plus
/// `tlbi alle2is` when the kernel runs at EL2); `dsb sy; isb`.
///
/// Under the VHE host (`HCR_EL2.{E2H,TGE} = {1,1}`) `vmalle1is` executed at
/// EL2 already targets the EL2&0 regime the kernel and Cells share; `alle2is`
/// is issued as well, as the per-page path (`hal::paging::flush_tlb_page`)
/// adds `vae2is`, so the EL2 leg never rests on that redirection alone. It is
/// branched over below EL2, where the encoding is UNDEFINED.
#[inline(always)]
pub(crate) fn flush_all() {
    let el2 = hal::aarch64::el2::is_el2() as u64;
    // SAFETY: TLB maintenance modifies no memory, only translation caches.
    // `dsb ishst` first makes prior page-table stores visible to every PE's
    // walker before the broadcast; `dsb sy` waits for the invalidation and
    // `isb` re-fetches through the clean TLB. `nomem` is NOT set: the compiler
    // must not sink PTE stores past the first barrier.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vmalle1is",
            "cbz {el2}, 2f",
            "tlbi alle2is",
            "2:",
            "dsb sy",
            "isb",
            el2 = in(reg) el2,
            options(nostack),
        );
    }
}

/// Order prior page-table stores before asking another CPU to invalidate.
pub(crate) fn publish_pte_stores() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    // SAFETY: `dsb ishst` orders prior page-table stores before the IPI that
    // asks a remote hart to invalidate them.
    unsafe { core::arch::asm!("dsb ishst", options(nostack)) };
}

/// The local half of a shared-root flush (`tlb_shootdown::flush_ranges`): each
/// `tlbi` here is broadcast inside its barrier pair and complete on return, so
/// no remote hart owes anything.
pub(crate) fn shared_flush_local(
    ranges: &[(types::VAddr, usize)],
    all: bool,
) -> crate::memory::tlb_shootdown::RemoteWork {
    // As on x86_64 and RV64: the PTE stores are visible to every walker
    // before any invalidation is broadcast.
    publish_pte_stores();
    if all {
        flush_all();
    } else {
        for &(start, size) in ranges {
            for page in (start..start + size).step_by(crate::memory::paging::PAGE_SIZE) {
                hal::paging::flush_tlb_page(page);
            }
        }
    }
    crate::memory::tlb_shootdown::RemoteWork::None
}

/// Nothing to report: AArch64 has no firmware invalidation path.
pub(crate) fn report_rfence_failures() {}
