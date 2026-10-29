//! Core-only byte reader and error, used by the adapted searcher in place of
//! `std::io::Read` and `std::io::Error`.
//!
//! Only what the adapted upstream code needs is provided: a pull-style
//! [`Read`] trait and an [`Error`] that carries an [`ErrorKind`] plus a
//! message. Implementations over Cellos VFS file handles live with their
//! consumers; this crate implements [`Read`] for byte slices only.

use core::fmt;

use crate::prelude::*;

/// The category of an [`Error`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The named entity does not exist.
    NotFound,
    /// The caller lacks the authority to perform the operation.
    PermissionDenied,
    /// Data read from a source was not valid for the operation.
    InvalidData,
    /// A parameter supplied by the caller was invalid.
    InvalidInput,
    /// A directory operation was applied to something that is not a
    /// directory.
    NotADirectory,
    /// The operation was interrupted and may be retried.
    Interrupted,
    /// The operation is not supported by the source.
    Unsupported,
    /// Any other failure.
    Other,
}

impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "entity not found",
            ErrorKind::PermissionDenied => "permission denied",
            ErrorKind::InvalidData => "invalid data",
            ErrorKind::InvalidInput => "invalid input parameter",
            ErrorKind::NotADirectory => "not a directory",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Other => "other error",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An I/O error: a kind plus an optional human readable message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    message: Box<str>,
}

impl Error {
    /// Create an error of the given kind with a message.
    pub fn new<M: fmt::Display>(kind: ErrorKind, message: M) -> Error {
        Error { kind, message: message.to_string().into_boxed_str() }
    }

    /// Create an [`ErrorKind::Other`] error with a message.
    pub fn other<M: fmt::Display>(message: M) -> Error {
        Error::new(ErrorKind::Other, message)
    }

    /// The kind of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message of this error; empty when the error was created from a
    /// bare kind.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { kind, message: Box::from("") }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            f.write_str(self.kind.as_str())
        } else {
            f.write_str(&self.message)
        }
    }
}

impl core::error::Error for Error {}

/// A source of bytes.
///
/// Semantics follow `std::io::Read::read`: fill a prefix of `buf` and return
/// its length; `Ok(0)` with a non-empty `buf` means end of input. An error of
/// kind [`ErrorKind::Interrupted`] may be retried by the caller.
pub trait Read {
    /// Pull bytes into `buf`, returning how many were written.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
}

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let n = core::cmp::min(buf.len(), self.len());
        let (head, tail) = self.split_at(n);
        buf[..n].copy_from_slice(head);
        *self = tail;
        Ok(n)
    }
}

impl<R: Read + ?Sized> Read for &mut R {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        (**self).read(buf)
    }
}

impl<R: Read + ?Sized> Read for Box<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        (**self).read(buf)
    }
}

/// Host tests only: lets ported upstream test sinks that write into a
/// `Vec<u8>` through `std::io::Write` use `?` with this error type.
#[cfg(test)]
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::other(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_reader_drains_in_order() {
        let mut src: &[u8] = b"abcdef";
        let mut buf = [0u8; 4];
        assert_eq!(src.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf, b"abcd");
        assert_eq!(src.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], b"ef");
        assert_eq!(src.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn error_display_prefers_message() {
        assert_eq!(Error::other("boom").to_string(), "boom");
        assert_eq!(
            Error::from(ErrorKind::PermissionDenied).to_string(),
            "permission denied"
        );
        assert_eq!(
            Error::new(ErrorKind::NotFound, "x").kind(),
            ErrorKind::NotFound
        );
    }
}
