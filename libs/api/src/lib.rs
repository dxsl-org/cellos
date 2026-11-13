// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0>.

//! Public API for Cellos — a one-cycle facade (Phase 13 A1).
//!
//! The crate was split into `abi` (frozen kernel ABI), `wire` (service
//! contracts) and `posix-shim` (Tier-A C library), and the KMS codecs left
//! `types` for `kms-codec`. This facade re-exports them so every existing
//! `api::` path (including `api::kms`, Spec 23) keeps compiling while imports
//! migrate to the split crates.

#![cfg_attr(not(test), no_std)]

pub use ::abi::*;
pub use types::*;
pub use wire::*;

/// Frozen kernel ABI (`api::abi::…`).
pub mod abi {
    pub use ::abi::*;
}

/// Service interface contracts (`api::services::…`).
pub mod services {
    pub use posix_shim as posix;
    pub use wire::*;
}

pub use posix_shim as posix;

/// The KMS service wire (`api::kms`, formerly `types::kms`).
pub use kms_codec as kms;
