//! ViCell EL2 hypervisor kernel support (Phase 03).
//!
//! Provides the Stage-2 table builder, vCPU world-switch, and smoke-test
//! harnesses used by the ARM64 VMM before the full hypervisor cell is available
//! (Phase 05).  The public API here is intentionally kernel-internal; the
//! stable VMM ABI (syscalls 220+) is exposed in Phase 04.

pub mod registry;

#[cfg(all(target_arch = "aarch64", feature = "test-hooks"))]
pub mod smoke_guest;
