// SPDX-License-Identifier: MPL-2.0
//! `tlb`: local TLB flush and ordering page-table stores before a shootdown.

/// Flush this hart's TLB: `sfence.vma`, all ASIDs.
#[inline(always)]
pub(crate) fn flush_all() {
    // SAFETY: sfence.vma is a privileged S-mode fence; always safe from S-mode.
    unsafe {
        core::arch::asm!("sfence.vma", options(nostack));
    }
}

/// Order prior page-table stores before asking another hart to invalidate.
pub(crate) fn publish_pte_stores() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    // SAFETY: an S-mode fence orders prior page-table stores.
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
}
