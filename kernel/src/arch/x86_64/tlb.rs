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
