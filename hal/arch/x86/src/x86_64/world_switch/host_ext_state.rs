// SPDX-License-Identifier: MIT
//! Host XCR0/PKRU preservation across a guest run.
//!
//! Neither `#VMEXIT` (SVM) nor a VMX VM exit reloads XCR0 or PKRU, so whatever
//! the guest left there would otherwise govern the host kernel after the exit.
//! PKRU also gates *supervisor* data accesses to U=1 pages: an all-deny guest
//! PKRU makes the kernel fault on its first write to a user buffer (the exit
//! record). XCR0 decides which extended state user code may use and how large
//! an XSAVE image is.
//!
//! The registers are only accessible while the matching CR4 bit is set
//! (`OSXSAVE` for XGETBV/XSETBV, `PKE` for RDPKRU/WRPKRU — otherwise #UD), so
//! CR4 is read once per switch and gates every access. Writes happen only when
//! the value differs, keeping the common exit path to a register compare.
//!
//! Callers run with IF=0 from [`HostExtState::enter`] to
//! [`HostExtState::exit`]: no host code that depends on PKRU/XCR0 may run in
//! between.

const CR4_OSXSAVE: u64 = 1 << 18;
const CR4_PKE: u64 = 1 << 22;

/// Host values captured immediately before guest entry.
pub(super) struct HostExtState {
    cr4: u64,
    xcr0: u64,
    pkru: u32,
}

impl HostExtState {
    /// Snapshot host XCR0/PKRU and install the guest's PKRU.
    ///
    /// # Safety
    /// CPL 0, IF=0, and the guest must be entered before any host access to a
    /// U=1 page (the guest PKRU is live on return when `CR4.PKE` is set).
    #[inline(always)]
    pub(super) unsafe fn enter(guest_pkru: u32) -> Self {
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        let xcr0 = if cr4 & CR4_OSXSAVE != 0 { xgetbv0() } else { 0 };
        let pkru = if cr4 & CR4_PKE != 0 {
            let host = rdpkru();
            if host != guest_pkru {
                wrpkru(guest_pkru);
            }
            host
        } else {
            0
        };
        Self { cr4, xcr0, pkru }
    }

    /// Record the guest's PKRU into `guest_pkru` and restore host XCR0/PKRU.
    ///
    /// Must run right after the exit, before any host access to a U=1 page.
    /// Host CR4 equals the value read in [`Self::enter`] (VMX reloads it from
    /// `HOST_CR4`, SVM from the host save area), so the same gates apply.
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
        if self.cr4 & CR4_OSXSAVE != 0 && xgetbv0() != self.xcr0 {
            xsetbv0(self.xcr0);
        }
    }
}

#[inline(always)]
unsafe fn xgetbv0() -> u64 {
    let (lo, hi): (u32, u32);
    core::arch::asm!("xgetbv", in("ecx") 0u32, out("eax") lo, out("edx") hi,
        options(nomem, nostack, preserves_flags));
    (u64::from(hi) << 32) | u64::from(lo)
}

#[inline(always)]
unsafe fn xsetbv0(value: u64) {
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
