#![no_std]

pub mod aarch32;
pub mod aarch64;
pub mod common;

#[cfg(feature = "critical-section-impl")]
mod critical_section;

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

#[cfg(target_arch = "arm")]
pub use aarch32::*;
