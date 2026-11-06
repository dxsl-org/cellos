#![no_std]
#![forbid(unsafe_code)]

pub mod gates;
mod head_record;
pub mod journal;
mod slot_select;
pub mod superblock;

pub use gates::{
    AuthorityGate, GateStatus, Obligation, ObligationStatus, ResizeOp, MAX_OBLIGATIONS,
};
pub use journal::Journal;
pub use slot_select::JournalExpectation;
pub use superblock::Superblock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    Magic,
    Version,
    /// Superblock slot not signed by the expected writer key.
    Signature,
    Corrupt,
    Uninitialized,
    Capacity,
    DeviceIo,
    /// Record bytes failed DCO1 structural decoding.
    Record(durable_charge_codec::Error),
    /// Ledger/genesis on disk or in a record differ from the expected journal identity.
    Identity,
    /// Newest valid slot is older than the caller's `min_sequence` floor.
    Rollback,
    /// Head record bytes no longer hash to the signed `head_digest`.
    HeadRecord,
    /// Record does not link to the head (`local_revision`/`previous_record_digest`).
    Chain,
    /// Appending key is not the journal's writer key.
    WriterKey,
    Overflow,
    Underflow,
    NotFound,
    Duplicate,
    State,
    Arithmetic,
    GateClosed,
    StaleWriter,
    UnresolvedObligation,
}
