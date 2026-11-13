//! `FsError` — why an [`FsBackend`](crate::backend::FsBackend) operation failed.
//!
//! VFS-internal: it never crosses the wire as itself. [`FsError::wire`] maps it
//! onto the existing `VfsError` bytes, so a client sees exactly the codes it
//! saw before backends reported typed errors (CP1: no new wire byte). The type
//! exists so that a failure is never mistaken for an answer — in particular a
//! read that hit a device error fails instead of returning truncated bytes.

use api::services::proto::vfs::VfsError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FsError {
    /// The path, or a component of it, does not exist.
    NotFound,
    /// A component the operation needs to be a directory is not one.
    NotADirectory,
    /// The operation needs a regular file and the path names a directory.
    IsADirectory,
    /// The directory still has entries.
    DirectoryNotEmpty,
    /// The target the operation would create already exists.
    AlreadyExists,
    /// The request names nothing this backend can address: outside its mount,
    /// the mount root where an entry is required, a `..` component, an invalid
    /// name, or an offset/length outside the representable file range.
    InvalidPath,
    /// The volume has no room for the result.
    NoSpace,
    /// The backend does not perform this operation (read-only backend, or an
    /// operation it never implements). The only variant with its own wire byte.
    Unsupported,
    /// The device reported a failure, a read timed out, or the on-disk
    /// structures could not be trusted. Nothing partial is returned.
    Io,
    /// A mutation's block outcome is unknown: the device may hold the old, the
    /// new or a torn state. Never retried.
    Indeterminate,
    /// The backend is not serving: no backend at the path, never mounted,
    /// poisoned or unavailable.
    Unavailable,
}

impl FsError {
    /// The `VfsError` byte a client sees: `Unsupported` keeps its own code,
    /// every other cause answers `Io` exactly as before typed errors.
    pub fn wire(self) -> u8 {
        match self {
            Self::Unsupported => VfsError::Unsupported as u8,
            _ => VfsError::Io as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FsError;

    #[test]
    fn only_unsupported_has_its_own_wire_byte() {
        assert_eq!(FsError::Unsupported.wire(), 6);
        for error in [
            FsError::NotFound,
            FsError::NotADirectory,
            FsError::IsADirectory,
            FsError::DirectoryNotEmpty,
            FsError::AlreadyExists,
            FsError::InvalidPath,
            FsError::NoSpace,
            FsError::Io,
            FsError::Indeterminate,
            FsError::Unavailable,
        ] {
            assert_eq!(error.wire(), 1, "{error:?}");
        }
    }
}
