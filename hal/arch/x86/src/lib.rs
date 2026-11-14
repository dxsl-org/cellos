#![no_std]

pub mod common;
pub mod x86_32;
pub mod x86_64;

#[cfg(feature = "critical-section-impl")]
mod critical_section;

#[cfg(target_arch = "x86")]
pub use x86_32::*;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;
