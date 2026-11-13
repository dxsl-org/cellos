// SPDX-License-Identifier: Apache-2.0
//! One wire module per service: opcodes, frame lengths and status/error bytes.
//!
//! Servers, SDK clients, tools and the kernel import these items instead of
//! re-declaring bytes. Every value is pinned to the byte it had before the
//! modules existed (`proto_tests.rs`); a value here never changes, it is only
//! appended to. Wires that already had a single owner (block, kms, silo,
//! vmm_session, compositor, the postcard enums in `ipc.rs`) keep it and are
//! re-exported where a module groups them.

/// The kernel emits App SDK envelopes itself, so the module lives in `abi`.
pub use abi::app_envelope;
pub mod compositor;
pub mod config;
pub mod input;
pub mod net;
pub mod nic;
pub mod supervisor;
pub mod tls;
pub mod vfs;

#[cfg(test)]
#[path = "proto/proto_tests.rs"]
mod proto_tests;
