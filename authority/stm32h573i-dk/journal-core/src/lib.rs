#![no_std]
#![forbid(unsafe_code)]
//! Host-verifiable model of the STM32 protected full-record journal.

mod auth;
mod codec;
mod journal;
mod model;
mod recovery;
mod successor;
pub use auth::RecordAuthenticator;
pub use codec::{decode_record, encode_record, CodecError, RECORD_MAX};
pub use journal::{BackendError, Counter, Journal, JournalError, SlotStorage};
pub use model::{FullRecord, HardwareBindings, ProfileMaterial, RecordError, SlotRole, SPKI_MAX};
pub use recovery::{recover, ExpectedIdentity, RecoveredRecord, RecoveryError};

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
