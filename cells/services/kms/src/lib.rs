#![no_std]
#![forbid(unsafe_code)]

//! Fail-closed runtime boundary for the Cellos node-identity KMS.
//!
//! This slice deliberately has no root provider. It proves service lifecycle,
//! attested authorization, and wire dispatch while making `Ready` impossible.

mod auth;
mod dispatch;
mod reply;

pub use auth::{BrokerBinding, ServiceRegistrySnapshot};
pub use dispatch::KmsService;

#[cfg(test)]
mod tests;
