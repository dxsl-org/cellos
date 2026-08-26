#![no_std]
#![forbid(unsafe_code)]

//! Fail-closed runtime boundary for the Cellos node-identity KMS.
//!
//! This slice deliberately keeps the root backend fail-closed. It proves
//! service lifecycle, attested authorization, and wire dispatch while making
//! `Ready` impossible without the later production root backend.
#[cfg(any(
    all(
        feature = "hardware-relay-provider",
        feature = "development-silo-provider"
    ),
    all(feature = "hardware-relay-provider", feature = "fixture-provider"),
    all(feature = "development-silo-provider", feature = "fixture-provider"),
))]
compile_error!("service-kms: select at most one relay provider");

#[cfg(all(
    feature = "hardware-relay-provider",
    any(
        feature = "test-hooks",
        feature = "raw-relay-provider",
        feature = "k1-fallback"
    )
))]
compile_error!("service-kms: hardware relay excludes development and downgrade features");

#[cfg(feature = "hardware-relay-provider")]
compile_error!(
    "service-kms: hardware-relay-provider remains blocked until Phases 6-7 \
     select and implement the exact production root"
);

#[cfg(all(
    feature = "development-silo-provider",
    not(all(target_arch = "aarch64", target_os = "none"))
))]
compile_error!(
    "service-kms: development-silo-provider is restricted to the AArch64 bare-metal QEMU lane"
);

#[cfg(test)]
extern crate std;

mod auth;
mod dispatch;
mod lifecycle;
mod reply;
mod storage;

pub use auth::{BrokerBinding, ServiceNetBinding, ServiceRegistrySnapshot};
pub use dispatch::KmsService;
pub use storage::boot_probe_store;

#[cfg(test)]
mod tests;
