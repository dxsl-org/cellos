#![no_std]

// common/ contains RISC-V-specific SBI calls and CSR asm — only compile for
// RISC-V targets; building for AArch64/x86 in the same workspace must not
// see these inline asm register names (a0-a7) which are invalid there.
#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
pub mod common;

#[cfg(feature = "critical-section-impl")]
mod critical_section;

// Architecture-specific modules are gated by target_arch so that, e.g.,
// rv64's 64-bit shifts and literals are never compiled for a riscv32 target
// (they would overflow usize). Each arch module is both declared and
// re-exported only when its target is active.
#[cfg(target_arch = "riscv64")]
pub mod rv64;
#[cfg(target_arch = "riscv64")]
pub use rv64::*;

#[cfg(target_arch = "riscv32")]
pub mod rv32;
#[cfg(target_arch = "riscv32")]
pub use rv32::*;

// ViHypervisor ENOSYS stub — makes the multi-arch trait contract explicit.
// Real H-extension impl is pending; kernel/src/hypervisor/registry.rs handles
// NotSupported at syscall dispatch on riscv64 today.
#[cfg(target_arch = "riscv64")]
pub mod hypervisor;
#[cfg(target_arch = "riscv64")]
pub use hypervisor::RiscV64Hypervisor;
