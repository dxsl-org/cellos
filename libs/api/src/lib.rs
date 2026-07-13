// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0>.

//! Public API for Cellos.

// Disable `no_std` when running the test harness so `#[test]` can link
// against the host libstd.  All production builds remain bare-metal.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

pub use types::*;

/// Frozen kernel ABI — changes require 2× explicit confirmation.
pub mod abi;
/// Extensible service interface contracts — new services go here.
pub mod services;

// Flat re-exports: all existing `api::X` paths continue to work unchanged
// in kernel and cell code. The abi/services split is organisational only.
pub use abi::*;
pub use services::*;

pub use abi::syscall::ViSyscall;
pub use abi::task::TaskPriority;
