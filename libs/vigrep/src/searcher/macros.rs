// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/searcher/src/macros.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

#[allow(unused_imports)]
use crate::prelude::*;

/// Like assert_eq, but nicer output for long strings.
#[cfg(test)]
#[macro_export]
macro_rules! assert_eq_printed {
    ($expected:expr, $got:expr, $($tt:tt)*) => {
        let expected = &*$expected;
        let got = &*$got;
        let label = format!($($tt)*);
        if expected != got {
            panic!("
printed outputs differ! (label: {})

expected:
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
{}
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

got:
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
{}
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
", label, expected, got);
        }
    }
}
