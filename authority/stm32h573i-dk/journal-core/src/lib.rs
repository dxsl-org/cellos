#![no_std]
#![forbid(unsafe_code)]
//! Host-verifiable model of the STM32 protected full-record journal.

mod auth;
mod codec;
mod journal;
mod model;
mod profile_bank;
mod recovery;
mod snapshot;
mod successor;
pub use auth::RecordAuthenticator;
pub use codec::{decode_record, encode_record, CodecError, RECORD_MAX};
pub use journal::{BackendError, Counter, Journal, JournalError, SlotStorage};
pub use model::{FullRecord, HardwareBindings, ProfileMaterial, RecordError, SlotRole, SPKI_MAX};
pub use profile_bank::{
    begin_profile_upload, write_profile_chunk, BankError, BankStorageError, ProfileBank,
    ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankReference, ProfileBankStorage,
    UploadFlowError, UploadHead, PROFILE_BANK_CHUNK_REGION_MAX, PROFILE_BANK_HEADER_MAX,
    PROFILE_CHUNK_REGIONS, PROFILE_CHUNK_SIZE,
};
pub use recovery::{
    recover, ExpectedIdentity, RecoveredRecord, RecoveryError, UnvalidatedRecoveredRecord,
};
pub use snapshot::PendingEnrollmentSnapshot;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
