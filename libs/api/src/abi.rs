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

pub mod caller_identity;
pub mod cap;
pub mod cell_owner;
pub mod completion;
pub mod dir_attestation;
pub mod dir_handles;
pub mod dir_handles_tests;
pub mod disk;
pub mod fast_ipc;
pub mod hypervisor;
pub mod manifest;
#[cfg(test)]
mod manifest_compat_tests;
pub mod manifest_flags;
pub mod manifest_macro;
pub mod manifest_parse;
#[cfg(test)]
mod manifest_tests;
pub mod syscall;
pub mod syscall_tests;
pub mod task;
pub mod vm;
