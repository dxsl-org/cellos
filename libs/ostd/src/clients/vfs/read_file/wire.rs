use wire::proto::vfs::VfsError;

use crate::ViError;

pub(in crate::clients::vfs) const MAX_READ_CHUNK: u32 = 4000;

pub(in crate::clients::vfs) fn vfs_err_from_code(code: u8) -> ViError {
    match VfsError::from_u8(code) {
        Some(VfsError::Io) => ViError::IO,
        Some(VfsError::Quota) => ViError::OutOfMemory,
        Some(VfsError::Denied) => ViError::PermissionDenied,
        Some(VfsError::Handle) => ViError::NotFound,
        Some(VfsError::Unsupported) => ViError::NotSupported,
        Some(VfsError::Malformed) | None => ViError::Unknown,
    }
}
