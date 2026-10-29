//! Shell-facing text utilities.
//!
//! `awk` and `sed` are pure and re-exported straight from `libs/text-engine`.
//! `grep` needs stdin and the VFS, so only its adapter lives here — the option
//! parser and matcher core stay in the library where host tests can reach them.

pub use text_engine::{awk, sed};

pub mod grep;
