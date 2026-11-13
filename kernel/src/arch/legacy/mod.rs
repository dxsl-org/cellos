// SPDX-License-Identifier: MPL-2.0
//! 32-bit targets (riscv32, arm, x86): quarantined, not built by any lane.

pub mod clock;
pub mod cpu;
#[cfg(target_arch = "x86")]
pub mod dev;
pub mod irq;
pub mod power;

#[cfg(target_arch = "riscv32")]
mod riscv32;
#[cfg(target_arch = "riscv32")]
pub use riscv32::*;

#[cfg(target_arch = "x86")]
mod x86;
#[cfg(target_arch = "x86")]
pub use x86::*;
