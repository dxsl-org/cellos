//! Service-local file handles for bounded inline reads.
//!
//! Every entry is owned by one attested caller generation and anchored to the
//! exact directory handle used to open it. The ids are opaque but not secret;
//! confidentiality rests on the owner check, not on a caller failing to guess a
//! number.

mod owner_counts;
mod table;

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use table::MAX_FILE_HANDLES_PER_CALLER;
pub use table::{FileHandleError, FileHandleTable};

#[cfg(feature = "test-hooks")]
pub mod selftest;

#[cfg(test)]
mod tests;
