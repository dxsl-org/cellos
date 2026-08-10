use crate::ViError;

pub(super) const MAX_READ_CHUNK: u32 = 4000;
pub(super) const VFS_ERR_IO: u8 = 1;
pub(super) const VFS_ERR_QUOTA: u8 = 2;
pub(super) const VFS_ERR_DENIED: u8 = 3;
pub(super) const VFS_ERR_HANDLE: u8 = 4;

pub(in crate::clients::vfs) fn vfs_err_from_code(code: u8) -> ViError {
    match code {
        VFS_ERR_IO => ViError::IO,
        VFS_ERR_QUOTA => ViError::OutOfMemory,
        VFS_ERR_DENIED => ViError::PermissionDenied,
        VFS_ERR_HANDLE => ViError::NotFound,
        _ => ViError::Unknown,
    }
}
