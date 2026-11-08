#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
extern crate alloc;

pub mod ioctl_defs;
pub mod query;
pub mod shim;

#[cfg(test)]
mod tests;
