//! Pure, host-testable HTTP/1.1 + JSON protocol logic.
//!
//! # Running the host tests
//!
//! The workspace `.cargo/config.toml` pins a bare-metal default target
//! (`riscv64gc-unknown-none-elf`), so a bare `cargo test -p http-core` tries to
//! build the test harness for `no_std` and fails.  Pass an explicit HOST target:
//!
//! ```text
//! cargo test -p http-core --target x86_64-pc-windows-msvc   # Windows host
//! cargo test -p http-core --target x86_64-unknown-linux-gnu # Linux host
//! ```
//!
//! (substitute your platform's host triple — `rustc -vV | grep host`).
//!
//! # Why this crate exists
//!
//! `ostd` owns `#[global_allocator]` and `#[panic_handler]`, and its workspace
//! `.cargo/config.toml` forces `riscv64gc-unknown-none-elf` → `cargo test -p ostd`
//! never runs on the host.  All pure byte-in/byte-out protocol code lives here
//! instead, so that `cargo test -p http-core` runs on the host with the standard
//! test harness.  `ostd` depends on this crate and adds only transport glue.
//!
//! # no_std contract
//!
//! Outside of `#[cfg(test)]`, this crate is `no_std` + `alloc`.  It defines
//! **no** allocator and **no** panic handler — those are the host runner's
//! (test) or ostd's (bare metal) responsibility.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod body;
pub mod json;
pub mod request;
pub mod response;

pub use body::BodyReader;
pub use response::{Framing, HttpError, ParsedHeaders};

/// Maximum number of response headers parsed in a single call.
///
/// Chosen to cover typical HTTP/1.1 responses with room to spare.  A hostile
/// server sending more than `MAX_HEADERS` headers triggers `HttpError::TooManyHeaders`
/// rather than an unbounded allocation.
pub const MAX_HEADERS: usize = 32;
