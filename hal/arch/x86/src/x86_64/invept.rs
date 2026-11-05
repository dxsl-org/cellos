//! Intel `INVEPT` — invalidation of EPT-derived translations (SDM Vol. 3C 29.4.3).
//!
//! Guest-physical and combined mappings are tagged by the EPTP's EP4TA and
//! survive VM entries and exits. Any change that removes or narrows an EPT
//! entry, and every reuse of an EPT root or guest frame, is only safe once the
//! CPU that may hold those mappings has executed `INVEPT` for that context.
//!
//! `INVEPT` acts on the executing logical processor only. Callers that need a
//! context clean on the CPU about to enter the guest must invalidate on that
//! CPU with interrupts masked up to VM entry.
//!
//! The supported invalidation types come from `IA32_VMX_EPT_VPID_CAP`, read
//! once and cached: capabilities are uniform across the CPUs of one machine.

use core::sync::atomic::{AtomicU8, Ordering};

use super::msr;
use types::{ViError, ViResult};

const MSR_IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
const MSR_IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
const MSR_IA32_VMX_EPT_VPID_CAP: u32 = 0x48C;

/// `IA32_VMX_PROCBASED_CTLS[63]`: "activate secondary controls" may be 1.
const SECONDARY_CTLS_ALLOWED: u64 = 1 << 63;
/// `IA32_VMX_PROCBASED_CTLS2[33]`: "enable EPT" may be 1. Without it the
/// EPT/VPID capability MSR does not exist and reading it raises `#GP`.
const EPT_ALLOWED: u64 = 1 << 33;
/// `IA32_VMX_EPT_VPID_CAP[20]`: the `INVEPT` instruction is supported.
const CAP_INVEPT: u64 = 1 << 20;
/// `IA32_VMX_EPT_VPID_CAP[25]`: single-context `INVEPT` is supported.
const CAP_SINGLE_CONTEXT: u64 = 1 << 25;
/// `IA32_VMX_EPT_VPID_CAP[26]`: all-context `INVEPT` is supported.
const CAP_ALL_CONTEXT: u64 = 1 << 26;

/// `INVEPT` type operands (SDM Vol. 3C 29.4.3.1).
const TYPE_SINGLE_CONTEXT: u64 = 1;
const TYPE_ALL_CONTEXT: u64 = 2;

// Cached capability byte. `PROBED` distinguishes "probed, nothing supported"
// from "not yet probed" (0).
const PROBED: u8 = 1 << 7;
const SINGLE: u8 = 1 << 0;
const ALL: u8 = 1 << 1;
static CAPS: AtomicU8 = AtomicU8::new(0);

/// Read and cache which `INVEPT` types this CPU implements.
///
/// # Safety
/// The CPU must support VMX (`CPUID.1:ECX[5]`), so the VMX capability MSRs exist.
unsafe fn caps() -> u8 {
    let cached = CAPS.load(Ordering::Relaxed);
    if cached & PROBED != 0 {
        return cached;
    }
    let mut caps = PROBED;
    // SAFETY: the caller guarantees VMX support; each MSR is read only after
    // the control that makes it architecturally present was found allowed.
    unsafe {
        if msr::read(MSR_IA32_VMX_PROCBASED_CTLS) & SECONDARY_CTLS_ALLOWED != 0
            && msr::read(MSR_IA32_VMX_PROCBASED_CTLS2) & EPT_ALLOWED != 0
        {
            let cap = msr::read(MSR_IA32_VMX_EPT_VPID_CAP);
            if cap & CAP_INVEPT != 0 {
                if cap & CAP_SINGLE_CONTEXT != 0 {
                    caps |= SINGLE;
                }
                if cap & CAP_ALL_CONTEXT != 0 {
                    caps |= ALL;
                }
            }
        }
    }
    CAPS.store(caps, Ordering::Relaxed);
    caps
}

/// True if this CPU can invalidate an EPT context by some `INVEPT` type. An
/// EPT guest must not be created without it: unmaps and frame reuse could
/// never be made visible to the CPU.
///
/// # Safety
/// The CPU must support VMX (`CPUID.1:ECX[5]`).
pub unsafe fn supported() -> bool {
    // SAFETY: forwarded caller contract.
    unsafe { caps() & (SINGLE | ALL) != 0 }
}

/// Execute `INVEPT ty, [descriptor]`.
///
/// # Safety
/// The current CPU must be in VMX operation (after a successful `VMXON`) and
/// `ty` must be a type this CPU supports; otherwise the instruction faults.
unsafe fn invept(ty: u64, eptp: u64) -> ViResult<()> {
    // 128-bit descriptor: EPTP in bits 63:0, bits 127:64 reserved (zero).
    let descriptor: [u64; 2] = [eptp, 0];
    let rflags: u64;
    // SAFETY: caller contract; the descriptor lives on this stack frame for the
    // duration of the instruction.
    unsafe {
        core::arch::asm!(
            "invept {ty}, [{desc}]",
            "pushfq",
            "pop {rf}",
            ty = in(reg) ty,
            desc = in(reg) descriptor.as_ptr(),
            rf = out(reg) rflags,
            options(nostack),
        );
    }
    // VMfailInvalid (CF) / VMfailValid (ZF).
    if rflags & 0x41 != 0 {
        Err(ViError::IO)
    } else {
        Ok(())
    }
}

/// Invalidate every mapping derived from `eptp` on the current CPU.
///
/// Uses single-context `INVEPT`; on CPUs that implement only the all-context
/// type it falls back to that (a superset). `Err(NotSupported)` when neither
/// exists — callers must then refuse the operation that needed invalidation.
///
/// # Safety
/// The current CPU must be in VMX operation (after a successful `VMXON`).
pub unsafe fn invalidate_context(eptp: u64) -> ViResult<()> {
    // SAFETY: VMX operation implies VMX support; type chosen from the caps.
    unsafe {
        let caps = caps();
        if caps & SINGLE != 0 {
            invept(TYPE_SINGLE_CONTEXT, eptp)
        } else if caps & ALL != 0 {
            invept(TYPE_ALL_CONTEXT, 0)
        } else {
            Err(ViError::NotSupported)
        }
    }
}

/// Invalidate mappings derived from every EPTP on the current CPU.
///
/// Falls back to single-context invalidation of `eptp` when all-context is not
/// implemented, so the named context is always covered.
///
/// # Safety
/// The current CPU must be in VMX operation (after a successful `VMXON`).
pub unsafe fn invalidate_all(eptp: u64) -> ViResult<()> {
    // SAFETY: VMX operation implies VMX support; type chosen from the caps.
    unsafe {
        let caps = caps();
        if caps & ALL != 0 {
            invept(TYPE_ALL_CONTEXT, 0)
        } else if caps & SINGLE != 0 {
            invept(TYPE_SINGLE_CONTEXT, eptp)
        } else {
            Err(ViError::NotSupported)
        }
    }
}
