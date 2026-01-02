//! A module for path manipulation that works with string slices.
//!
//! This module provides a `Path` struct that is a thin wrapper around `&str`,
//! offering various methods for path inspection and manipulation.

use alloc::{borrow::ToOwned, vec::Vec};

use super::pathbuf::PathBuf;

/// Represents a path slice, akin to `&str`.
///
/// This struct provides a number of methods for inspecting a path,
/// including breaking the path into its components, determining if it's
/// absolute, and more.
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Path {
    inner: str,
}

impl Path {
    /// Creates a new `Path` from a string slice.
    ///
    /// This is a cost-free conversion.
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Self {
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    /// Returns the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Determines whether the path is absolute.
    pub fn is_absolute(&self) -> bool {
        self.inner.starts_with('/')
    }

    /// Determines whether the path is relative.
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    /// Produces an iterator over the components of the path.
    pub fn components(&self) -> Components<'_> {
        Components {
            remaining: &self.inner,
        }
    }

    /// Joins two paths together.
    pub fn join(&self, other: &Path) -> PathBuf {
        let mut ret: PathBuf = PathBuf::with_capacity(self.inner.len() + other.inner.len());

        ret.push(self);
        ret.push(other);

        ret
    }

    /// Strips a prefix from the path.
    pub fn strip_prefix(&self, base: &Path) -> Option<&Path> {
        if self.inner.starts_with(&base.inner) {
            // If the prefixes are the same and they have the same length, the
            // whole string is the prefix.
            if base.inner.len() == self.inner.len() {
                return Some(Path::new(""));
            }
            if self.inner.as_bytes().get(base.inner.len()) == Some(&b'/') {
                let stripped = &self.inner[base.inner.len()..];
                // If the base ends with a slash, we don't want a leading slash on the result
                if base.inner.ends_with('/') {
                    return Some(Path::new(stripped));
                }
                return Some(Path::new(&stripped[1..]));
            }
        }
        None
    }

    /// Returns the parent directory of the path.
    pub fn parent(&self) -> Option<&Path> {
        let mut components = self.components().collect::<Vec<_>>();
        if components.len() <= 1 {
            return None;
        }
        components.pop();
        let parent_len = components.iter().map(|s| s.len()).sum::<usize>() + components.len() - 1;
        let end = if self.is_absolute() {
            parent_len + 1
        } else {
            parent_len
        };

        Some(Path::new(&self.inner[..end]))
    }

    /// Returns the final component of the path, if there is one.
    pub fn file_name(&self) -> Option<&str> {
        self.components().last()
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;

    fn to_owned(&self) -> Self::Owned {
        PathBuf::from(self.as_str())
    }
}

/// An iterator over the components of a `Path`.
#[derive(Clone, Debug)]
pub struct Components<'a> {
    remaining: &'a str,
}

impl<'a> Iterator for Components<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        // Trim leading slashes
        self.remaining = self.remaining.trim_start_matches('/');

        if self.remaining.is_empty() {
            return None;
        }

        match self.remaining.find('/') {
            Some(index) => {
                let component = &self.remaining[..index];
                self.remaining = &self.remaining[index..];
                if component == "." {
                    self.next()
                } else {
                    Some(component)
                }
            }
            None => {
                let component = self.remaining;
                self.remaining = "";
                if component == "." {
                    self.next()
                } else {
                    Some(component)
                }
            }
        }
    }
}
