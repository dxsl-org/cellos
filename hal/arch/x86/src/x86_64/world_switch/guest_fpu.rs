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
