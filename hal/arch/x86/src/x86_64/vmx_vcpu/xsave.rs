// SPDX-License-Identifier: MIT
//! AVX for a VMX guest: its XCR0, the `XSETBV` exits that set it, and the
//! CPUID leaves that describe it (SDM Vol. 1 ch. 13, "Managing State Using the
//! XSAVE Feature Set"; Vol. 3C 26.1.2: `XSETBV` always exits).
//!
//! A guest may enable x87, SSE and AVX state and nothing else (no AVX-512,
//! AMX or supervisor components), which is what [`super::GuestXState`] holds.
//! Its XCR0 is in the register for a whole run (`VmxVcpu::run`); the host's
//! is x87 | SSE (`init_sse`).

use types::ViResult;

use super::super::vmcs::*;
use super::super::world_switch::xsetbv0;
use super::{queue_gp, VmxVcpu};

const XCR0_X87: u64 = 1 << 0;
const XCR0_SSE: u64 = 1 << 1;
const XCR0_AVX: u64 = 1 << 2;
/// The state components a guest may enable.
const GUEST_COMPONENTS: u64 = XCR0_X87 | XCR0_SSE | XCR0_AVX;
/// XCR0 after reset.
pub(super) const XCR0_RESET: u64 = XCR0_X87;
const CR4_OSXSAVE: u64 = 1 << 18;

const LEAF1_ECX_XSAVE: u32 = 1 << 26;
const LEAF1_ECX_OSXSAVE: u32 = 1 << 27;
const LEAF1_ECX_AVX: u32 = 1 << 28;
/// Leaf 7.0 features whose state is outside x87/SSE/AVX: AVX-512 (EBX:
/// F, DQ, IFMA, PF, ER, CD, BW, VL; ECX: VBMI, VBMI2, VNNI, BITALG,
/// VPOPCNTDQ; EDX: 4VNNIW, 4FMAPS, VP2INTERSECT, FP16) and AMX (EDX: BF16,
/// TILE, INT8).
const LEAF7_EBX_HIDDEN: u32 =
    (1 << 16) | (1 << 17) | (1 << 21) | (1 << 26) | (1 << 27) | (1 << 28) | (1 << 30) | (1 << 31);
const LEAF7_ECX_HIDDEN: u32 = (1 << 1) | (1 << 6) | (1 << 11) | (1 << 12) | (1 << 14);
const LEAF7_EDX_HIDDEN: u32 =
    (1 << 2) | (1 << 3) | (1 << 8) | (1 << 22) | (1 << 23) | (1 << 24) | (1 << 25);
/// Leaf 7.1 EAX: AVX512_BF16.
const LEAF7_1_EAX_HIDDEN: u32 = 1 << 5;
/// Leaf 0xD.1 EAX: XSAVEOPT only (no XSAVEC, no XGETBV with ECX=1, no
/// XSAVES, which would need IA32_XSS and its own execution control).
const LEAF_D1_EAX_SHOWN: u32 = 1 << 0;
/// Standard-format layout: legacy region and header, then AVX at 576.
const LEGACY_AND_HEADER: u32 = 512 + 64;
const AVX_OFFSET: u32 = 576;
const AVX_BYTES: u32 = 256;

/// Whether this CPU can give guests AVX: the host turned XSAVE on
/// (`init_sse`) and the CPU has AVX state.
pub(super) fn host_offers_avx() -> bool {
    let cr4: u64;
    // SAFETY: reading CR4 has no side effect; the HAL runs at CPL 0.
    unsafe { core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack)) };
    let leaf1 = core::arch::x86_64::__cpuid(1);
    let leaf_d = core::arch::x86_64::__cpuid_count(0xD, 0);
    cr4 & CR4_OSXSAVE != 0
        && leaf1.ecx & LEAF1_ECX_AVX != 0
        && u64::from(leaf_d.eax) & GUEST_COMPONENTS == GUEST_COMPONENTS
}

/// Whether `value` is an XCR0 a guest may set: x87 on, nothing outside
/// x87/SSE/AVX, AVX only with SSE.
fn valid_xcr0(value: u64) -> bool {
    value & XCR0_X87 != 0
        && value & !GUEST_COMPONENTS == 0
        && (value & XCR0_AVX == 0 || value & XCR0_SSE != 0)
}

/// Bytes of an XSAVE image for the components of `xcr0`.
fn image_bytes(xcr0: u64) -> u32 {
    if xcr0 & XCR0_AVX != 0 {
        AVX_OFFSET + AVX_BYTES
    } else {
        LEGACY_AND_HEADER
    }
}

/// CPUID as a guest with AVX sees it, from the policy's result `regs`:
/// XSAVE and AVX shown, OSXSAVE following the guest's CR4, AVX-512 and AMX
/// hidden, leaf 0xD describing x87/SSE/AVX under the guest's `xcr0`.
pub(super) fn cpuid(leaf: u32, subleaf: u32, regs: [u32; 4], xcr0: u64, osxsave: bool) -> [u32; 4] {
    let [eax, ebx, ecx, edx] = regs;
    match (leaf, subleaf) {
        (1, _) => {
            let mut ecx = ecx | LEAF1_ECX_XSAVE | LEAF1_ECX_AVX;
            if osxsave {
                ecx |= LEAF1_ECX_OSXSAVE;
            } else {
                ecx &= !LEAF1_ECX_OSXSAVE;
            }
            [eax, ebx, ecx, edx]
        }
        (7, 0) => [
            eax,
            ebx & !LEAF7_EBX_HIDDEN,
            ecx & !LEAF7_ECX_HIDDEN,
            edx & !LEAF7_EDX_HIDDEN,
        ],
        (7, 1) => [eax & !LEAF7_1_EAX_HIDDEN, ebx, ecx, edx],
        (0xD, 0) => [
            GUEST_COMPONENTS as u32,
            image_bytes(xcr0),
            image_bytes(GUEST_COMPONENTS),
            0,
        ],
        (0xD, 1) => [LEAF_D1_EAX_SHOWN, image_bytes(xcr0), 0, 0],
        (0xD, 2) => [AVX_BYTES, AVX_OFFSET, 0, 0],
        (0xD, _) => [0; 4],
        _ => regs,
    }
}

impl VmxVcpu {
    /// The guest's `XSETBV`: a valid value becomes its XCR0, in the register
    /// at once (its XCR0 is live for the whole run). Returns whether the
    /// instruction completed; otherwise #UD (no XSAVE for this guest, or
    /// CR4.OSXSAVE clear) or #GP(0) (CPL above 0, ECX not 0, an XCR0 the
    /// guest may not set) is queued and RIP stays.
    ///
    /// # Safety
    /// This vCPU's VMCS is current; the guest's XCR0 is in the register.
    pub(super) unsafe fn handle_xsetbv(&mut self) -> ViResult<bool> {
        if !self.avx || vmread(GUEST_CR4)? & CR4_OSXSAVE == 0 {
            queue_ud()?;
            return Ok(false);
        }
        let cpl = (vmread(GUEST_SS_AR_BYTES)? >> 5) & 0b11;
        let value = (self.gpr[2] << 32) | (self.gpr[0] & 0xFFFF_FFFF);
        if cpl != 0 || self.gpr[1] as u32 != 0 || !valid_xcr0(value) {
            queue_gp()?;
            return Ok(false);
        }
        self.guest_xcr0 = value;
        xsetbv0(value);
        Ok(true)
    }
}

/// Queue #UD for the next entry (no error code).
///
/// # Safety
/// The target VMCS is current.
unsafe fn queue_ud() -> ViResult<()> {
    const UD: u64 = (1 << 31) | (3 << 8) | 6;
    vmwrite(VM_ENTRY_INTR_INFO_FIELD, UD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_xcr0_keeps_x87_and_takes_avx_only_with_sse() {
        assert!(valid_xcr0(0b001));
        assert!(valid_xcr0(0b011));
        assert!(valid_xcr0(0b111));
        assert!(!valid_xcr0(0b000), "x87 cannot be off");
        assert!(!valid_xcr0(0b101), "AVX needs SSE");
        assert!(!valid_xcr0(0b1110_0111), "AVX-512 components");
        assert!(!valid_xcr0(1 << 63 | 1));
    }

    #[test]
    fn leaf_1_shows_avx_and_follows_the_guests_osxsave() {
        let [_, _, ecx, edx] = cpuid(1, 0, [0, 0, 0, 7], 1, false);
        assert_eq!(
            ecx & (LEAF1_ECX_XSAVE | LEAF1_ECX_AVX | LEAF1_ECX_OSXSAVE),
            LEAF1_ECX_XSAVE | LEAF1_ECX_AVX
        );
        assert_eq!(edx, 7);
        assert_ne!(cpuid(1, 0, [0; 4], 1, true)[2] & LEAF1_ECX_OSXSAVE, 0);
    }

    #[test]
    fn leaf_d_sizes_follow_the_guests_xcr0() {
        assert_eq!(cpuid(0xD, 0, [u32::MAX; 4], 0b011, true), [7, 576, 832, 0]);
        assert_eq!(cpuid(0xD, 0, [u32::MAX; 4], 0b111, true)[1], 832);
        assert_eq!(cpuid(0xD, 1, [u32::MAX; 4], 0b111, true), [1, 832, 0, 0]);
        assert_eq!(cpuid(0xD, 2, [0; 4], 0b111, true), [256, 576, 0, 0]);
        assert_eq!(
            cpuid(0xD, 5, [u32::MAX; 4], 0b111, true),
            [0; 4],
            "AVX-512 opmask"
        );
    }

    #[test]
    fn leaf_7_hides_avx512_and_amx_and_keeps_avx2() {
        const AVX2: u32 = 1 << 5;
        let [_, ebx, ecx, edx] = cpuid(7, 0, [0, u32::MAX, u32::MAX, u32::MAX], 7, true);
        assert_ne!(ebx & AVX2, 0);
        assert_eq!(ebx & (1 << 16), 0, "AVX512F");
        assert_eq!((ecx & LEAF7_ECX_HIDDEN, edx & LEAF7_EDX_HIDDEN), (0, 0));
        assert_eq!(cpuid(6, 0, [1, 2, 3, 4], 7, true), [1, 2, 3, 4]);
    }
}
