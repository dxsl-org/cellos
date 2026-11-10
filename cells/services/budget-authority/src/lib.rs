//! Spec 26 design skeleton — NOT wired and fail-open (owner decision D1, 2026-11-10).
//! Excluded from the workspace and every image; no enforcement may rely on it. Kept as
//! source for controlling-plan P08 (`.agents/261011-1200-cell-contract-authority`).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod bootstrap;
pub mod claim;
pub mod fence;
pub mod ledger;
pub mod recovery;
pub mod reservations;

pub use bootstrap::BootstrapGrant;
pub use claim::{claim_reservation, mark_closing, release_closed, ClaimError};
pub use fence::EnforcementFence;
pub use ledger::{ApplicationLedger, LedgerError, ResourceVector, VECTOR_DIMENSIONS};
pub use recovery::{recover_ledger, RecoveryStatus, StoredLedgerState};
pub use reservations::{ReservationEntry, ReservationState, ReservationTable};
