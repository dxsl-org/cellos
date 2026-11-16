// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the DMA remapper behind `crate::dev::iommu` on aarch64, which has
//! no kernel IOMMU backend (VirtIO DMA runs untranslated; DMA grants are
//! refused).

pub use crate::dev::iommu::NoRemapper as Backend;

/// No remapper.
pub const BACKEND: Backend = Backend;
