#![no_std]

//! Immutable RISC-V SoC profile facts for early platform discovery.
//!
//! The crate is data-only: it carries compatible lookup lists and fail-closed
//! access policies without embedding board wiring, memory maps, or driver code.

mod access_policy;
mod catalog;
mod plic_policy;
mod profile;
mod sdhci_policy;

pub use access_policy::{RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy};
pub use catalog::{GENERIC_VIRT, JH7110, SG2042};
pub use plic_policy::PlicContextPolicy;
pub use profile::RiscvSocProfile;
pub use sdhci_policy::RiscvSdhciProfile;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
