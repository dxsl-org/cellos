// SPDX-License-Identifier: Apache-2.0
//! Deciding whether a byte string may name something inside a directory handle.
//!
//! ## Why this works on raw bytes
//! Every rule below is applied to the bytes exactly as they arrived, before any
//! normalisation, case folding, or separator rewriting. Normalisation is where
//! traversal holes come from: a check that runs against a cleaned-up copy is a
//! check against a string the backend will never see, and the two disagree
//! precisely on the inputs an attacker is choosing. Decoding to UTF-8 happens
//! *last* and only to hand the caller a `&str`; it never changes a byte, so the
//! decision has already been made by then.
//!
//! ## What a component may not be
//! A component names one entry in one directory. It therefore may not contain a
//! separator of any kind, may not be `.` or `..`, may not be empty, and may not
//! carry control bytes. With separators excluded, `..` as a whole component is
//! the only remaining way to name a parent, so rejecting it closes the set.
//!
//! Backslash is refused alongside `/` even though no backend here treats it as a
//! separator: the cost is one comparison, and the alternative is that adding a
//! backend which does treat it as one silently opens an escape.

/// Longest component a directory handle will resolve.
///
/// Matches the widest limit among the mounted backends, so a name this layer
/// accepts can never be truncated into a different name further down.
pub const MAX_DIR_NAME_LEN: usize = 255;

/// Longest absolute path accepted when a root handle is acquired.
pub const MAX_DIR_PATH_LEN: usize = 1024;

/// Why a name was refused. Every variant is a refusal; none is recoverable by
/// rewriting the name on the service's behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirNameError {
    /// No bytes at all. Names the directory itself, which no `*At` operation
    /// means, and which several backends would resolve as the directory.
    Empty,
    /// Longer than the limit for its kind.
    TooLong,
    /// Contains `/` or `\`, so it spans more than one directory.
    Separator,
    /// Is `..`, the one remaining way to name a parent once separators are out.
    Traversal,
    /// Is `.`, which names the directory the handle already refers to.
    CurrentDir,
    /// Carries a byte below `0x20` or `0x7F`, including NUL. A backend reached
    /// through a C interface would read a NUL as the end of the name, so the
    /// name it stores and the name checked here would differ.
    Control,
    /// Not valid UTF-8. Rejected rather than replaced: an overlong encoding of
    /// `/` is invalid UTF-8 and must not become a separator, and lossy decoding
    /// would turn it into a name nobody asked for.
    NotUtf8,
    /// An absolute path that does not begin at the root.
    NotAbsolute,
}

/// Check one component of a path, as raw bytes, and return it as `&str`.
///
/// The returned string is the same bytes, unchanged — this function never
/// rewrites, trims, or folds anything.
///
/// # Errors
/// See [`DirNameError`]. A component that passes cannot name anything outside
/// the directory it is resolved against.
pub fn validate_dir_component(raw: &[u8]) -> Result<&str, DirNameError> {
    if raw.is_empty() {
        return Err(DirNameError::Empty);
    }
    if raw.len() > MAX_DIR_NAME_LEN {
        return Err(DirNameError::TooLong);
    }
    if raw == b".." {
        return Err(DirNameError::Traversal);
    }
    if raw == b"." {
        return Err(DirNameError::CurrentDir);
    }
    for &b in raw {
        if b == b'/' || b == b'\\' {
            return Err(DirNameError::Separator);
        }
        if b < 0x20 || b == 0x7F {
            return Err(DirNameError::Control);
        }
    }
    core::str::from_utf8(raw).map_err(|_| DirNameError::NotUtf8)
}

/// Check an absolute path offered as the root of a new handle, as raw bytes.
///
/// Root acquisition is the one place a path string still appears, so the same
/// byte-level rules apply to every component of it. A path that passes contains
/// no traversal at all, which means the handle it produces refers to exactly the
/// directory the caller named and the resolution rules above hold from then on.
///
/// # Errors
/// [`DirNameError::NotAbsolute`] when the path does not start at the root;
/// otherwise whatever the offending component fails on, with an empty component
/// (from `//` or a trailing `/`) reported as [`DirNameError::Empty`]. `/` itself
/// is accepted and has no components.
pub fn validate_dir_path(raw: &[u8]) -> Result<&str, DirNameError> {
    if raw.is_empty() {
        return Err(DirNameError::Empty);
    }
    if raw.len() > MAX_DIR_PATH_LEN {
        return Err(DirNameError::TooLong);
    }
    if raw[0] != b'/' {
        return Err(DirNameError::NotAbsolute);
    }
    if raw != b"/" {
        for component in raw[1..].split(|&b| b == b'/') {
            validate_dir_component(component)?;
        }
    }
    core::str::from_utf8(raw).map_err(|_| DirNameError::NotUtf8)
}

/// Join a validated component onto a validated directory path.
///
/// Only sound for inputs that came through [`validate_dir_component`] and
/// [`validate_dir_path`]: with no separator in `name` the result has exactly one
/// more component than `dir`, and that component is `name`.
pub fn join_component(dir: &str, name: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(dir.len() + 1 + name.len());
    out.push_str(dir.trim_end_matches('/'));
    out.push('/');
    out.push_str(name);
    out
}
