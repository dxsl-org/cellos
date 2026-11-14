// SPDX-License-Identifier: MPL-2.0
//! `tlb`: TLB flush and ordering page-table stores before a shootdown.

/// All PCIDs and globals (INVPCID type 2 or untagged PGE toggle).
#[inline(always)]
pub(crate) fn flush_all() {
    hal::domain::flush_all_global();
}

/// Order prior page-table stores before asking another CPU to invalidate.
pub(crate) fn publish_pte_stores() {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// The local half of a shared-root flush (`tlb_shootdown::flush_ranges`).
/// Shared leaves may be cached under inactive PCIDs, and `invlpg` reaches only
/// the current one, so every tag and global entry goes; each remote CPU must
/// then serve an IPI epoch with its own full flush.
pub(crate) fn shared_flush_local(
    _ranges: &[(types::VAddr, usize)],
    _all: bool,
) -> crate::memory::tlb_shootdown::RemoteWork {
    publish_pte_stores();
    flush_all();
    crate::memory::tlb_shootdown::RemoteWork::EpochPath {
        rfence_failed: false,
    }
}

/// Nothing to report: x86 has no firmware invalidation path.
pub(crate) fn report_rfence_failures() {}
