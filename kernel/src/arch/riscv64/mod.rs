// SPDX-License-Identifier: MPL-2.0
//! riscv64 implementation of the `crate::arch` function sets.
//!
//! The submodules mirror the kernel module each file came from
//! (`crate::dev::iommu_riscv` is now `crate::arch::dev::iommu_riscv`).

pub mod clock;

pub mod irq;

pub mod user_access;

pub mod page_probe;

pub mod cell {
    #[cfg(feature = "test-hooks")]
    pub mod provenance_selftest;
}

pub mod dev {
    pub mod iommu_riscv; // RISC-V IOMMU — 3-level DDT + Sv39 first-stage
    mod iommu_riscv_cmd;
    pub mod irq_dispatch;
}

pub mod memory {
    #[cfg(feature = "test-hooks")]
    pub mod tlb_shootdown_selftest;
}

pub mod task;
