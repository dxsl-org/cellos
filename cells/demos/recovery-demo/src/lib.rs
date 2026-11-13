//! Phase 09B software provider crash/restore witness (CP1 option B,
//! `.agents/261030-1945-ginux-lessons-hardening/phase-09b-cp1.md`).
//!
//! Three Cells: `recovery-authority` owns the committed state, the writer
//! epoch and the request-binding ledger and is the lifecycle parent of
//! `recovery-worker`, which serves clients and commits every mutation at the
//! authority before replying; `recovery-client` drives the evidence scenarios.
//! All state is RAM-only and survives a worker crash only.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod ledger;
pub mod wire;

#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod wire_tests;

/// Cell-private fixture service ids (same pattern as init's 0x7A01 fixture);
/// no `abi`/`wire` constant. Init registers the authority from autoload config.
pub const AUTHORITY_SERVICE: u16 = 0x7A0A;
/// Published by the authority only after a replacement restored and validated.
pub const WORKER_SERVICE: u16 = 0x7A0B;
pub const WORKER_PATH: &str = "/bin/recovery-worker";

/// Worker exit code after a refused or invalid restore: never restarted.
pub const EXIT_RECOVERY_FAILED: usize = 0x9B;
/// Worker exit code after its epoch was fenced.
pub const EXIT_FENCED: usize = 0x9C;

/// `Status.state` values reported by the authority.
pub mod state {
    pub const SERVING: u8 = 0;
    pub const RECOVERING: u8 = 1;
    pub const RECOVERY_FAILED: u8 = 2;
    pub const NODE_RECOVERY_REQUIRED: u8 = 3;
}

/// `Crash.mode` scenarios executed by a fault-inject worker.
pub mod crash {
    /// A helper thread spins on another hart while the root thread faults.
    pub const SPIN: u8 = 1;
    /// A helper faults while the root thread waits in a VFS read whose lease
    /// the `recovery-fixture` VFS withholds (missing borrower acknowledgement).
    pub const HOLD_LEASE: u8 = 2;
    /// Allocate and map an untracked SHM frame, then fault.
    pub const SHM_EXPORT: u8 = 3;
    /// Set the lock word in the authority's writable grant, then fault.
    pub const LOCK_AND_DIE: u8 = 4;
}

/// `Corrupt.kind` injected at the authority (fault-inject builds only).
pub mod corrupt {
    pub const CHECKSUM: u8 = 1;
    pub const MISSING: u8 = 2;
    pub const SCHEMA: u8 = 3;
    pub const REVISION: u8 = 4;
}

/// VFS path whose grant lease the `recovery-fixture` VFS holds before replying.
pub const HOLD_LEASE_PATH: &str = "/tmp/recovery-hold-lease";

/// Fixed receive buffer: one frame plus the kernel caller-identity trailer.
pub const RECV_LEN: usize = 128;
