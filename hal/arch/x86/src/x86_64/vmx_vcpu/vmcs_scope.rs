// SPDX-License-Identifier: MIT
//! VMCS currency discipline and the accessors that need it.
//!
//! A VMCS is *active* on the CPU that last `VMPTRLD`ed it until that CPU
//! `VMCLEAR`s it; using it on another CPU meanwhile is architecturally
//! undefined (the first CPU may still hold, and later write back, cached
//! state). The owner task can migrate between `RunVcpu` calls, so
//! every access goes through [`with_vmcs`]: load, operate, clear — all with
//! IF=0 so the task cannot be moved in between. Consequences: no VMCS is ever
//! current outside these scopes (raw `vmread`/`vmwrite` from callers fail
//! closed), and every run starts with `VMLAUNCH` because `VMCLEAR` resets the
//! launch state.

use types::ViResult;

use super::super::vmcs::*;
use super::VmxVcpu;

/// Run `f` with the VMCS at `vmcs_pa` current on this CPU, then clear it.
///
/// # Safety
/// VMX root operation on this CPU; `vmcs_pa` a VMCS owned by the caller that
/// is not active on any other CPU (guaranteed when all users go through here).
pub(super) unsafe fn with_vmcs<R>(vmcs_pa: u64, f: impl FnOnce() -> ViResult<R>) -> ViResult<R> {
    let rflags: u64;
    core::arch::asm!("pushfq", "pop {}", "cli", out(reg) rflags, options(nomem));
    let result = vmptrld(vmcs_pa).and_then(|()| f());
    let cleared = vmclear(vmcs_pa);
    if rflags & (1 << 9) != 0 {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    let value = result?;
    cleared?;
    Ok(value)
}

impl VmxVcpu {
    /// `(rip, rflags, cr0)` snapshot for triple-fault diagnostics; zeros if
    /// the VMCS cannot be loaded.
    pub fn shutdown_diagnostics(&self) -> (u64, u64, u64) {
        // SAFETY: VMX root is active whenever a VmxVcpu exists (it was built
        // by `new`, which VMCLEARs through the same discipline); reads only.
        unsafe {
            with_vmcs(self.vmcs_pa, || {
                Ok((
                    vmread(GUEST_RIP)?,
                    vmread(GUEST_RFLAGS)?,
                    vmread(GUEST_CR0)?,
                ))
            })
        }
        .unwrap_or((0, 0, 0))
    }

    /// `(exit qualification, guest physical address, RIP)` snapshot for EPT
    /// violation diagnostics; zeros if the VMCS cannot be loaded.
    pub fn npf_diagnostics(&self) -> (u64, u64, u64) {
        // SAFETY: as in `shutdown_diagnostics`; reads only.
        unsafe {
            with_vmcs(self.vmcs_pa, || {
                Ok((
                    vmread(EXIT_QUALIFICATION)?,
                    vmread(GUEST_PHYSICAL_ADDRESS)?,
                    vmread(GUEST_RIP)?,
                ))
            })
        }
        .unwrap_or((0, 0, 0))
    }
}
