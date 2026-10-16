//! Live destination atomic claim and Closing serialization.

use super::ledger::ApplicationLedger;
use super::reservations::{ReservationState, ReservationTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    NotFound,
    AlreadyClaimed,
    ClosingOrReleased,
    EpochMismatch,
}

/// Atomically claim a reservation for an admitted destination node.
pub fn claim_reservation(
    table: &mut ReservationTable,
    key: &[u8; 16],
    destination: [u8; 32],
    epoch: u64,
) -> Result<(), ClaimError> {
    let entry = table.get_mut(key).ok_or(ClaimError::NotFound)?;
    if entry.epoch != epoch {
        return Err(ClaimError::EpochMismatch);
    }
    match entry.state {
        ReservationState::Reserved => {
            entry.state = ReservationState::Claimed { destination };
            Ok(())
        }
        ReservationState::Claimed { .. } => Err(ClaimError::AlreadyClaimed),
        ReservationState::Closing | ReservationState::Released => {
            Err(ClaimError::ClosingOrReleased)
        }
    }
}

/// Atomically transition a reservation to Closing.
pub fn mark_closing(
    table: &mut ReservationTable,
    key: &[u8; 16],
) -> Result<(), ClaimError> {
    let entry = table.get_mut(key).ok_or(ClaimError::NotFound)?;
    match entry.state {
        ReservationState::Reserved | ReservationState::Claimed { .. } => {
            entry.state = ReservationState::Closing;
            Ok(())
        }
        ReservationState::Closing | ReservationState::Released => {
            Err(ClaimError::ClosingOrReleased)
        }
    }
}

/// Confirm release and credit the ledger exactly once.
pub fn release_closed(
    table: &mut ReservationTable,
    ledger: &mut ApplicationLedger,
    key: &[u8; 16],
) -> Result<(), ClaimError> {
    let entry = table.get_mut(key).ok_or(ClaimError::NotFound)?;
    if entry.state != ReservationState::Closing {
        return Err(ClaimError::ClosingOrReleased);
    }
    let vector = entry.vector;
    entry.state = ReservationState::Released;
    let _ = ledger.release(&vector);
    Ok(())
}
