//! # ViOS Kernel Prelude
//!
//! This module contains **ONLY** the most fundamental types that are used
//! in virtually every kernel module. Following the pattern of `std::prelude::v1`,
//! we re-export `Option` and `Result` to reduce boilerplate while maintaining
//! the explicitness required for kernel development.
//!
//! ## Policy
//!
//! - **ONLY** core language fundamentals are allowed here
//! - Any additions must be approved by team review (see docs/KERNEL_PRELUDE_POLICY.md)
//! - Collections, utilities, and domain types **MUST** be imported explicitly
//!
//! ## Usage
//!
//! Add this to the top of every kernel module:
//!
//! ```rust
//! use crate::prelude::*;
//! ```
//!
//! ## Current Contents
//!
//! - `Option`, `Some`, `None` - Optional values
//! - `Result`, `Ok`, `Err` - Error handling
//!
//! ## See Also
//!
//! - [Kernel Prelude Policy](../docs/KERNEL_PRELUDE_POLICY.md)
//! - [Rust std::prelude](https://doc.rust-lang.org/std/prelude/)

// Re-export core fundamentals (these are in std::prelude::v1 too)
pub use core::option::Option::{self, Some, None};
pub use core::result::Result::{self, Ok, Err};

// End of prelude.
// DO NOT add more without team discussion and policy update!
// See docs/KERNEL_PRELUDE_POLICY.md for rationale.
