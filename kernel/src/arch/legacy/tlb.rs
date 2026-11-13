// SPDX-License-Identifier: MPL-2.0
//! `tlb` for the 32-bit targets: bare physical, nothing to flush.

#[inline(always)]
pub(crate) fn flush_all() {}

pub(crate) fn publish_pte_stores() {}
