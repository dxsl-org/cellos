//! Intel VT-x root-operation enablement (Tier 3b x86 VMM, phase 01).
//!
//! `enter_root` performs the full VMXON sequence: `CR4.VMXE`, the
//! `IA32_FEATURE_CONTROL` firmware-lock dance, revision-ID stamping of the
//! VMXON region, then `VMXON` itself. VMCS allocation and world-switch are
//! later phases (P09 — the VT-x backend runs only on KVM/real hardware; QEMU
//! TCG cannot emulate VMX, which is why SVM ships first).

use types::{ViError, ViResult};

const MSR_IA32_FEATURE_CONTROL: u32 = 0x3A;
/// Bit 0: lock — once set, the MSR is immutable until reset.
const FC_LOCK: u64 = 1 << 0;
/// Bit 2: VMXON allowed outside SMX operation.
const FC_VMXON_OUTSIDE_SMX: u64 = 1 << 2;

/// `IA32_VMX_BASIC[30:0]` = VMCS/VMXON revision identifier.
const MSR_IA32_VMX_BASIC: u32 = 0x480;

const CR4_VMXE: u64 = 1 << 13;

/// Returns `true` if the CPU advertises VMX (`CPUID.1:ECX[5]`).
pub fn supported() -> bool {
    let leaf = core::arch::x86_64::__cpuid(1);
    leaf.ecx & (1 << 5) != 0
}

/// Returns `true` if firmware locked `IA32_FEATURE_CONTROL` with VMXON
/// disallowed — VMXON would #GP with no recovery until a BIOS change.
pub fn disabled_by_firmware() -> bool {
    // SAFETY: caller context is VMX-capable (CPUID.1:ECX[5]); the MSR exists.
    let fc = unsafe { rdmsr(MSR_IA32_FEATURE_CONTROL) };
    fc & FC_LOCK != 0 && fc & FC_VMXON_OUTSIDE_SMX == 0
}

/// Returns `true` if this CPU already set `CR4.VMXE` (root operation entered).
pub fn is_root() -> bool {
    let cr4: u64;
    // SAFETY: reading CR4 from ring-0 has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    cr4 & CR4_VMXE != 0
}

/// Enter VMX root operation on the current CPU. Idempotent.
///
/// # Arguments
/// * `vmxon_pa` — physical address of a kernel-owned, 4 KiB-aligned VMXON region.
/// * `vmxon_va` — a writable mapping of that same frame (HHDM), used to stamp
///   the revision ID into its first dword before `VMXON`.
///
/// # Errors
/// * [`ViError::NotSupported`] — no VMX per CPUID, or firmware-locked off.
/// * [`ViError::InvalidArgument`] — null/misaligned region.
/// * [`ViError::IO`] — `VMXON` itself failed (RFLAGS.CF/ZF set).
///
/// # Safety
/// `vmxon_pa`/`vmxon_va` must name the same exclusively-owned 4 KiB frame,
/// alive for as long as the CPU stays in root operation.
pub unsafe fn enter_root(vmxon_pa: u64, vmxon_va: *mut u32) -> ViResult<()> {
    if vmxon_pa == 0 || vmxon_pa & 0xFFF != 0 || vmxon_va.is_null() {
        return Err(ViError::InvalidArgument);
    }
    if !supported() {
        return Err(ViError::NotSupported);
    }
    if is_root() {
        return Ok(());
    }
    if disabled_by_firmware() {
        return Err(ViError::NotSupported);
    }
    // If the MSR is unlocked, lock it ourselves with VMXON-outside-SMX allowed
    // (what BIOSes normally do; required before VMXON).
    // SAFETY: VMX advertised; FEATURE_CONTROL exists and is unlocked (checked).
    unsafe {
        let fc = rdmsr(MSR_IA32_FEATURE_CONTROL);
        if fc & FC_LOCK == 0 {
            wrmsr(
                MSR_IA32_FEATURE_CONTROL,
                fc | FC_LOCK | FC_VMXON_OUTSIDE_SMX,
            );
        }
    }
    // Stamp the revision ID (IA32_VMX_BASIC[30:0]) into the region's first dword.
    // SAFETY: vmxon_va is a writable mapping of the caller-owned frame.
    unsafe {
        let revid = (rdmsr(MSR_IA32_VMX_BASIC) & 0x7FFF_FFFF) as u32;
        core::ptr::write_volatile(vmxon_va, revid);
    }
    // SAFETY: CR4.VMXE is a prerequisite for VMXON; setting it has no other effect.
    unsafe {
        core::arch::asm!(
            "mov {tmp}, cr4",
            "or  {tmp}, {vmxe}",
            "mov cr4, {tmp}",
            tmp = out(reg) _,
            vmxe = const CR4_VMXE,
            options(nomem, nostack),
        );
    }
    // VMXON reports failure via CF (invalid region) or ZF (VMfailValid).
    let rflags: u64;
    // SAFETY: all VMXON preconditions established above (CR4.VMXE, locked
    // FEATURE_CONTROL, revision-stamped 4 KiB-aligned region).
    unsafe {
        core::arch::asm!(
            "vmxon [{pa_slot}]",
            "pushfq",
            "pop {rf}",
            pa_slot = in(reg) &vmxon_pa,
            rf = out(reg) rflags,
            options(nostack),
        );
    }
    if rflags & 0x41 != 0 {
        // CF (bit 0) or ZF (bit 6)
        return Err(ViError::IO);
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
/// `msr` must be a writable MSR present on this CPU and `val` legal for it.
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
