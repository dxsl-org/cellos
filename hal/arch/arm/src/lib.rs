#![no_std]

pub mod common;
pub mod aarch64;
pub mod aarch32;

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

#[cfg(target_arch = "arm")]
pub use aarch32::*;
