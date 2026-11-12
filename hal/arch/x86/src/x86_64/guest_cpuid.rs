// SPDX-License-Identifier: MIT
//! Guest-visible CPUID policy shared by the VMX and SVM run loops.
//!
//! Both backends execute the real leaf on the host and pass the result through
//! [`filter`]. Every bit cleared here names a feature whose instructions the
//! VMM intercepts and does not emulate; advertising it would steer a correct
//! guest into an exit that stops the VM:
//!
//! - **XSAVE/OSXSAVE/AVX** (leaf 1, leaf 0xD): `XSETBV` always exits and the
//!   world switch keeps no per-vCPU extended state, so the guest stays on
//!   FXSAVE-managed x87/SSE and can never widen the physical XCR0.
//! - **MONITOR/MWAIT** (leaf 1): intercepted on SVM so an idle guest cannot
//!   park the physical CPU outside host control.
//! - **VMX / SVM** (leaf 1, 0x8000_0001, 0x8000_000A): nested virtualization is
//!   not provided; every VMX/SVM instruction exits.
//! - **x2APIC** (leaf 1): the guest LAPIC is the RAM-backed xAPIC MMIO window.
//! - **TSC-deadline timer** (leaf 1): `IA32_TSC_DEADLINE` is an unemulated MSR
//!   whose writes are dropped, so a guest that knows its TSC frequency and
//!   picks this clockevent never receives a timer interrupt again (its boot
//!   stopped after the virtio-gpu probe). Hidden, Linux keeps the PIT tick.
//! - **Hypervisor leaves** (0x4000_0000..=0x4FFF_FFFF): when Cellos itself
//!   runs under KVM/Hyper-V, the host's leaves name that outer hypervisor; a
//!   guest believing it would use its paravirtual MSRs (kvm-clock), which this
//!   VMM does not implement.
//! - **INVPCID, WAITPKG** (leaf 7): on VMX each needs its own secondary
//!   execution control, without which the instruction raises #UD in the guest
//!   (Linux flushes the TLB with INVPCID during early boot). Linux falls back
//!   to CR4/INVLPG flushes and PAUSE loops.

/// Host CPUID result in `[eax, ebx, ecx, edx]` order.
pub type CpuidRegs = [u32; 4];

const LEAF1_ECX_MONITOR: u32 = 1 << 3;
const LEAF1_ECX_VMX: u32 = 1 << 5;
const LEAF1_ECX_X2APIC: u32 = 1 << 21;
const LEAF1_ECX_TSC_DEADLINE: u32 = 1 << 24;
const LEAF7_EBX_INVPCID: u32 = 1 << 10;
const LEAF7_ECX_WAITPKG: u32 = 1 << 5;
const LEAF1_ECX_XSAVE: u32 = 1 << 26;
const LEAF1_ECX_OSXSAVE: u32 = 1 << 27;
const LEAF1_ECX_AVX: u32 = 1 << 28;
const EXT1_ECX_SVM: u32 = 1 << 2;

const LEAF1_ECX_HIDDEN: u32 = LEAF1_ECX_MONITOR
    | LEAF1_ECX_VMX
    | LEAF1_ECX_X2APIC
    | LEAF1_ECX_TSC_DEADLINE
    | LEAF1_ECX_XSAVE
    | LEAF1_ECX_OSXSAVE
    | LEAF1_ECX_AVX;

/// Apply the guest CPUID policy to the host result for `leaf`/`subleaf`.
pub fn filter(leaf: u32, subleaf: u32, regs: CpuidRegs) -> CpuidRegs {
    let [eax, ebx, ecx, edx] = regs;
    match (leaf, subleaf) {
        (0x1, _) => [eax, ebx, ecx & !LEAF1_ECX_HIDDEN, edx],
        // XSAVE state enumeration and the SVM feature leaf describe hidden
        // features; an all-zero leaf reads as "nothing supported".
        (0xD | 0x8000_000A, _) => [0; 4],
        (0x4000_0000..=0x4FFF_FFFF, _) => [0; 4],
        (0x7, 0) => [eax, ebx & !LEAF7_EBX_INVPCID, ecx & !LEAF7_ECX_WAITPKG, edx],
        (0x8000_0001, _) => [eax, ebx, ecx & !EXT1_ECX_SVM, edx],
        _ => regs,
    }
}

/// Execute CPUID on the host and apply [`filter`].
#[inline]
pub fn query(leaf: u32, subleaf: u32) -> CpuidRegs {
    let r = core::arch::x86_64::__cpuid_count(leaf, subleaf);
    filter(leaf, subleaf, [r.eax, r.ebx, r.ecx, r.edx])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf1_hides_intercepted_features_and_keeps_the_rest() {
        let [_, _, ecx, edx] = filter(0x1, 0, [1, 2, u32::MAX, 0xDEAD_BEEF]);
        assert_eq!(ecx & LEAF1_ECX_HIDDEN, 0);
        assert_eq!(ecx | LEAF1_ECX_HIDDEN, u32::MAX);
        assert_eq!(edx, 0xDEAD_BEEF);
    }

    #[test]
    fn nested_virt_and_xsave_leaves_are_hidden() {
        assert_eq!(
            filter(0x8000_0001, 0, [0, 0, u32::MAX, 0])[2] & EXT1_ECX_SVM,
            0
        );
        assert_eq!(filter(0x8000_000A, 0, [u32::MAX; 4]), [0; 4]);
        assert_eq!(filter(0xD, 0, [u32::MAX; 4]), [0; 4]);
    }

    #[test]
    fn outer_hypervisor_leaves_are_hidden_and_unrelated_leaves_pass_through() {
        assert_eq!(filter(0x4000_0000, 0, [0x4000_0001, 1, 2, 3]), [0; 4]);
        assert_eq!(filter(0x4000_0101, 0, [1; 4]), [0; 4]);
        let regs = [1, 2, 3, 4];
        assert_eq!(filter(0x6, 0, regs), regs);
        let [_, ebx, ecx, edx] = filter(0x7, 0, [0, u32::MAX, u32::MAX, u32::MAX]);
        assert_eq!((ebx | LEAF7_EBX_INVPCID, ecx | LEAF7_ECX_WAITPKG), (u32::MAX, u32::MAX));
        assert_eq!((ebx & LEAF7_EBX_INVPCID, ecx & LEAF7_ECX_WAITPKG, edx), (0, 0, u32::MAX));
        assert_eq!(filter(0x5000_0000, 0, regs), regs);
        assert_eq!(filter(0x7, 1, regs), regs, "leaf 7 subleaf 1 is not the feature subleaf");
    }
}
