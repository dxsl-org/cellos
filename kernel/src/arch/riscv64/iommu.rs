// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the DMA remapper behind `crate::dev::iommu` on riscv64 (RISC-V
//! IOMMU).

pub use crate::arch::dev::iommu_riscv::RiscvIommu as Backend;

/// The machine's RISC-V IOMMU.
pub const BACKEND: Backend = Backend;
