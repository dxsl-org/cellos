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

/// Punch passthrough holes in an all-ones MSRPM so a booting guest touches its
/// own context MSRs natively instead of round-tripping every access to the VMM.
///
/// The bitmap must already be filled with `0xFF` (intercept every MSR). This
/// clears the intercept bits for MSRs the CPU saves/restores **per guest** —
/// the `SYSCALL`/`SYSENTER` target MSRs and the segment-base MSRs, handled by
/// `VMSAVE`/`VMLOAD` and the VMCB state-save area — so letting the guest read
/// and write them directly is both correct and safe. `EFER` keeps its **write**
/// intercept (so the run loop can re-assert `SVME`) but its read passes through.
/// Every other MSR stays intercepted and is stubbed by the run loop.
///
/// # Safety
/// `msrpm_va` must be the live kernel VA of the 8 KiB MSRPM frame, already
/// zero-filled to `0xFF`, owned by the caller for the vCPU's lifetime.
pub unsafe fn msrpm_passthrough_boot(msrpm_va: *mut u8) {
    // Guest-context MSRs: passthrough read + write.
    const PASSTHRU: [u32; 10] = [
        0x174,       // IA32_SYSENTER_CS
        0x175,       // IA32_SYSENTER_ESP
        0x176,       // IA32_SYSENTER_EIP
        0xC000_0081, // STAR
        0xC000_0082, // LSTAR
        0xC000_0083, // CSTAR
        0xC000_0084, // SFMASK
        0xC000_0100, // FS_BASE
        0xC000_0101, // GS_BASE
        0xC000_0102, // KERNEL_GS_BASE
    ];
    for &m in &PASSTHRU {
        // SAFETY: caller guarantees msrpm_va is the live 8 KiB MSRPM frame.
        unsafe { msrpm_clear(msrpm_va, m, true, true) };
    }
    // EFER: read passthrough, write stays intercepted (SVME re-assert).
    // SAFETY: as above.
    unsafe { msrpm_clear(msrpm_va, MSR_EFER, true, false) };
}

/// Clear the read and/or write intercept bit of `msr` in the MSRPM at `base`.
/// MSRs outside the three MSRPM-covered ranges are left intercepted.
///
/// # Safety
/// `base` must be the live 8 KiB MSRPM frame VA (see [`msrpm_passthrough_boot`]).
unsafe fn msrpm_clear(base: *mut u8, msr: u32, clr_read: bool, clr_write: bool) {
    let (range_byte, idx) = if msr < 0x2000 {
        (0usize, msr)
    } else if (0xC000_0000..0xC000_2000).contains(&msr) {
        (0x800usize, msr - 0xC000_0000)
    } else if (0xC001_0000..0xC001_2000).contains(&msr) {
        (0x1000usize, msr - 0xC001_0000)
    } else {
        return;
    };
    let bit = (idx as usize) * 2;
    let byte_off = range_byte + bit / 8;
    let read_mask = 1u8 << (bit % 8);
    let write_mask = 1u8 << ((bit % 8) + 1);
    // SAFETY: byte_off < 0x1800 < 8 KiB frame; caller owns the frame.
    unsafe {
        let p = base.add(byte_off);
        let mut b = core::ptr::read_volatile(p);
        if clr_read {
            b &= !read_mask;
        }
        if clr_write {
            b &= !write_mask;
        }
        core::ptr::write_volatile(p, b);
    }
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
