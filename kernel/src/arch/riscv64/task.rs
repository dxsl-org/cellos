// SPDX-License-Identifier: MPL-2.0
//! riscv64-only task self-tests (moved from `crate::task`).

#[cfg(feature = "test-hooks")]
pub mod context_handoff_selftest;
#[cfg(feature = "test-hooks")]
pub mod fstat_selftest;
#[cfg(feature = "getrandom-sas-test")]
#[path = "task/getrandom-sas-tests.rs"]
pub(crate) mod getrandom_sas_tests;
#[cfg(feature = "test-hooks")]
pub mod path_selftest;
#[cfg(feature = "test-hooks")]
pub mod retirement_selftest;
#[cfg(feature = "test-hooks")]
pub mod stack_overflow_probe;
