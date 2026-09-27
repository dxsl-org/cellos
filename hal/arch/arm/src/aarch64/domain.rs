// SPDX-License-Identifier: MPL-2.0
//! TTBR0_EL1 and ASID operations used by private native-domain roots on AArch64.

use core::sync::atomic::{AtomicUsize, Ordering};

#[no_mangle]
pub static VI_KERNEL_TTBR0: AtomicUsize = AtomicUsize::new(0);

/// Test-only: root programmings the scheduler has proven, never an admission signal.
#[cfg(feature = "test-hooks")]
static ROOT_PROGRAMMINGS: AtomicUsize = AtomicUsize::new(0);
/// Test-only: local `tlbi` operations issued through this module.
#[cfg(feature = "test-hooks")]
static ASID_FLUSHES: AtomicUsize = AtomicUsize::new(0);

/// Test-only view of `(root programmings, invalidations issued)`.
///
/// AArch64 carries the ASID in `TTBR0_EL1`, so a switch to an ASID whose
/// translations are still valid issues no `tlbi`: a domain activation is one
/// root programming and zero flushes here, unlike RV64 where the switch itself
/// must fence. Invalidations are counted where they are issued — `flush_asid`
/// and `flush_all` — so a fixture can prove the counter is live.
#[cfg(feature = "test-hooks")]
pub fn switch_counters() -> (usize, usize) {
    (
        ROOT_PROGRAMMINGS.load(Ordering::Acquire),
        ASID_FLUSHES.load(Ordering::Acquire),
    )
}

#[cfg(feature = "test-hooks")]
pub fn reset_switch_counters() {
    ROOT_PROGRAMMINGS.store(0, Ordering::Release);
    ASID_FLUSHES.store(0, Ordering::Release);
}

pub fn record_kernel_ttbr0(ttbr0: usize) {
    VI_KERNEL_TTBR0.store(ttbr0, Ordering::Release);
}

pub fn kernel_ttbr0() -> usize {
    VI_KERNEL_TTBR0.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPagingError {
    Unsupported,
}

/// The TTBR0_EL1 value for a private root: ASID in the top bits, table base in
/// bits 47:12. One definition, used by the switch routine and by any caller that
/// programs the register directly — the encoding must not be duplicated.
#[inline]
pub const fn root_register_value(root_baddr: usize, asid: usize) -> usize {
    ((asid & 0xffff) << 48) | (root_baddr & 0x0000_ffff_ffff_f000)
}

/// Invalidate local translations tagged with `asid`.
#[inline]
pub fn flush_asid(asid: usize) {
    let asid_val = (asid as u64 & 0xffff) << 48;
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi aside1is, {asid}",
            "dsb ish",
            "isb",
            asid = in(reg) asid_val,
            options(nostack),
        );
    }
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

pub fn flush_asid_remote(_hart_mask: usize, asid: usize) -> Result<(), DomainPagingError> {
    // On AArch64, `tlbi aside1is` is already broadcast across all PEs in the Inner Shareable domain.
    flush_asid(asid);
    Ok(())
}

/// Invalidate all EL1 translations across the inner-shareable domain.
#[inline]
pub fn flush_all() {
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack),
        );
    }
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

/// Records a scheduler-proven root programming for test fixtures.
///
/// Called from `SwitchPlan::root_switch` for every transition that programs
/// `TTBR0_EL1` — activation, same-domain resume, and the safe-root handoff —
/// so the counter names plans, not instructions; the instruction itself is
/// issued by the switch routine itself, between the outgoing save and the incoming load.
#[inline]
pub fn observe_switch_activation() {
    #[cfg(feature = "test-hooks")]
    ROOT_PROGRAMMINGS.fetch_add(1, Ordering::Relaxed);
}
