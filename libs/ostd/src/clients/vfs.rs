// SPDX-License-Identifier: MPL-2.0

//! VFS service client — ergonomic file-system access.

extern crate alloc;

use super::vierr_from_code;
use crate::service::VfsRef;
use crate::{ViError, ViResult};
use alloc::vec::Vec;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};

/// Ergonomic client for the VFS service.
///
/// Wraps [`VfsRef`] and hides request construction + postcard encoding.
/// Each method allocates a 4 KiB response buffer on the stack for the duration
/// of the call (freed on return).
///
/// # Legacy read contract
/// [`read_file`][Self::read_file] still sends `GetFile`, but the current VFS
/// replies with `DataPtr`. This client rejects that raw SAS pointer as
/// [`ViError::IO`]; callers migrate to bounded reads in later phases.
pub struct VfsClient {
    svc: VfsRef,
}

fn read_file_response(response: VfsResponse<'_>) -> ViResult<Vec<u8>> {
    match response {
        VfsResponse::Data(data) => Ok(data.to_vec()),
        // Current path dispatch uses local ERR_IO=1 and ERR_DENIED=3 codes.
        VfsResponse::Err(1) => Err(ViError::IO),
        VfsResponse::Err(3) => Err(ViError::PermissionDenied),
        VfsResponse::Err(_) => Err(ViError::Unknown),
        _ => Err(ViError::IO),
    }
}

impl VfsClient {
    /// Create a new unresolved client. Resolution is lazy (first call).
    pub fn new() -> Self {
        Self { svc: VfsRef::new() }
    }

    /// Read the full contents of a file at `path`.
    ///
    /// Returns copied response bytes. The current `DataPtr` reply is rejected
    /// rather than dereferenced or treated as empty success.
    pub fn read_file(&mut self, path: &str) -> ViResult<Vec<u8>> {
        let req = VfsRequest::GetFile(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        read_file_response(
            self.svc
                .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?,
        )
    }

    /// Write (create or overwrite) a file at `path` with `content`.
    ///
    /// Content is limited to ~3.9 KB per call by the IPC buffer.
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        let req = VfsRequest::Write { path, content };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Append `content` to a file at `path` (creates it if absent).
    pub fn append_file(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        let req = VfsRequest::Append { path, content };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Stat a path. Returns `(size_bytes, is_dir)`.
    pub fn stat(&mut self, path: &str) -> ViResult<(u64, bool)> {
        let req = VfsRequest::Stat(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Stat { size, is_dir } => Ok((size, is_dir)),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// List directory entries at `path`.
    ///
    /// Returns a newline-separated UTF-8 byte string of entry names.
    pub fn list_dir(&mut self, path: &str) -> ViResult<Vec<u8>> {
        let req = VfsRequest::ListDir(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Data(data) => Ok(data.to_vec()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Create a directory at `path`.
    pub fn mkdir(&mut self, path: &str) -> ViResult<()> {
        let req = VfsRequest::Mkdir(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Delete a file at `path`.
    pub fn unlink(&mut self, path: &str) -> ViResult<()> {
        let req = VfsRequest::Unlink(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)?
        {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Check whether a path exists (stat succeeds).
    pub fn exists(&mut self, path: &str) -> bool {
        self.stat(path).is_ok()
    }
}

impl Default for VfsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_response_copies_data() {
        assert_eq!(
            read_file_response(VfsResponse::Data(b"owned bytes")),
            Ok(b"owned bytes".to_vec())
        );
    }

    #[test]
    fn read_file_response_rejects_raw_pointer_reply() {
        assert_eq!(
            read_file_response(VfsResponse::DataPtr {
                ptr: 0x1000,
                len: 12,
            }),
            Err(ViError::IO)
        );
    }

    #[test]
    fn read_file_response_preserves_typed_vfs_error() {
        assert_eq!(
            read_file_response(VfsResponse::Err(3)),
            Err(ViError::PermissionDenied)
        );
        assert_eq!(read_file_response(VfsResponse::Err(1)), Err(ViError::IO));
    }
}
