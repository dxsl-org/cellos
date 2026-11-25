// SPDX-License-Identifier: MIT
//! Guest x87/SSE state across VM entries.
//!
//! Neither VMX nor SVM switches x87/SSE state, and the syscall that runs a
//! vCPU (`RunVcpu`) saves the calling Cell's FXSAVE image on entry, runs the
//! kernel after `fninit` with a default MXCSR, and restores the Cell's image on
//! return. Without its own image the guest would resume each run on the
//! Cell's registers and lose what it left in them at every exit that returns
//! to the Cell — silently corrupting guest user code that keeps live values in
//! XMM registers. Each vCPU therefore owns an FXSAVE image: restored right
//! before entry, saved right after exit. The guest CPUID policy hides
//! XSAVE/AVX, so the legacy 512-byte image is the whole guest FPU state.

/// One FXSAVE64 image.
#[repr(C, align(16))]
pub struct GuestFpu([u8; 512]);

/// FCW after `fninit` (all exceptions masked, 64-bit precision).
const FCW_DEFAULT: u16 = 0x037F;
/// MXCSR at reset (all SIMD exceptions masked, round to nearest).
const MXCSR_DEFAULT: u32 = 0x1F80;

impl Default for GuestFpu {
    /// The state a CPU has after reset: what a guest boots with.
    fn default() -> Self {
        let mut image = [0u8; 512];
        image[0..2].copy_from_slice(&FCW_DEFAULT.to_le_bytes());
        image[24..28].copy_from_slice(&MXCSR_DEFAULT.to_le_bytes());
        Self(image)
    }
}

impl GuestFpu {
    /// Load this image into the x87/SSE registers.
    ///
    /// # Safety
    /// IF=0 until the matching [`Self::save`]; no host code that uses x87/SSE
    /// may run in between (the kernel is built without them).
    #[inline]
    pub(super) unsafe fn load(&self) {
        core::arch::asm!("fxrstor64 [{}]", in(reg) self.0.as_ptr(), options(nostack, readonly));
    }

    /// Store the x87/SSE registers into this image.
    ///
    /// # Safety
    /// As for [`Self::load`]; called right after the guest exits.
    #[inline]
    pub(super) unsafe fn save(&mut self) {
        core::arch::asm!("fxsave64 [{}]", in(reg) self.0.as_mut_ptr(), options(nostack));
    }
}

/// Bytes of a standard-format XSAVE image holding x87, SSE and AVX state:
/// the legacy region (512), the XSAVE header (64: XSTATE_BV, then XCOMP_BV,
/// which must stay 0 for XRSTOR), the upper YMM halves (256).
const XSTATE_BYTES: usize = 832;
/// The AVX component's bit in XCR0 and in an instruction's feature mask.
const COMPONENT_AVX: u32 = 1 << 2;

/// A VMX vCPU's x87, SSE and AVX state between runs.
///
/// The legacy region is the FXSAVE image (`FXSAVE`/`FXRSTOR` move x87 and
/// SSE whatever the guest's XCR0); the AVX component moves with
/// `XSAVE`/`XRSTOR` under the guest's XCR0, which then masks it out until the
/// guest enables AVX (`XSETBV`). Standard format: the AVX component sits at
/// byte 576 (SDM Vol. 1, 13.4).
#[repr(C, align(64))]
pub struct GuestXState([u8; XSTATE_BYTES]);

impl Default for GuestXState {
    /// Reset state: the legacy defaults of [`GuestFpu`], an all-zero header
    /// (every component in its initial configuration).
    fn default() -> Self {
        let mut image = [0u8; XSTATE_BYTES];
        image[..512].copy_from_slice(&GuestFpu::default().0);
        Self(image)
    }
}

impl GuestXState {
    /// Load this state. With `avx`, the AVX component comes back too, as far
    /// as the XCR0 now in place enables it.
    ///
    /// # Safety
    /// IF=0 until the matching [`Self::save`]; no host code that uses x87,
    /// SSE or AVX may run in between. With `avx`, CR4.OSXSAVE is set and
    /// XCR0 is the guest's.
    #[inline]
    pub unsafe fn load(&self, avx: bool) {
        core::arch::asm!("fxrstor64 [{}]", in(reg) self.0.as_ptr(), options(nostack, readonly));
        if avx {
            core::arch::asm!("xrstor64 [{}]", in(reg) self.0.as_ptr(),
                in("eax") COMPONENT_AVX, in("edx") 0u32, options(nostack, readonly));
        }
    }

    /// Store the state [`Self::load`] put in place, then clear the upper YMM
    /// halves so nothing of the guest's vectors stays in the registers.
    ///
    /// # Safety
    /// As for [`Self::load`], with the same `avx` and XCR0.
    #[inline]
    pub unsafe fn save(&mut self, avx: bool, xcr0: u64) {
        if avx {
            core::arch::asm!("xsave64 [{}]", in(reg) self.0.as_mut_ptr(),
                in("eax") COMPONENT_AVX, in("edx") 0u32, options(nostack));
        }
        core::arch::asm!("fxsave64 [{}]", in(reg) self.0.as_mut_ptr(), options(nostack));
        // VZEROUPPER is a VEX instruction: #UD unless XCR0 enables SSE and AVX.
        if avx && xcr0 & 0b110 == 0b110 {
            core::arch::asm!("vzeroupper", options(nomem, nostack, preserves_flags));
        }
    }
}
