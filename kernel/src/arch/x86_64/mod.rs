// SPDX-License-Identifier: MPL-2.0
//! x86_64 implementation of the `crate::arch` function sets.
//!
//! The submodules mirror the kernel module each file came from
//! (`crate::arch::dev::pcie_msi` is now `crate::arch::dev::pcie_msi`).

pub mod page_probe;

pub mod acpi {
    // The G1 private lane does not call `sci::init` (its x86 contract is pinned),
    // so the discovery half of these modules is unused there.
    #[cfg_attr(feature = "g1-cpu-accounting", allow(dead_code))]
    pub mod power;
    #[cfg_attr(feature = "g1-cpu-accounting", allow(dead_code))]
    pub mod sci;
}

pub mod boot {
    pub mod limine;
}

pub mod dev {
    pub mod console_select;
    pub mod fb_console;
    pub mod fb_font;
    pub mod iommu_x86; // Intel VT-d — TT=TRANSLATED + Sv39 SLPT
    pub mod pcie_msi; // PCIe function record (434) and MSI/MSI-X allocation (435)
    pub mod ps2_keyboard;
}

pub mod hypervisor {
    /// x86 SVM VM store + run loop (Tier 3 P03), the x86 twin of the aarch64
    /// branch in [`crate::hypervisor::registry`].
    pub mod svm_registry;
}

pub mod memory {
    /// x86 nested paging (EPT/NPT) — Tier 3 x86 VMM.
    pub mod ept;
}

pub mod task {
    #[cfg(feature = "x86-idt-cpl3-test")]
    pub(crate) mod x86_idt_cpl3;
}
