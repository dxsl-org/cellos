#![no_std]
#![forbid(unsafe_code)]
//! Bounded, allocation-free `SOFTWARE_HARNESS` codec and verifier for the
//! `DEV_REFERENCE` VF2 root stream. All capacities and hardware limits are
//! supplied by the caller; every function returns [`Error`] rather than panicking.
//!
//! Encode/decode functions return their exact written/borrowed length or a bounded
//! [`Error`]. `OutputTooSmall`/`ScratchTooSmall` report caller-buffer capacity;
//! malformed wire forms use parsing/profile errors; semantic failures use identity,
//! freshness, range, limit, padding, digest, or signature errors. Decode output and
//! scratch must be treated as untrusted and cleared by the caller after any error.

mod bundle;
#[path = "cbor-read.rs"]
mod cbor_read;
#[path = "cbor-write.rs"]
mod cbor_write;
mod cose;
mod digest;
mod error;
mod model;
mod payload;
mod quarantine;
mod ranges;
mod xmodem;

pub use bundle::{
    encode_outer, outer_encoded_len, verify_bundle, verify_components, VerifiedBundle,
};
pub use cose::{key_id, verify_cose};
#[cfg(feature = "signing")]
pub use cose::{public_key_from_seed, sign_cose};
pub use digest::sha256;
pub use error::{Error, Result};
pub use model::{
    Component, ComponentKind, ComponentLimit, ExpectedManifest, Manifest, ManifestLimits,
    COMPONENT_COUNT, DIGEST_LEN, EVIDENCE_BOUNDARY, EXTERNAL_AAD, LANE, MAX_COSE_LEN,
    MAX_PAYLOAD_LEN, MAX_SIG_STRUCTURE_LEN,
};
pub use payload::{decode_payload, encode_payload};
pub use quarantine::{CleanupHook, LogicalQuarantine};
pub use ranges::{validate_manifest, validate_staging, PhysicalRange, StagingLimits};
pub use xmodem::{
    crc16_xmodem, decode_xmodem, encode_xmodem, xmodem_encoded_len, XMODEM_BLOCK_LEN, XMODEM_EOT,
    XMODEM_FRAME_LEN, XMODEM_PADDING, XMODEM_STX,
};

#[cfg(test)]
extern crate std;
#[cfg(all(test, feature = "signing"))]
#[path = "tests/bundle-xmodem-tests.rs"]
mod bundle_xmodem_tests;
#[cfg(all(test, feature = "signing"))]
#[path = "tests/payload-cose-tests.rs"]
mod payload_cose_tests;
#[cfg(test)]
#[path = "tests/quarantine-tests.rs"]
mod quarantine_tests;
#[cfg(test)]
#[path = "tests/range-tests.rs"]
mod range_tests;
#[cfg(all(test, feature = "signing"))]
#[path = "tests/test-support.rs"]
mod test_support;
