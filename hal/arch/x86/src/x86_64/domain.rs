// SPDX-License-Identifier: MPL-2.0
//! CR3 and PCID operations used by private native-domain roots on x86_64.
//!
//! PCID is a **runtime** property, not a build property: `CR4.PCIDE` is clear
//! unless this module sets it, and the SDM makes a nonzero PCID in CR3 with
//! `PCIDE = 0` a general-protection fault. The tag policy therefore lives in one
//! pure function, [`cr3_for`], and the decision is made once at boot by
//! [`init_pcid`]: a root that cannot carry a tag is programmed with tag 0 and
//! invalidated by a full CR3 reload — never with a value the hardware would
//! ignore, alias, or fault on.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[no_mangle]
pub static VI_KERNEL_CR3: AtomicUsize = AtomicUsize::new(0);

/// Set once at boot: the CPU supports PCID and `CR4.PCIDE` is now set.
static PCID_USABLE: AtomicBool = AtomicBool::new(false);
/// Set once at boot: `INVPCID` is available, so invalidation can name a PCID.
static INVPCID_USABLE: AtomicBool = AtomicBool::new(false);

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
/// CPUID leaf 1, ECX.
const CPUID_ECX_INVPCID: u32 = 1 << 12;
const CPUID_ECX_PCID: u32 = 1 << 17;

/// `INVPCID` type 1 — single-context invalidation, PCID in the descriptor.
const INVPCID_TYPE_SINGLE_CONTEXT: u64 = 1;

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

pub fn invpcid_supported() -> bool {
    cpuid_leaf1_ecx() & CPUID_ECX_INVPCID != 0
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

/// Decide and enable PCID. Idempotent; call before the first CR3 write.
///
/// Returns whether a nonzero PCID may be programmed. When it returns false the
/// caller keeps tag 0 and invalidation falls back to a full CR3 reload.
pub fn init_pcid() -> bool {
    if !pcid_supported() {
        PCID_USABLE.store(false, Ordering::Release);
        INVPCID_USABLE.store(false, Ordering::Release);
        return false;
    }
    let cr4 = read_cr4();
    if cr4 & CR4_PCIDE == 0 {
        // SAFETY: PCID is supported (CPUID) and long mode implies CR0.PG=1 and
        // CR4.PAE=1, the architectural preconditions for setting PCIDE. No flush
        // is required: translations created while PCIDE=0 are global, so they
        // stay valid under every PCID.
        unsafe { write_cr4(cr4 | CR4_PCIDE) };
    }
    let enabled = read_cr4() & CR4_PCIDE != 0;
    PCID_USABLE.store(enabled, Ordering::Release);
    INVPCID_USABLE.store(enabled && invpcid_supported(), Ordering::Release);
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

/// Activate private address space on x86_64.
///
/// `root_pml4`: physical base address of PML4 (4KB aligned).
/// `pcid`: 12-bit PCID, honored only when [`pcid_usable`].
#[inline]
pub fn activate_address_space(root_pml4: usize, pcid: usize) {
    let cr3 = cr3_for(root_pml4, pcid, pcid_usable());
    // SAFETY: `cr3` names a completed PML4 (or the kernel root) with a tag the
    // hardware can carry; the tag decision was made once at boot.
    unsafe {
        core::arch::asm!(
            "mov cr3, {cr3}",
            cr3 = in(reg) cr3,
            options(nostack),
        );
    }
}

/// Invalidate local translations tagged with `pcid`.
///
/// With `INVPCID` the invalidation names the tag; without it (or without PCID)
/// the fallback reloads CR3, which invalidates everything but global entries —
/// coarser than needed and never narrower.
#[inline]
pub fn flush_asid(pcid: usize) {
    if INVPCID_USABLE.load(Ordering::Acquire) {
        let descriptor: [u64; 2] = [(pcid & PCID_MASK) as u64, 0];
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

/// Invalidate all translations by reloading CR3.
#[inline]
pub fn flush_all() {
    reload_cr3();
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
fn reload_cr3() {
    // SAFETY: reloading the current CR3 is the architectural whole-TLB flush.
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
