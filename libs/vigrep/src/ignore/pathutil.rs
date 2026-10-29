// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/ignore/src/pathutil.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT
//
// vigrep: paths are bytes with `/` as the only separator, so upstream's
// `cfg(unix)` byte implementations are kept unconditionally and its
// non-Unix variants are dropped. `is_hidden` takes a path instead of a
// walk `DirEntry`. `parent`, `has_root`, `same_path` and `join` replace the
// `std::path::Path` methods the adapted gitignore code and walker use, with
// std's Unix component semantics.

#[allow(unused_imports)]
use crate::prelude::*;

use memchr::{memchr, memrchr};

/// Returns true if and only if this path is considered to be hidden.
///
/// This only returns true if the base name of the path starts with a `.`.
pub(crate) fn is_hidden(path: &[u8]) -> bool {
    if let Some(name) = file_name(path) {
        name.get(0) == Some(&b'.')
    } else {
        false
    }
}

/// Strip `prefix` from the `path` and return the remainder.
///
/// If `path` doesn't have a prefix `prefix`, then return `None`.
pub(crate) fn strip_prefix<'a, P: AsRef<[u8]> + ?Sized>(
    prefix: &'a P,
    path: &'a [u8],
) -> Option<&'a [u8]> {
    let prefix = prefix.as_ref();
    if prefix.len() > path.len() || prefix != &path[0..prefix.len()] {
        None
    } else {
        Some(&path[prefix.len()..])
    }
}

/// Returns true if this file path is just a file name. i.e., Its parent is
/// the empty string.
pub(crate) fn is_file_name<P: AsRef<[u8]>>(path: P) -> bool {
    memchr(b'/', path.as_ref()).is_none()
}

/// The final component of the path, if it is a normal file.
///
/// If the path terminates in ., .., or consists solely of a root of prefix,
/// file_name will return None.
pub(crate) fn file_name<'a, P: AsRef<[u8]> + ?Sized>(
    path: &'a P,
) -> Option<&'a [u8]> {
    let path = path.as_ref();
    if path.is_empty() {
        return None;
    } else if path.len() == 1 && path[0] == b'.' {
        return None;
    } else if path.last() == Some(&b'.') {
        return None;
    } else if path.len() >= 2 && &path[path.len() - 2..] == &b".."[..] {
        return None;
    }
    let last_slash = memrchr(b'/', path).map(|i| i + 1).unwrap_or(0);
    Some(&path[last_slash..])
}

/// `Path::has_root` for byte paths.
pub(crate) fn has_root(path: &[u8]) -> bool {
    path.first() == Some(&b'/')
}

/// Trims trailing separators and trailing `.` components, the way std's
/// `Components::as_path` does, without removing a root `/` or a leading `.`
/// of a relative path.
fn trim_end(path: &[u8]) -> &[u8] {
    let mut end = path.len();
    loop {
        while end > 1 && path[end - 1] == b'/' {
            end -= 1;
        }
        if end >= 2 && path[end - 1] == b'.' && path[end - 2] == b'/' {
            end -= 1;
            continue;
        }
        return &path[..end];
    }
}

/// `Path::parent` for byte paths: the path without its final component, or
/// `None` if the path terminates in a root or is empty.
pub(crate) fn parent(path: &[u8]) -> Option<&[u8]> {
    let path = trim_end(path);
    if path.is_empty() || path == b"/" {
        return None;
    }
    match memrchr(b'/', path) {
        None => Some(&path[..0]),
        Some(0) => Some(&path[..1]),
        Some(i) => Some(trim_end(&path[..i])),
    }
}

/// Iterates over the components of a byte path with std's Unix rules: a
/// leading `/` is a root component, empty components and non-leading `.`
/// components are skipped.
fn components(path: &[u8]) -> impl Iterator<Item = &[u8]> {
    let root = if has_root(path) { Some(&path[..1]) } else { None };
    let mut first = true;
    root.into_iter().chain(path.split(|&b| b == b'/').filter(move |c| {
        let keep = !c.is_empty() && (*c != b"." || (first && !has_root(path)));
        if !c.is_empty() {
            first = false;
        }
        keep
    }))
}

/// `Path` equality for byte paths (component-wise, as std compares paths).
pub(crate) fn same_path(a: &[u8], b: &[u8]) -> bool {
    components(a).eq(components(b))
}

/// `Path::join` for byte paths: an absolute `name` replaces `dir`.
pub(crate) fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if has_root(name) {
        return name.to_vec();
    }
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    if !dir.is_empty() && dir.last() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The byte helpers agree with `std::path::Path` on Unix.
    #[test]
    fn agrees_with_std_path() {
        let cases: &[&str] = &[
            "", "/", "a", "a/", "a/b", "a/b/", "a//b", "/a", "/a/b", ".",
            "./a", "a/./b", "a/.", "..", "../a", "a/..", "//a", "././", "./.",
            ".a/b",
        ];
        for &c in cases {
            let std_parent =
                Path::new(c).parent().map(|p| p.to_str().unwrap().as_bytes());
            assert_eq!(parent(c.as_bytes()), std_parent, "parent({c:?})");
            assert_eq!(has_root(c.as_bytes()), Path::new(c).has_root());
            for &d in cases {
                assert_eq!(
                    same_path(c.as_bytes(), d.as_bytes()),
                    Path::new(c) == Path::new(d),
                    "same_path({c:?}, {d:?})"
                );
            }
            for &n in &["x", "/x", "x/y"] {
                let std_join = Path::new(c).join(n);
                assert_eq!(
                    join(c.as_bytes(), n.as_bytes()),
                    std_join.to_str().unwrap().as_bytes(),
                    "join({c:?}, {n:?})"
                );
            }
        }
    }

    #[test]
    fn hidden_by_base_name() {
        assert!(is_hidden(b"/a/.git"));
        assert!(is_hidden(b".x"));
        assert!(!is_hidden(b"/.a/b"));
        assert!(!is_hidden(b"."));
        assert!(!is_hidden(b".."));
    }
}
