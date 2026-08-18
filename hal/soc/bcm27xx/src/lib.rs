#![no_std]

mod irq_topology;
mod profile;
mod sdhci_policy;

pub use irq_topology::Bcm27xxIrqTopology;
pub use profile::{Bcm27xxMmioLayout, Bcm27xxSocProfile, BCM2837};
pub use sdhci_policy::SdhciAccessPolicy;

#[cfg(test)]
mod tests;
