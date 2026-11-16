// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the DMA remapper behind `crate::dev::iommu` on the 32-bit targets,
//! which have no kernel IOMMU backend.

pub use crate::dev::iommu::NoRemapper as Backend;

/// No remapper.
pub const BACKEND: Backend = Backend;
