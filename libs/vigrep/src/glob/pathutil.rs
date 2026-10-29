// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/globset/src/pathutil.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

#[allow(unused_imports)]
use crate::prelude::*;

use alloc::borrow::Cow;

use bstr::ByteSlice;

/// The final component of the path, if it is a normal file.
///
/// If the path terminates in `..`, or consists solely of a root of prefix,
/// file_name will return `None`.
pub(crate) fn file_name<'a>(path: &Cow<'a, [u8]>) -> Option<Cow<'a, [u8]>> {
    if path.is_empty() {
        return None;
    }
    let last_slash = path.rfind_byte(b'/').map(|i| i + 1).unwrap_or(0);
    let got = match *path {
        Cow::Borrowed(path) => Cow::Borrowed(&path[last_slash..]),
        Cow::Owned(ref path) => Cow::Owned(path[last_slash..].to_vec()),
    };
    if got == &b".."[..] {
        return None;
    }
    Some(got)
}

/// Return a file extension given a path's file name.
///
/// Note that this does NOT match the semantics of std::path::Path::extension.
/// Namely, the extension includes the `.` and matching is otherwise more
/// liberal. Specifically, the extension is:
///
/// * None, if the file name given is empty;
/// * None, if there is no embedded `.`;
/// * Otherwise, the portion of the file name starting with the final `.`.
///
/// e.g., A file name of `.rs` has an extension `.rs`.
///
/// N.B. This is done to make certain glob match optimizations easier. Namely,
/// a pattern like `*.rs` is obviously trying to match files with a `rs`
/// extension, but it also matches files like `.rs`, which doesn't have an
/// extension according to std::path::Path::extension.
pub(crate) fn file_name_ext<'a>(
    name: &Cow<'a, [u8]>,
) -> Option<Cow<'a, [u8]>> {
    if name.is_empty() {
        return None;
    }
    let last_dot_at = match name.rfind_byte(b'.') {
        None => return None,
        Some(i) => i,
    };
    Some(match *name {
        Cow::Borrowed(name) => Cow::Borrowed(&name[last_dot_at..]),
        Cow::Owned(ref name) => Cow::Owned(name[last_dot_at..].to_vec()),
    })
}

/// Normalizes a path to use `/` as a separator everywhere, even on platforms
/// that recognize other characters as separators.
///
/// vigrep: Cellos only uses `/` (upstream's `cfg(unix)` variant).
pub(crate) fn normalize_path(path: Cow<'_, [u8]>) -> Cow<'_, [u8]> {
    // UNIX only uses /, so we're good.
    path
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;

    use bstr::{B, ByteVec};

    use super::{file_name, file_name_ext, normalize_path};

    macro_rules! name {
        ($name:ident, $path:expr, $expected:expr) => {
            #[test]
            fn $name() {
                let bs = Vec::from($path);
                let got = file_name(&Cow::Owned(bs));
                assert_eq!($expected.map(|s| Cow::Borrowed(B(s))), got);
            }
        };
    }

    name!(name1, "foo/bar", Some("bar"));
    name!(name2, "foo", Some("foo"));
    name!(name3, "foo/..", None::<&str>);
    name!(name4, "", None::<&str>);

    macro_rules! ext {
        ($name:ident, $file_name:expr, $ext:expr) => {
            #[test]
            fn $name() {
                let bs = Vec::from($file_name);
                let got = file_name_ext(&Cow::Owned(bs));
                assert_eq!($ext.map(|s| Cow::Borrowed(B(s))), got);
            }
        };
    }

    ext!(ext1, "foo.rs", Some(".rs"));
    ext!(ext2, ".rs", Some(".rs"));
    ext!(ext3, "..rs", Some(".rs"));
    ext!(ext4, "", None::<&str>);
    ext!(ext5, "foo", None::<&str>);

    macro_rules! normalize {
        ($name:ident, $path:expr, $expected:expr) => {
            #[test]
            fn $name() {
                let bs = Vec::from_slice($path);
                let got = normalize_path(Cow::Owned(bs));
                assert_eq!($expected.to_vec(), got.into_owned());
            }
        };
    }

    normalize!(normal1, b"foo", b"foo");
    normalize!(normal2, b"foo/bar", b"foo/bar");
    normalize!(normal3, b"foo\\bar", b"foo\\bar");
    normalize!(normal4, b"foo\\bar/baz", b"foo\\bar/baz");
}
