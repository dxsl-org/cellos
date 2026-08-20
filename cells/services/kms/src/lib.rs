#![no_std]
#![forbid(unsafe_code)]

//! Fail-closed runtime boundary for the Cellos node-identity KMS.
//!
//! This slice deliberately keeps the root backend fail-closed. It proves
//! service lifecycle, attested authorization, and wire dispatch while making
//! `Ready` impossible without the later production root backend.

mod auth;
mod dispatch;
mod reply;
mod storage;

pub use auth::{BrokerBinding, ServiceRegistrySnapshot};
pub use dispatch::KmsService;
pub use storage::boot_probe_store;

#[cfg(test)]
mod tests;
