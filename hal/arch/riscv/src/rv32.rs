//! RISC-V 32-bit (RV32) Hardware Abstraction Layer.
//!
//! Targets `riscv32imac-unknown-none-elf` on QEMU virt + OpenSBI (S-mode).
//! Sub-modules are gated on `#[cfg(target_arch = "riscv32")]` so building
//! for any other target still compiles this file without inline asm errors.

use hal_arch_trait::Arch;

/// RV32 architecture implementation.
pub struct RiscV32Arch;

pub type PlatformArch = RiscV32Arch;
pub static ARCH: PlatformArch = RiscV32Arch;

// ── Stub for non-RV32 targets (compile-time only) ─────────────────────────────
#[cfg(not(target_arch = "riscv32"))]
impl Arch for RiscV32Arch {
    type Context = usize;
    fn init(&self) {}
    unsafe fn switch_context(&self, _: *mut Self::Context, _: *const Self::Context) {}
    fn enable_interrupts(&self) {}
    fn disable_interrupts(&self) {}
    fn wait_for_interrupt(&self) {}
    fn interrupts_enabled(&self) -> bool { false }
}

// ── RV32-specific sub-modules ─────────────────────────────────────────────────
#[cfg(target_arch = "riscv32")]
pub mod boot;
/// Arch-level re-exports for the kernel (mirrors rv64's `pub mod arch` layout).
/// Aliasing ViTrapFrame32 as ViTrapFrame lets kernel code use one name on both arches.
#[cfg(target_arch = "riscv32")]
pub mod arch {
    // ViTrapFrame alias: kernel code uses `hal::arch::ViTrapFrame` on both arches.
    pub use crate::rv32::trap::ViTrapFrame32 as ViTrapFrame;
    pub use crate::rv32::trap::*;
    // Context alias: Rv32Context is defined in the parent rv32 module.
    pub use crate::rv32::Rv32Context as Context;
    extern "C" {
        pub fn thread_trampoline();
    }

    /// Read the current GP and TP registers.
    pub fn get_gp_tp() -> (usize, usize) {
        let gp: usize;
        let tp: usize;
        unsafe {
            // SAFETY: reading gp/tp is always safe — they are callee-saved registers.
            core::arch::asm!("mv {0}, gp", out(reg) gp);
            core::arch::asm!("mv {0}, tp", out(reg) tp);
        }
        (gp, tp)
    }

    /// Write the kernel-stack top into sscratch before a context switch to user mode.
    pub fn set_kernel_stack(kernel_stack_top: usize) {
        unsafe {
            // SAFETY: writing sscratch from S-mode is the standard mechanism for
            // saving the kernel stack pointer across U-mode execution.
            core::arch::asm!("csrw sscratch, {}", in(reg) kernel_stack_top, options(nomem, nostack));
        }
    }

    /// Enable S-mode interrupts (SIE bit in sstatus).
    pub fn enable_interrupts() {
        unsafe {
            // SAFETY: csrsi sstatus SIE (bit 1) — standard S-mode interrupt enable.
            core::arch::asm!("csrsi sstatus, 0x2", options(nomem, nostack));
        }
    }
}

// Re-export common SBI so `hal::sbi` resolves on riscv32 (mirrors rv64).
#[cfg(target_arch = "riscv32")]
pub use crate::common::sbi;
#[cfg(target_arch = "riscv32")]
pub use crate::common::timer;
#[cfg(target_arch = "riscv32")]
pub mod context;
#[cfg(target_arch = "riscv32")]
pub mod trap;
#[cfg(target_arch = "riscv32")]
mod asm;

// ── Full implementation for RV32 ──────────────────────────────────────────────
#[cfg(target_arch = "riscv32")]
impl Arch for RiscV32Arch {
    type Context = Rv32Context;

    fn init(&self) {
        // Install trap vector (stvec) and clear sscratch.
        trap::init();

        // Enable S-mode software interrupt (SSIE) for zero-latency RT preemption
        // (same as RV64: kernel asserts sip.SSIP to force a yield on RT cells).
        // SAFETY: csrsi sie bit 1 (SSIE) is valid in S-mode.
        unsafe { core::arch::asm!("csrsi sie, 0x2", options(nomem, nostack)); }
    }

    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        Rv32Context::switch(old, new);
    }

    fn enable_interrupts(&self) {
        // SAFETY: csrsi sstatus SIE (bit 1) — standard S-mode interrupt enable.
        unsafe { core::arch::asm!("csrsi sstatus, 0x2", options(nomem, nostack)); }
    }

    fn disable_interrupts(&self) {
        // SAFETY: csrci sstatus SIE (bit 1) — standard S-mode interrupt disable.
        unsafe { core::arch::asm!("csrci sstatus, 0x2", options(nomem, nostack)); }
    }

    fn wait_for_interrupt(&self) {
        // SAFETY: wfi is safe and available in RV32 S-mode.
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }
    }

    fn interrupts_enabled(&self) -> bool {
        let sstatus: u32;
        // SAFETY: csrr sstatus reads the current status without any side effects.
        unsafe { core::arch::asm!("csrr {}, sstatus", out(reg) sstatus, options(nomem, nostack)); }
        sstatus & (1 << 1) != 0 // SIE bit
    }
}

/// Minimal CPU context for RV32 cooperative + preemptive context switching.
///
/// Field order MUST match `rv32/asm/switch.S` exactly.
/// Size assertion: 19 fields × 4 bytes = 76 bytes.
#[cfg(target_arch = "riscv32")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Rv32Context {
    pub ra: u32,
    pub sp: u32,
    pub s0: u32, pub s1: u32, pub s2: u32, pub s3: u32,
    pub s4: u32, pub s5: u32, pub s6: u32, pub s7: u32,
    pub s8: u32, pub s9: u32, pub s10: u32, pub s11: u32,
    /// Saved program counter (for tasks resumed via sret after pre-emption).
    pub sepc: u32,
    /// Saved interrupt enable state.
    pub sstatus: u32,
    pub gp: u32,
    pub tp: u32,
    pub sscratch: u32,
}

#[cfg(target_arch = "riscv32")]
const _: () = assert!(core::mem::size_of::<Rv32Context>() == 76);

/// Page size for SV32: 4 KiB.
pub const PAGE_SIZE: usize = 4096;
/// Superpage size for SV32: 4 MiB (level-1 leaf entry).
pub const SUPERPAGE_SIZE: usize = 4 * 1024 * 1024;
