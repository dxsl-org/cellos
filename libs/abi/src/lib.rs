// SPDX-License-Identifier: Apache-2.0
//! Frozen kernel ABI — stable contract between kernel and Cells.
//!
//! ⚠️ **FROZEN** — every item here is part of the stable ABI between the Cellos
//! kernel and Cells.  Changes force a full kernel recompile and require
//! **2× explicit user confirmation**.  See `docs/specs/15-kernel-boundary.md`.
//!
//! Rules:
//! - Never remove or rename existing items.
//! - Never change `repr`, discriminant values, or field layouts.
//! - Only add at new explicit discriminant/offset values.
//! - Even additions require 2× confirmation (they change the build contract).
//!
//! Besides the syscall ABI, this crate owns the service contracts the kernel
//! speaks itself (`fs`, `block`, `kernel_ipc`, `app_envelope`,
//! `supervisor_power`, `input_producer`); `wire` re-exports them at their
//! service-contract paths.

// Disable `no_std` when running the test harness so `#[test]` can link
// against the host libstd.  All production builds remain bare-metal.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod anon_memory;
pub mod app_envelope;
pub mod block;
pub mod block_route;
pub mod caller_identity;
pub mod cap;
#[path = "cell-image.rs"]
pub mod cell_image;
pub mod cell_owner;
pub mod cell_session;
pub mod completion;
pub mod dir_attestation;
pub mod dir_handles;
pub mod dir_handles_tests;
pub mod disk;
pub mod dma_export;
#[cfg(test)]
mod dma_export_tests;
pub mod fast_ipc;
pub mod fs;
pub mod hypervisor;
pub mod input_producer;
pub mod kernel_ipc;
pub mod machine_facts;
pub mod manifest;
#[cfg(test)]
mod manifest_compat_tests;
pub mod manifest_flags;
pub mod manifest_macro;
pub mod manifest_parse;
#[cfg(test)]
mod manifest_tests;
pub mod pcie_function;
pub mod retirement;
pub mod service_binding;
pub mod supervisor_power;
pub mod syscall;
pub mod syscall_tests;
pub mod task;
pub mod telemetry;
pub mod vm;

pub use syscall::ViSyscall;
pub use task::TaskPriority;
