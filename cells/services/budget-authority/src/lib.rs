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
pub use recovery::{recover_ledger, RecoveryStatus};
pub use reservations::{ReservationEntry, ReservationState, ReservationTable};
