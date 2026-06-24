// SPDX-License-Identifier: Apache-2.0
//! Extensible service interface contracts.
//!
//! Add new service traits, IPC message types, and driver abstractions here.
//! Adding a new sub-module does **not** require a kernel recompile and does
//! **not** require 2× confirmation — only changes to [`crate::abi`] do.

pub mod allocator;
pub mod async_io;
pub mod benchmark;
pub mod block;
pub mod cluster;
pub mod config;
pub mod display;
pub mod driver;
pub mod fs;
pub mod hotswap;
pub mod input;
pub mod ipc;
pub mod net;
pub mod posix;
pub mod serde_helpers;
