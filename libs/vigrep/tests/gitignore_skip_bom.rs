// Adapted for vigrep (host test) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/ignore/tests/gitignore_skip_bom.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT
//
// vigrep: `GitignoreBuilder::add` read the fixture from disk; the fixture
// bytes are now passed to `add_bytes`, labelled with the same path.

use vigrep::ignore::gitignore::GitignoreBuilder;

const IGNORE_BYTES: &[u8] = include_bytes!("gitignore_skip_bom.gitignore");

const IGNORE_FILE: &'static str = "tests/gitignore_skip_bom.gitignore";

/// Skip a Byte-Order Mark (BOM) at the beginning of the file, matching Git's
/// behavior.
///
/// Ref: <https://github.com/BurntSushi/ripgrep/issues/2177>
#[test]
fn gitignore_skip_bom() {
    let mut builder = GitignoreBuilder::new("ROOT");
    let error = builder.add_bytes(IGNORE_FILE, IGNORE_BYTES);
    assert!(error.is_none(), "failed to open gitignore file");
    let g = builder.build().unwrap();

    assert!(g.matched("ignore/this/path", false).is_ignore());
}
