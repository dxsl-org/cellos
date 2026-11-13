// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Cell-side virtio 1.x PCI transport (ADR-0031 phase 05).
//!
//! A Driver Cell runs in ring 3 and cannot touch PCI configuration space; all
//! it knows about its function is the [`abi::pcie_function::ViPcieFunctionV1`]
//! record returned by `QueryPcieFunction` (434). This crate turns that record
//! into a [`virtio_drivers::transport::Transport`]:
//!
//! - [`caps`] decodes the virtio vendor capabilities copied into the record
//!   and proves every structure lies inside a memory BAR.
//! - [`identity`] maps PCI vendor/device IDs to a virtio [`DeviceType`].
//! - [`features`] holds the feature policy: `VERSION_1` is mandatory and
//!   `ACCESS_PLATFORM` is always accepted when offered.
//! - [`VirtioPciTransport`] maps the BARs through a caller-supplied
//!   [`BarMapper`] and drives the common configuration structure. Interrupts
//!   are disabled at the device (`NO_VECTOR`); drivers poll.
//!
//! [`DeviceType`]: virtio_drivers::transport::DeviceType
#![no_std]

#[cfg(test)]
extern crate std;

pub mod caps;
mod error;
pub mod features;
pub mod identity;
mod regs;
mod transport;

pub use caps::{parse_caps, CapRegion, CfgType, VirtioCaps};
pub use error::{InitFailure, TransportError};
pub use features::AccessPlatform;
pub use identity::{device_type_for, VIRTIO_VENDOR_ID};
pub use regs::{COMMON_CFG_LEN, NO_VECTOR, RESET_SPIN_BUDGET};
pub use transport::{BarMapper, VirtioPciTransport};

#[cfg(test)]
mod tests;
