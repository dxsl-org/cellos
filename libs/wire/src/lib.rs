// SPDX-License-Identifier: Apache-2.0
//! Extensible service interface contracts.
//!
//! Add new service traits, IPC message types, and driver abstractions here.
//! Adding a new sub-module does **not** require a kernel recompile and does
//! **not** require 2× confirmation — only changes to the `abi` crate do.
//!
//! The contracts the kernel itself speaks (`fs`, `block`, `IPC_BUF_SIZE`,
//! `REACTOR_WAKE_OPCODE`, the App SDK envelope, the power-button request and
//! input producer frames) live in `abi` and are re-exported here at their
//! service-contract paths.

// Disable `no_std` when running the test harness so `#[test]` can link
// against the host libstd.  All production builds remain bare-metal.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod allocator;
pub mod async_io;
pub mod benchmark;
pub use abi::block;
pub mod cluster;
pub mod config;
pub mod dir_name;
pub mod dir_name_tests;
pub mod display;
pub mod driver;
pub use abi::fs;
#[cfg(feature = "hostile-backend-recovery")]
pub mod hostile_backend_recovery;
pub mod hotswap;
pub mod input;
pub mod ipc;
pub mod machine_pin;
pub mod mime;
pub mod net;
#[path = "net-reply.rs"]
pub mod net_reply;
pub mod proto;
pub mod ring_channel;
pub mod serde_helpers;
pub mod session;
pub mod ssh_completion;
pub mod vfs_file_handles;
pub mod vmm_session;
