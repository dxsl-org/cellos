// SPDX-License-Identifier: MPL-2.0
//! `tlb` for the 32-bit targets: bare physical, nothing to flush.

#[inline(always)]
pub(crate) fn flush_all() {}

pub(crate) fn publish_pte_stores() {}

/// Bare physical: no translation is cached anywhere.
pub(crate) fn shared_flush_local(
    _ranges: &[(types::VAddr, usize)],
    _all: bool,
) -> crate::memory::tlb_shootdown::RemoteWork {
    crate::memory::tlb_shootdown::RemoteWork::None
}

pub(crate) fn report_rfence_failures() {}
