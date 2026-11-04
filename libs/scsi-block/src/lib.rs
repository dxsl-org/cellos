//! SCSI block command layer (ADR-0031 phase 05; reused by PVSCSI in phase 09).
//!
//! Transport-independent: a driver cell implements [`ScsiExecutor`] for its
//! request queue (virtio-scsi, PVSCSI) and gets [`ScsiDisk`], which discovers
//! the first direct-access LUN on a target and issues READ(16) / WRITE(16) /
//! SYNCHRONIZE CACHE(10). Everything the device returns (data-in, sense,
//! residuals) is untrusted: malformed input becomes a [`ScsiError`], never a
//! panic.
#![no_std]
#![forbid(unsafe_code)]

pub mod cdb;
mod disk;
mod error;
mod exec;
pub mod parse;
mod probe;
mod sense;

pub use disk::ScsiDisk;
pub use error::{ParseError, ScsiError};
pub use exec::{status, Completion, DataPhase, ScsiExecutor, LUN0};
pub use probe::{ProbeConfig, MAX_REPORTED_LUNS};
pub use sense::{sense_key, Sense, SenseClass};
