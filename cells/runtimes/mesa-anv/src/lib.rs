//! Mesa `anv` DRM transport shim: render-node calls forwarded to the Intel GPU driver Cell
//! over the `XE_GPU` service (ADR-0033).
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod memory;
pub mod shim;
pub mod transport;

#[cfg(test)]
mod tests;
