//! AMD SVM root-operation enablement (Tier 3b x86 VMM, phase 01).
//!
//! Entering SVM "root operation" means setting `EFER.SVME` so the CPU accepts
//! `VMRUN`/`VMLOAD`/`VMSAVE`, and pointing `VM_HSAVE_PA` at a 4 KiB host save
//! area the CPU uses to stash host state across `VMRUN`. No guest structures
//! are touched here — VMCB allocation and world-switch live in later phases.
//!
//! SVM is the first x86 backend because QEMU TCG emulates it (`-cpu
//! qemu64,+svm -accel tcg`) while TCG has no VMX support — see
//! `.agents/260711-1917-tier3b-x86-vtx/plan.md` Validation Log #9.

use types::{ViError, ViResult};

/// Extended Feature Enable Register — bit 12 (`SVME`) gates all SVM instructions.
const MSR_EFER: u32 = 0xC000_0080;
const EFER_SVME: u64 = 1 << 12;

/// SVM control MSR — bit 4 (`SVMDIS`) set means firmware locked SVM off.
const MSR_VM_CR: u32 = 0xC001_0114;
const VM_CR_SVMDIS: u64 = 1 << 4;

/// Physical address of the 4 KiB host save area used by `VMRUN`.
const MSR_VM_HSAVE_PA: u32 = 0xC001_0117;

/// Returns `true` if the CPU advertises SVM (`CPUID.8000_0001:ECX[2]`).
///
/// Guards the extended leaf range first: a CPU whose max extended leaf is
/// below `0x8000_0001` (possible under exotic emulators) must not be probed.
pub fn supported() -> bool {
    let max_ext = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if max_ext < 0x8000_0001 {
        return false;
    }
    let leaf = core::arch::x86_64::__cpuid(0x8000_0001);
    leaf.ecx & (1 << 2) != 0
}

/// Returns `true` if firmware locked SVM off (`VM_CR.SVMDIS` set).
///
/// Only meaningful when [`supported`] already returned `true` — reading the
/// SVM MSR range on a non-SVM CPU would #GP.
pub fn disabled_by_bios() -> bool {
    // SAFETY: caller contract — VM_CR exists because CPUID advertised SVM.
    unsafe { rdmsr(MSR_VM_CR) & VM_CR_SVMDIS != 0 }
}

/// Returns `true` if this CPU is already in SVM root operation (`EFER.SVME`).
pub fn is_enabled() -> bool {
    // SAFETY: EFER exists on every x86_64 CPU; rdmsr has no side effects.
    unsafe { rdmsr(MSR_EFER) & EFER_SVME != 0 }
}

/// Enter SVM root operation on the current CPU.
///
/// Idempotent: returns `Ok(())` without rewriting MSRs if `EFER.SVME` is
/// already set (AP re-entry / double init).
///
/// # Arguments
/// * `hsave_pa` — physical address of a kernel-owned, 4 KiB-aligned frame the
///   CPU uses as the host save area. Must never be mapped into any guest.
///
/// # Errors
/// * [`ViError::NotSupported`] — CPUID does not advertise SVM, or firmware
///   locked it off (`VM_CR.SVMDIS`). No MSR is written in either case.
/// * [`ViError::InvalidArgument`] — `hsave_pa` is 0 or not 4 KiB-aligned.
///
/// # Safety
/// Caller must guarantee `hsave_pa` names an exclusively-owned physical frame
/// that stays alive for the whole time SVM remains enabled (the CPU DMAs host
/// state into it on every future `VMRUN`).
pub unsafe fn enable(hsave_pa: u64) -> ViResult<()> {
    if hsave_pa == 0 || hsave_pa & 0xFFF != 0 {
        return Err(ViError::InvalidArgument);
    }
    if !supported() {
        return Err(ViError::NotSupported);
    }
    if disabled_by_bios() {
        return Err(ViError::NotSupported);
    }
    if is_enabled() {
        return Ok(());
    }
    // SAFETY: SVM advertised + not firmware-locked (checked above); setting
    // SVME only unlocks the SVM instruction set, it changes no memory state.
    unsafe {
        wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_SVME);
        // SAFETY: hsave_pa is a caller-owned 4 KiB frame (checked aligned);
        // the CPU only writes it during VMRUN, which cannot occur before a
        // VMCB exists (later phase).
        wrmsr(MSR_VM_HSAVE_PA, hsave_pa);
    }
    Ok(())
}

/// # Safety
/// `msr` must be an architecturally-defined MSR present on this CPU, else #GP.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (hi, lo): (u32, u32);
    // SAFETY: invariant upheld by caller.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | lo as u64
}

/// # Safety
/// `msr` must be a writable MSR present on this CPU and `val` a legal value
/// for it, else #GP.
unsafe fn wrmsr(msr: u32, val: u64) {
    // SAFETY: invariant upheld by caller.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") val as u32,
            in("edx") (val >> 32) as u32,
            options(nomem, nostack, preserves_flags),
        );
    }
}
