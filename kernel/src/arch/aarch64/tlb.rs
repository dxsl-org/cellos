// SPDX-License-Identifier: MPL-2.0
//! `tlb`: TLB flush and ordering page-table stores before a shootdown.

/// Broadcast flush, all ASIDs, EL1: `tlbi vmalle1is; dsb sy; isb`.
#[inline(always)]
pub(crate) fn flush_all() {
    // SAFETY: tlbi vmalle1is invalidates all EL1 TLB entries broadcast across CPUs.
    // dsb sy ensures the invalidation completes before subsequent memory accesses.
    // isb serializes the instruction stream so the next fetch sees the clean TLB.
    unsafe {
        core::arch::asm!("tlbi vmalle1is", "dsb sy", "isb", options(nostack, nomem));
    }
}

/// Order prior page-table stores before asking another CPU to invalidate.
pub(crate) fn publish_pte_stores() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    // SAFETY: `dsb ishst` orders prior page-table stores before the IPI that
    // asks a remote hart to invalidate them.
    unsafe { core::arch::asm!("dsb ishst", options(nostack)) };
}
