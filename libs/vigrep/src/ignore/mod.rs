// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/ignore/src/lib.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

/*!
The ignore crate provides a fast recursive directory iterator that respects
various filters such as globs, file types and `.gitignore` files. The precise
matching rules and precedence is explained in the documentation for
`WalkBuilder`.

Secondarily, this crate exposes gitignore and file type matchers for use cases
that demand more fine-grained control.

# vigrep adaptation

Only the matchers are ported: [`gitignore`], [`overrides`], the error types
and the per-directory precedence logic of upstream's `dir.rs`, which the
serial walker in [`crate::walk`] drives. Paths are bytes with `/` separators.
Ignore rules are fed from bytes the caller has read
([`gitignore::GitignoreBuilder::add_bytes`] / `add_line`); this module never
opens a file. Upstream's parallel and serial walkers, file types, global and
`.git/info/exclude` discovery, ancestor-above-root discovery and repository
(`.git`/`.jj`) detection are not ported: `.gitignore` rules apply whether or
not a repository is present (upstream's `require_git(false)`).
*/

#![deny(missing_docs)]

#[allow(unused_imports)]
use crate::prelude::*;

mod dir;
pub mod gitignore;
pub mod overrides;
mod pathutil;

pub(crate) use crate::ignore::dir::{Ignore, IgnoreBuilder};

/// Represents an error that can occur when parsing a gitignore file.
///
/// vigrep: upstream's walk-only `WithDepth` and `Loop` variants (and
/// `Error::depth`) and the file-type `UnrecognizedFileType` and
/// `InvalidDefinition` variants are not ported; walk errors are
/// [`crate::walk::WalkError`]. Paths are bytes and I/O errors are
/// [`crate::io::Error`].
#[derive(Clone, Debug)]
pub enum Error {
    /// A collection of "soft" errors. These occur when adding an ignore
    /// file partially succeeded.
    Partial(Vec<Error>),
    /// An error associated with a specific line number.
    WithLineNumber {
        /// The line number.
        line: u64,
        /// The underlying error.
        err: Box<Error>,
    },
    /// An error associated with a particular file path.
    WithPath {
        /// The file path.
        path: Vec<u8>,
        /// The underlying error.
        err: Box<Error>,
    },
    /// An error that occurs when doing I/O, such as reading an ignore file.
    Io(crate::io::Error),
    /// An error that occurs when trying to parse a glob.
    Glob {
        /// The original glob that caused this error. This glob, when
        /// available, always corresponds to the glob provided by an end user.
        /// e.g., It is the glob as written in a `.gitignore` file.
        ///
        /// (This glob may be distinct from the glob that is actually
        /// compiled, after accounting for `gitignore` semantics.)
        glob: Option<String>,
        /// The underlying glob error as a string.
        err: String,
    },
}

impl Error {
    /// Returns true if this is a partial error.
    ///
    /// A partial error occurs when only some operations failed while others
    /// may have succeeded. For example, an ignore file may contain an invalid
    /// glob among otherwise valid globs.
    pub fn is_partial(&self) -> bool {
        match *self {
            Error::Partial(_) => true,
            Error::WithLineNumber { ref err, .. } => err.is_partial(),
            Error::WithPath { ref err, .. } => err.is_partial(),
            _ => false,
        }
    }

    /// Returns true if this error is exclusively an I/O error.
    pub fn is_io(&self) -> bool {
        match *self {
            Error::Partial(ref errs) => errs.len() == 1 && errs[0].is_io(),
            Error::WithLineNumber { ref err, .. } => err.is_io(),
            Error::WithPath { ref err, .. } => err.is_io(),
            Error::Io(_) => true,
            Error::Glob { .. } => false,
        }
    }

    /// Inspect the original [`crate::io::Error`] if there is one.
    ///
    /// [`None`] is returned if the [`Error`] doesn't correspond to an
    /// I/O error.
    ///
    /// This method returns a borrowed value that is bound to the lifetime of
    /// the [`Error`]. To obtain an owned value, [`Error::into_io_error`] can
    /// be used instead.
    pub fn io_error(&self) -> Option<&crate::io::Error> {
        match *self {
            Error::Partial(ref errs) => {
                if errs.len() == 1 {
                    errs[0].io_error()
                } else {
                    None
                }
            }
            Error::WithLineNumber { ref err, .. } => err.io_error(),
            Error::WithPath { ref err, .. } => err.io_error(),
            Error::Io(ref err) => Some(err),
            Error::Glob { .. } => None,
        }
    }

    /// Similar to [`Error::io_error`] except consumes self to convert to the
    /// original [`crate::io::Error`] if one exists.
    pub fn into_io_error(self) -> Option<crate::io::Error> {
        match self {
            Error::Partial(mut errs) => {
                if errs.len() == 1 {
                    errs.remove(0).into_io_error()
                } else {
                    None
                }
            }
            Error::WithLineNumber { err, .. } => err.into_io_error(),
            Error::WithPath { err, .. } => err.into_io_error(),
            Error::Io(err) => Some(err),
            Error::Glob { .. } => None,
        }
    }

    /// Turn an error into a tagged error with the given file path.
    fn with_path<P: AsRef<[u8]>>(self, path: P) -> Error {
        Error::WithPath { path: path.as_ref().to_vec(), err: Box::new(self) }
    }

    /// Turn an error into a tagged error with the given file path and line
    /// number. If path is empty, then it is omitted from the error.
    fn tagged<P: AsRef<[u8]>>(self, path: P, lineno: u64) -> Error {
        let errline =
            Error::WithLineNumber { line: lineno, err: Box::new(self) };
        if path.as_ref().is_empty() {
            return errline;
        }
        errline.with_path(path)
    }
}

impl core::error::Error for Error {}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Error::Partial(ref errs) => {
                let msgs: Vec<String> =
                    errs.iter().map(|err| err.to_string()).collect();
                write!(f, "{}", msgs.join("\n"))
            }
            Error::WithLineNumber { line, ref err } => {
                write!(f, "line {}: {}", line, err)
            }
            Error::WithPath { ref path, ref err } => {
                write!(f, "{}: {}", bstr::BStr::new(path), err)
            }
            Error::Io(ref err) => err.fmt(f),
            Error::Glob { glob: None, ref err } => write!(f, "{}", err),
            Error::Glob { glob: Some(ref glob), ref err } => {
                write!(f, "error parsing glob '{}': {}", glob, err)
            }
        }
    }
}

impl From<crate::io::Error> for Error {
    fn from(err: crate::io::Error) -> Error {
        Error::Io(err)
    }
}

#[derive(Debug, Default)]
struct PartialErrorBuilder(Vec<Error>);

impl PartialErrorBuilder {
    fn push(&mut self, err: Error) {
        self.0.push(err);
    }

    fn maybe_push(&mut self, err: Option<Error>) {
        if let Some(err) = err {
            self.push(err);
        }
    }

    fn into_error_option(mut self) -> Option<Error> {
        if self.0.is_empty() {
            None
        } else if self.0.len() == 1 {
            Some(self.0.pop().unwrap())
        } else {
            Some(Error::Partial(self.0))
        }
    }
}

/// The result of a glob match.
///
/// The type parameter `T` typically refers to a type that provides more
/// information about a particular match. For example, it might identify
/// the specific gitignore file and the specific glob pattern that caused
/// the match.
#[derive(Clone, Debug)]
pub enum Match<T> {
    /// The path didn't match any glob.
    None,
    /// The highest precedent glob matched indicates the path should be
    /// ignored.
    Ignore(T),
    /// The highest precedent glob matched indicates the path should be
    /// whitelisted.
    Whitelist(T),
}

impl<T> Match<T> {
    /// Returns true if the match result didn't match any globs.
    pub fn is_none(&self) -> bool {
        match *self {
            Match::None => true,
            Match::Ignore(_) | Match::Whitelist(_) => false,
        }
    }

    /// Returns true if the match result implies the path should be ignored.
    pub fn is_ignore(&self) -> bool {
        match *self {
            Match::Ignore(_) => true,
            Match::None | Match::Whitelist(_) => false,
        }
    }

    /// Returns true if the match result implies the path should be
    /// whitelisted.
    pub fn is_whitelist(&self) -> bool {
        match *self {
            Match::Whitelist(_) => true,
            Match::None | Match::Ignore(_) => false,
        }
    }

    /// Inverts the match so that `Ignore` becomes `Whitelist` and
    /// `Whitelist` becomes `Ignore`. A non-match remains the same.
    pub fn invert(self) -> Match<T> {
        match self {
            Match::None => Match::None,
            Match::Ignore(t) => Match::Whitelist(t),
            Match::Whitelist(t) => Match::Ignore(t),
        }
    }

    /// Return the value inside this match if it exists.
    pub fn inner(&self) -> Option<&T> {
        match *self {
            Match::None => None,
            Match::Ignore(ref t) => Some(t),
            Match::Whitelist(ref t) => Some(t),
        }
    }

    /// Apply the given function to the value inside this match.
    ///
    /// If the match has no value, then return the match unchanged.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Match<U> {
        match self {
            Match::None => Match::None,
            Match::Ignore(t) => Match::Ignore(f(t)),
            Match::Whitelist(t) => Match::Whitelist(f(t)),
        }
    }

    /// Return the match if it is not none. Otherwise, return other.
    pub fn or(self, other: Self) -> Self {
        if self.is_none() { other } else { self }
    }
}
