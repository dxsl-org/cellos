// SPDX-License-Identifier: MPL-2.0
//! aarch64 implementation of the `crate::arch` function sets.
//!
//! The submodules mirror the kernel module each file came from
//! (`crate::hypervisor::pending_irqs` is now `crate::arch::hypervisor::pending_irqs`).

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
