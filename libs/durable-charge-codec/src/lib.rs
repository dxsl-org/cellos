#![no_std]
#![forbid(unsafe_code)]
//! Deterministic bytes for the proposed DCO1 r1/s1 schemas, not deployment approval.
//! Parsing and graph consistency do not authenticate commits, floors, custody or facts.

pub mod record;
pub mod references;
pub mod proof;

pub use record::{decode, encode, schema_id, Header, Record};
pub use references::{validate_references, AdmissionRequirement, GraphScratch, Ref, StructuralGraph};

/// Explicit caller-selected finite ceilings. These are not production profile defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_record_bytes: usize,
    pub max_entries: usize,
    pub max_records: usize,
    pub max_reference_depth: usize,
    pub max_total_bytes: usize,
    pub max_work: usize,
}
impl Limits {
    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.max_record_bytes < 248 || self.max_entries == 0 || self.max_records == 0
            || self.max_reference_depth == 0 || self.max_total_bytes < 248 || self.max_work == 0 {
            return Err(Error::Limits);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Limits, Length, Overflow, Magic, Version, Kind, Schema, Reserved, Identity,
    Counter, Digest, Discriminant, NonCanonical, Duplicate, Role, Binding,
    Profile, Watermark, State, Arithmetic, BufferTooSmall, MissingReference,
    Cycle, WorkExhausted, ScratchTooSmall,
    AdmissionRequired(AdmissionRequirement),
}
