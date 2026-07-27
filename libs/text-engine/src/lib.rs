//! Pure, host-testable text-utility logic for the Cellos shell.
//!
//! # Running the host tests
//!
//! The workspace `.cargo/config.toml` pins a bare-metal default target
//! (`riscv64gc-unknown-none-elf`), so a bare `cargo test -p text-engine` tries to
//! build the test harness for `no_std` and fails.  Pass an explicit HOST target:
//!
//! ```text
//! cargo test -p text-engine --target x86_64-pc-windows-msvc   # Windows host
//! cargo test -p text-engine --target x86_64-unknown-linux-gnu # Linux host
//! ```
//!
//! # Why this crate exists
//!
//! `app-shell` links `ostd`, which owns `#[global_allocator]` and
//! `#[panic_handler]`; linking it alongside the std test harness fails with
//! duplicate lang items (E0152).  Every byte-in/byte-out utility stage —
//! argv cursors, ERE-lite compilation, record reading, and the grep/sed/awk/top
//! cores — therefore lives here, and the shell keeps only the thin adapters that
//! touch stdin, the VFS, and the terminal.
//!
//! Outside of `#[cfg(test)]` this crate is `no_std` + `alloc`.
//!
//! # Delivered dialect
//!
//! ERE-lite, not POSIX: linear-time and ASCII-first, with no backreferences,
//! look-around, or locale support.  `awk` is a documented mini-language and
//! `sed` accepts a single command.  Every stage is bounded — see
//! [`matcher::MAX_PATTERN_BYTES`], [`records::MAX_INPUT_BYTES`],
//! [`records::MAX_RECORD_BYTES`], and [`awk::MAX_PROGRAM_BYTES`].

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
pub mod matcher;
pub mod records;

pub mod awk;
pub mod grep;
pub mod sed;
pub mod top;
