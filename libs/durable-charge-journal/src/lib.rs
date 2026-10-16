#![no_std]
#![forbid(unsafe_code)]

pub mod superblock;
pub mod journal;
pub mod gates;

pub use superblock::Superblock;
pub use journal::Journal;
pub use gates::{AuthorityGate, Obligation, ObligationStatus, GateStatus, ResizeOp, MAX_OBLIGATIONS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    Magic,
    Version,
    Checksum,
    Corrupt,
    Uninitialized,
    Capacity,
    DeviceIo,
    RecordLength,
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
