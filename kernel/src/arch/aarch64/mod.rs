// SPDX-License-Identifier: MPL-2.0
//! aarch64 implementation of the `crate::arch` function sets.
//!
//! The submodules mirror the kernel module each file came from
//! (`crate::hypervisor::pending_irqs` is now `crate::arch::hypervisor::pending_irqs`).

/// Layer-2 hardware security self-tests (test-hooks only).
#[cfg(feature = "test-hooks")]
pub mod layer2_selftest;

pub mod boot;

pub mod smp;

pub mod tlb;

pub mod power;

pub mod cpu;

pub mod clock;

pub mod irq;

pub mod user_access;

pub mod page_probe;

pub mod iommu;

pub mod virtio_mmio;

pub mod gpio_irq;

pub mod sdhci;

pub mod hart_local;

pub mod vm;

pub mod sched;

pub mod dev {
    pub(crate) mod bcm_pinmux;
    pub mod mmc {
        pub(crate) mod pinmux_bcm;
    }
}

pub mod hypervisor {
    pub mod pending_irqs;
    #[cfg(feature = "test-hooks")]
    pub mod smoke_guest;
}
