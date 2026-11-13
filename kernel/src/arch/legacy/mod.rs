// SPDX-License-Identifier: MPL-2.0
//! 32-bit targets (riscv32, arm, x86): quarantined, not built by any lane.

#[cfg(target_arch = "x86")]
pub mod dev;
