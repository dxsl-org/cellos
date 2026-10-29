//! `vigrep`: ripgrep's search components for Cellos, as `no_std` + `alloc`.
//!
//! This crate is an adaptation of five crates from ripgrep 15.2.0
//! (commit `e89fff89ac9af12e8d4ce9d5fd07beb408ca730f`, MIT chosen from
//! `Unlicense OR MIT`, copyright (c) 2015 Andrew Gallant) plus a serial
//! walker written for Cellos. The `std` boundary of the upstream crates is
//! replaced by [`io`] (a core-only reader) and by byte paths; nothing here
//! touches a filesystem, the environment, threads or memory maps.
//!
//! # Upstream → module mapping
//!
//! | Upstream crate (version) | Upstream files | Module |
//! |---|---|---|
//! | grep-matcher 0.1.9 | `src/{lib,interpolate}.rs` | [`matcher`] |
//! | grep-regex 0.1.14 | `src/{lib,ast,ban,config,error,literal,matcher,non_matching,strip}.rs` | [`regex`] |
//! | grep-searcher 0.1.17 | `src/{lib,macros,line_buffer,lines,sink}.rs`, `src/searcher/{mod,core,glue}.rs` | [`searcher`] |
//! | globset 0.4.19 | `src/{lib,glob,pathutil,fnv}.rs` | [`glob`] |
//! | ignore 0.4.29 | `src/{gitignore,overrides,pathutil}.rs`, error types of `src/lib.rs`, precedence logic of `src/dir.rs` | [`ignore`] |
//! | — (Cellos) | — | [`io`], [`walk`] |
//!
//! Dropped upstream code: `searcher/src/searcher/mmap.rs`, encoding/BOM
//! transcoding and the path/file entry points of the searcher; globset's
//! serde support; ignore's `walk.rs`, `types.rs`, `default_types.rs`, global
//! and git-exclude discovery, repository detection and environment lookups.
//! The per-file upstream SHA-256 values are listed in `PROVENANCE`.
//!
//! # Host tests
//!
//! The workspace pins a bare-metal default target, so run
//! `cargo test -p vigrep --target x86_64-unknown-linux-gnu`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
// The adapted upstream files keep ripgrep's own style so they stay
// line-for-line comparable with the 15.2.0 sources (upstream does not run
// clippy). These are the lints that style trips; vigrep's own modules
// (`io`, `walk`) are clean without them.
#![allow(
    clippy::bool_comparison,
    clippy::byte_char_slices,
    clippy::collapsible_if,
    clippy::derivable_impls,
    clippy::explicit_auto_deref,
    clippy::get_first,
    clippy::identity_op,
    clippy::if_same_then_else,
    clippy::let_and_return,
    clippy::match_like_matches_macro,
    clippy::module_inception,
    clippy::needless_borrow,
    clippy::needless_borrowed_reference,
    clippy::needless_lifetimes,
    clippy::needless_return,
    clippy::new_without_default,
    clippy::op_ref,
    clippy::option_as_ref_deref,
    clippy::question_mark,
    clippy::redundant_closure,
    clippy::single_char_add_str,
    clippy::unit_arg,
    clippy::unnecessary_map_or,
    clippy::upper_case_acronyms,
    clippy::wrong_self_convention
)]

extern crate alloc;

mod prelude;

pub mod glob;
pub mod ignore;
pub mod io;
pub mod matcher;
pub mod regex;
pub mod searcher;
pub mod walk;
