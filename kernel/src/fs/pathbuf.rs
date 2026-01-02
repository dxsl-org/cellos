//! A module for owned, mutable paths.
//!
//! This module provides a `PathBuf` struct that is an owned, mutable counterpart
//! to the `Path` slice. It provides methods for manipulating the path in place,
//! such as `push` and `pop`.

use super::path::Path;
use alloc::string::String;
use core::{borrow::Borrow, ops::Deref};

/// An owned, mutable path, akin to `String`.
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Clone, Default)]
pub struct PathBuf {
    inner: String,
}

impl PathBuf {
    /// Creates a new, empty `PathBuf`.
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    /// Creates a new `PathBuf` with a given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: String::with_capacity(capacity),
        }
    }

    /// Coerces to a `Path` slice.
    pub fn as_path(&self) -> &Path {
        self
    }

    /// Extends `self` with `path`.
    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();

        if path.is_absolute() {
            self.inner = path.as_str().into();
            return;
        }

        if !self.inner.is_empty() && !self.inner.ends_with('/') {
            self.inner.push('/');
        }

        self.inner.push_str(path.as_str());
    }

    /// Truncates `self` to its parent.
    pub fn pop(&mut self) -> bool {
        match self.as_path().parent() {
            Some(parent) => {
                self.inner.truncate(parent.as_str().len());
                true
            }
            None => false,
        }
    }

    /// Updates the file name of the path.
    pub fn set_file_name<S: AsRef<str>>(&mut self, file_name: S) {
        if self.as_path().file_name().is_some() {
            self.pop();
        }

        self.push(Path::new(file_name.as_ref()));
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl<T: AsRef<str>> From<T> for PathBuf {
    fn from(s: T) -> Self {
        Self {
            inner: s.as_ref().into(),
        }
    }
}

impl Deref for PathBuf {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        Path::new(&self.inner)
    }
}

impl Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self
    }
}
