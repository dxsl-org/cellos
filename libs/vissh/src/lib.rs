//! Cellos SSH client core.
//!
//! This crate wraps the vendored Sunset (`third_party/sunset`, see its `PROVENANCE` and
//! `PATCHES`). Two Cellos patches apply there:
//! - KEX is `curve25519-sha256` only, the cipher is `chacha20-poly1305@openssh.com` only,
//!   and strict KEX is required (command-contracts §4.2);
//! - entropy comes only from the source installed with [`install_entropy`]. With no
//!   source installed, or after a failed draw, KEX does not start.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
pub mod client;
pub mod session;
pub mod userauth;

pub use sunset;

/// Install the process entropy source: it must fill the whole buffer with
/// cryptographic random bytes, or return `false`. The first installation wins.
pub fn install_entropy(source: sunset::EntropySource) -> bool {
    sunset::set_entropy_source(source)
}
