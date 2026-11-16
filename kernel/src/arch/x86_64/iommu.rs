// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the DMA remapper behind `crate::dev::iommu` on x86_64 (Intel VT-d).

pub use crate::arch::dev::iommu_x86::Vtd as Backend;

/// The machine's VT-d remapper.
pub const BACKEND: Backend = Backend;
