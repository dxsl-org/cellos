// SPDX-License-Identifier: MIT
//! Host PKRU preservation across a guest entry.
//!
//! Neither `#VMEXIT` (SVM) nor a VMX VM exit reloads PKRU, so whatever the
//! guest left there would otherwise govern the host kernel after the exit.
//! PKRU also gates *supervisor* data accesses to U=1 pages: an all-deny guest
//! PKRU makes the kernel fault on its first write to a user buffer (the exit
//! record).
//!
//! XCR0 needs no such care around an entry: both backends intercept `XSETBV`
//! (VMX always, SVM through the VMCB), so a guest never writes it. A VMX vCPU
//! puts its guest's XCR0 in place for a whole run (`VmxVcpu::run`), through
//! [`xsetbv0`].
//!
//! PKRU is only accessible while `CR4.PKE` is set (RDPKRU/WRPKRU #UD
//! otherwise), so CR4 is read once per switch and gates every access. Writes
//! happen only when the value differs, keeping the common exit path to a
//! register compare.
//!
//! Callers run with IF=0 from [`HostExtState::enter`] to
//! [`HostExtState::exit`]: no host code that depends on PKRU may run in
//! between.

const CR4_PKE: u64 = 1 << 22;

/// Host values captured immediately before guest entry.
pub(super) struct HostExtState {
    cr4: u64,
    pkru: u32,
}

impl HostExtState {
    /// Snapshot host PKRU and install the guest's.
    ///
    /// # Safety
    /// CPL 0, IF=0, and the guest must be entered before any host access to a
    /// U=1 page (the guest PKRU is live on return when `CR4.PKE` is set).
    #[inline(always)]
    pub(super) unsafe fn enter(guest_pkru: u32) -> Self {
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        let pkru = if cr4 & CR4_PKE != 0 {
            let host = rdpkru();
            if host != guest_pkru {
                wrpkru(guest_pkru);
            }
            host
        } else {
            0
        };
        Self { cr4, pkru }
    }

    /// Record the guest's PKRU into `guest_pkru` and restore the host's.
    ///
    /// Must run right after the exit, before any host access to a U=1 page.
    /// Host CR4 equals the value read in [`Self::enter`] (VMX reloads it from
    /// `HOST_CR4`, SVM from the host save area), so the same gate applies.
    ///
    /// # Safety
    /// CPL 0, IF=0, paired with the [`Self::enter`] of this switch.
    #[inline(always)]
    pub(super) unsafe fn exit(&self, guest_pkru: &mut u32) {
        if self.cr4 & CR4_PKE != 0 {
            let guest = rdpkru();
            *guest_pkru = guest;
            if guest != self.pkru {
                wrpkru(self.pkru);
            }
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn xgetbv0() -> u64 {
    let (lo, hi): (u32, u32);
    core::arch::asm!("xgetbv", in("ecx") 0u32, out("eax") lo, out("edx") hi,
        options(nomem, nostack, preserves_flags));
    (u64::from(hi) << 32) | u64::from(lo)
}

#[inline(always)]
pub(crate) unsafe fn xsetbv0(value: u64) {
    core::arch::asm!("xsetbv", in("ecx") 0u32, in("eax") value as u32, in("edx") (value >> 32) as u32,
        options(nostack, preserves_flags));
}

#[inline(always)]
unsafe fn rdpkru() -> u32 {
    let value: u32;
    core::arch::asm!("rdpkru", in("ecx") 0u32, out("eax") value, out("edx") _,
        options(nomem, nostack, preserves_flags));
    value
}

#[inline(always)]
unsafe fn wrpkru(value: u32) {
    // WRPKRU changes access rights for later memory operations: no `nomem`.
    core::arch::asm!("wrpkru", in("eax") value, in("ecx") 0u32, in("edx") 0u32,
        options(nostack, preserves_flags));
}
