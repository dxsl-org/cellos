// SPDX-License-Identifier: MPL-2.0
//! CR3 and PCID operations used by private native-domain roots on x86_64.
//!
//! PCID is a **runtime** property, not a build property: `CR4.PCIDE` is clear
//! unless this module sets it, and the SDM makes a nonzero PCID in CR3 with
//! `PCIDE = 0` a general-protection fault. The tag policy therefore lives in one
//! pure function, [`cr3_for`], and the decision is made once at boot by
//! [`init_pcid`]. PCID requires INVPCID: a CR3 reload cannot invalidate an
//! inactive tag, so without INVPCID every root uses tag 0.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[no_mangle]
pub static VI_KERNEL_CR3: AtomicUsize = AtomicUsize::new(0);

/// Set once at boot: the CPU supports PCID and INVPCID, and `CR4.PCIDE` is set.
static PCID_USABLE: AtomicBool = AtomicBool::new(false);

/// Test-only: root programmings the scheduler has proven, never an admission signal.
#[cfg(feature = "test-hooks")]
static ROOT_PROGRAMMINGS: AtomicUsize = AtomicUsize::new(0);
/// Test-only: invalidations issued through this module.
#[cfg(feature = "test-hooks")]
static ASID_FLUSHES: AtomicUsize = AtomicUsize::new(0);

/// Bits CR3 carries a PCID in when `CR4.PCIDE` is set.
pub const PCID_WIDTH_BITS: usize = 12;
const PCID_MASK: usize = (1usize << PCID_WIDTH_BITS) - 1;

const CR4_PCIDE: u64 = 1 << 17;
/// CPUID leaf 1, ECX and leaf 7 subleaf 0, EBX respectively.
const CPUID_ECX_PCID: u32 = 1 << 17;
const CPUID_7_EBX_INVPCID: u32 = 1 << 10;

/// `INVPCID` type 1 — single-context invalidation, PCID in the descriptor.
const INVPCID_TYPE_SINGLE_CONTEXT: u64 = 1;
/// `INVPCID` type 3 — every PCID, retaining global translations.
const INVPCID_TYPE_ALL_CONTEXTS: u64 = 3;

#[inline]
fn read_cr4() -> u64 {
    let cr4: u64;
    // SAFETY: reading CR4 is unprivileged-harmless at CPL0 and has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    cr4
}

/// # Safety
/// Writing CR4 changes the CPU's feature state; the caller must ensure the new
/// value is architecturally valid for the current mode.
#[inline]
unsafe fn write_cr4(value: u64) {
    // SAFETY: caller guarantees the value is valid; mirrors `pku.rs`.
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn cpuid_leaf1_ecx() -> u32 {
    // `__cpuid` is safe on x86_64: leaf 1 exists on every CPU in this mode.
    core::arch::x86_64::__cpuid(1).ecx
}

pub fn pcid_supported() -> bool {
    cpuid_leaf1_ecx() & CPUID_ECX_PCID != 0
}

#[inline]
const fn cpuid_has_invpcid(max_leaf: u32, leaf7_ebx: u32) -> bool {
    max_leaf >= 7 && leaf7_ebx & CPUID_7_EBX_INVPCID != 0
}

pub fn invpcid_supported() -> bool {
    let max_leaf = core::arch::x86_64::__cpuid(0).eax;
    max_leaf >= 7 && cpuid_has_invpcid(max_leaf, core::arch::x86_64::__cpuid_count(7, 0).ebx)
}

/// Is a nonzero PCID in CR3 meaningful right now?
pub fn pcid_usable() -> bool {
    PCID_USABLE.load(Ordering::Acquire)
}

/// Is `CR4.PCIDE` set right now? (Reads the register, not the cached decision.)
pub fn pcide_set() -> bool {
    read_cr4() & CR4_PCIDE != 0
}

/// The CR3 value for `root_pml4` under tag `pcid`.
///
/// Pure so the policy is testable without touching the register. With PCID
/// unusable the tag is **dropped** rather than masked at the instruction: a
/// nonzero PCID with `CR4.PCIDE = 0` faults, so the fail-closed value is the
/// untagged root.
pub const fn cr3_for(root_pml4: usize, pcid: usize, pcid_usable: bool) -> usize {
    let tag = if pcid_usable { pcid & PCID_MASK } else { 0 };
    (root_pml4 & !PCID_MASK) | tag
}

#[inline]
const fn pcid_enable_allowed(cr4: u64, cr3: usize) -> bool {
    cr4 & CR4_PCIDE != 0 || cr3 & PCID_MASK == 0
}

/// Decide and enable PCID. Idempotent; call before the first domain CR3 write.
///
/// Nonzero tags require both PCID and INVPCID. Reloading CR3 only invalidates
/// the current tag, so CPUs without INVPCID must run with PCIDE clear. Enabling
/// PCIDE also requires the current CR3 low 12 bits to be zero.
pub fn init_pcid() -> bool {
    let cr4 = read_cr4();
    if !pcid_supported() || !invpcid_supported() {
        if cr4 & CR4_PCIDE != 0 {
            // Boot firmware may have left PCIDE enabled. Select tag 0 first
            // (preserving the active root), then disable tagged operation.
            let untagged = read_cr3() & !PCID_MASK;
            // SAFETY: the root is unchanged and tag 0 is valid with PCIDE set.
            unsafe {
                core::arch::asm!("mov cr3, {}", in(reg) untagged, options(nostack));
                write_cr4(cr4 & !CR4_PCIDE);
            }
        }
        PCID_USABLE.store(false, Ordering::Release);
        return false;
    }
    if cr4 & CR4_PCIDE == 0 {
        if !pcid_enable_allowed(cr4, read_cr3()) {
            // CR3 may still carry PWT/PCD bits from the bootloader. Setting
            // PCIDE in this state would #GP; leave all domain roots untagged.
            PCID_USABLE.store(false, Ordering::Release);
            return false;
        }
        // SAFETY: CPUID permits PCIDE, long mode implies CR0.PG and CR4.PAE,
        // and CR3[11:0] is zero. The current translations remain valid.
        unsafe { write_cr4(cr4 | CR4_PCIDE) };
    }
    let enabled = read_cr4() & CR4_PCIDE != 0;
    PCID_USABLE.store(enabled, Ordering::Release);
    enabled
}

pub fn record_kernel_cr3(cr3: usize) {
    VI_KERNEL_CR3.store(cr3, Ordering::Release);
}

pub fn kernel_cr3() -> usize {
    VI_KERNEL_CR3.load(Ordering::Acquire)
}

/// The live CR3 value, including its tag bits.
#[inline]
pub fn read_cr3() -> usize {
    let cr3: usize;
    // SAFETY: reading CR3 has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    }
    cr3
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPagingError {
    Unsupported,
}

#[inline]
const fn invpcid_descriptor(pcid: usize) -> [u64; 2] {
    [(pcid & PCID_MASK) as u64, 0]
}

/// Invalidate local translations tagged with `pcid`.
///
/// With PCID enabled, type-1 INVPCID invalidates even an inactive tag before
/// it can be recycled. Without PCID, every root uses tag 0 and a CR3 reload
/// invalidates its non-global translations.
#[inline]
pub fn flush_asid(pcid: usize) {
    if PCID_USABLE.load(Ordering::Acquire) {
        let descriptor = invpcid_descriptor(pcid);
        let kind = INVPCID_TYPE_SINGLE_CONTEXT;
        // SAFETY: type 1 with CR4.PCIDE=1 and CPUID.INVPCID is architecturally
        // valid; the descriptor is a 128-bit value in memory this frame owns.
        unsafe {
            core::arch::asm!(
                "invpcid {kind}, [{descriptor}]",
                kind = in(reg) kind,
                descriptor = in(reg) descriptor.as_ptr(),
                options(nostack),
            );
        }
    } else {
        reload_cr3();
    }
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

/// x86_64 remote invalidation: this backend is single-CPU (see the phase-02
/// blocker on non-RV64 SMP), so "remote" is this hart. Multicore x86 admission
/// stays refused until per-CPU hart identity and IPI exist.
pub fn flush_asid_remote(_hart_mask: usize, asid: usize) -> Result<(), DomainPagingError> {
    flush_asid(asid);
    Ok(())
}

/// Invalidate non-global translations for every PCID on this CPU.
#[inline]
pub fn flush_all() {
    if PCID_USABLE.load(Ordering::Acquire) {
        let descriptor = [0u64; 2];
        let kind = INVPCID_TYPE_ALL_CONTEXTS;
        // SAFETY: CPUID.INVPCID was verified at boot; type 3 is valid and
        // invalidates all tags, including ones not active in CR3.
        unsafe {
            core::arch::asm!(
                "invpcid {kind}, [{descriptor}]",
                kind = in(reg) kind,
                descriptor = in(reg) descriptor.as_ptr(),
                options(nostack),
            );
        }
    } else {
        reload_cr3();
    }
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
fn reload_cr3() {
    // SAFETY: PCIDE is disabled on this path, so reloading CR3 invalidates
    // every non-global translation (there is only the untagged context).
    unsafe {
        core::arch::asm!(
            "mov rax, cr3",
            "mov cr3, rax",
            out("rax") _,
            options(nostack),
        );
    }
}

/// Test-only view of `(root programmings, invalidations issued)`.
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

/// Records a scheduler-proven root programming for test fixtures.
#[inline]
pub fn observe_switch_activation() {
    #[cfg(feature = "test-hooks")]
    ROOT_PROGRAMMINGS.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invpcid_requires_leaf_seven_ebx_bit_ten() {
        assert!(!cpuid_has_invpcid(6, CPUID_7_EBX_INVPCID));
        assert!(!cpuid_has_invpcid(7, 1 << 12)); // EBX[12] is not INVPCID.
        assert!(cpuid_has_invpcid(7, CPUID_7_EBX_INVPCID));
    }

    #[test]
    fn pcide_requires_untagged_boot_cr3() {
        assert!(pcid_enable_allowed(0, 0x59000));
        assert!(!pcid_enable_allowed(0, 0x59008)); // Boot CR3.PWT.
        assert!(pcid_enable_allowed(CR4_PCIDE, 0x59008));
    }

    #[test]
    fn untagged_roots_and_target_context_descriptor() {
        let root = 0x59000;
        assert_eq!(cr3_for(root, 0xfff, false), root);
        assert_eq!(cr3_for(root, 0xfff, true), root | 0xfff);
        assert_eq!(invpcid_descriptor(0xfff), [0xfff, 0]);
        assert_eq!(invpcid_descriptor(0x1fff), [0xfff, 0]);
    }
}
