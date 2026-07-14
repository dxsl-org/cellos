// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0>.

//! Public API for Cellos.

// Disable `no_std` when running the test harness so `#[test]` can link
// against the host libstd.  All production builds remain bare-metal.
#![cfg_attr(not(test), no_std)]
// Required for VaList::next_arg (posix stdio_fmt) on toolchains where c_variadic
// is still unstable. The pinned nightly-2026-05-01 already stabilized it and
// flags this declaration as unused; allow(unused_features) keeps both old and
// new toolchains compiling clean instead of picking one at the other's expense.
#![allow(unused_features)]
#![feature(c_variadic)]

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
