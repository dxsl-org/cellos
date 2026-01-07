// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public API traits for ViOS Cells.
//! 
//! This crate defines the standard interfaces that Cells implement
//! to provide services to other Cells.

#![no_std]

extern crate alloc;
use alloc::boxed::Box;

pub use types::*;

pub mod fs;
pub mod block;
pub mod net;
pub mod hotswap;
pub mod vm;
pub mod serde_helpers;
pub mod async_io;
pub mod allocator;
pub mod benchmark;
pub mod driver;
pub mod syscall;
