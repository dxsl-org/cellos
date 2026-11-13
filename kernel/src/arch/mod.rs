// SPDX-License-Identifier: MPL-2.0
//! Architecture seam (Phase 14).
//!
//! Every architecture difference in the kernel lives under this directory. Each
//! `<arch>/` module exports the same function sets (`irq`, `clock`, `power`,
//! `smp`, `tlb`, `user_access`, `page_probe`, ...) with the same signatures,
//! and exactly one of them is selected here. Shared code calls
//! `crate::arch::<set>::<fn>()` and names no architecture.
//!
//! `crate::hal::arch` is the HAL's own per-architecture facade (`Context`,
//! `ViTrapFrame`, ...); it is spelled in full so the two never collide.
//!
//! Each arch module is glob re-exported (`pub use <arch>::*`) once it has
//! content.
//!
//! `legacy` holds the 32-bit code (riscv32, arm, x86). It is selected by
//! `not(cellos_wide)` and, like before the move, does not build today.

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

#[cfg(target_arch = "aarch64")]
mod aarch64;

#[cfg(target_arch = "riscv64")]
mod riscv64;

#[cfg(not(cellos_wide))]
mod legacy;
#[cfg(not(cellos_wide))]
pub use legacy::*;
