// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/regex/src/lib.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

/*!
An implementation of `grep-matcher`'s `Matcher` trait for Rust's regex engine.
*/
#![deny(missing_docs)]

#[allow(unused_imports)]
use crate::prelude::*;

pub use crate::regex::{
    error::{Error, ErrorKind},
    matcher::{RegexCaptures, RegexMatcher, RegexMatcherBuilder},
};

mod ast;
mod ban;
mod config;
mod error;
mod literal;
mod matcher;
mod non_matching;
mod strip;
