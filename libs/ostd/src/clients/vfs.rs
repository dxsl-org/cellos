// SPDX-License-Identifier: MPL-2.0

//! VFS service client — ergonomic file-system access.

extern crate alloc;

use alloc::vec::Vec;
use api::ipc::{IPC_BUF_SIZE, VfsRequest, VfsResponse};
use crate::{ViError, ViResult};
use crate::service::VfsRef;
use super::vierr_from_code;

/// Ergonomic client for the VFS service.
///
/// Wraps [`VfsRef`] and hides request construction + postcard encoding.
/// Each method allocates a 4 KiB response buffer on the stack for the duration
/// of the call (freed on return).
///
/// # Large files
/// [`read_file`][Self::read_file] uses `GetFile`, which transfers the entire
/// file contents in one IPC message.  Files larger than ~4 KB should use the
/// Grant-based API (`VfsRequest::ReadGrant`) directly via [`VfsRef`].
pub struct VfsClient {
    svc: VfsRef,
}

impl VfsClient {
    /// Create a new unresolved client. Resolution is lazy (first call).
    pub fn new() -> Self {
        Self { svc: VfsRef::new() }
    }

    /// Read the full contents of a file at `path`.
    ///
    /// Returns the raw byte contents.  Limited to ~4 KB by the IPC buffer;
    /// use the Grant API for larger files.
    pub fn read_file(&mut self, path: &str) -> ViResult<Vec<u8>> {
        let req = VfsRequest::GetFile(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Data(data) => Ok(data.to_vec()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Write (create or overwrite) a file at `path` with `content`.
    ///
    /// Content is limited to ~3.9 KB per call by the IPC buffer.
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        let req = VfsRequest::Write { path, content };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Append `content` to a file at `path` (creates it if absent).
    pub fn append_file(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        let req = VfsRequest::Append { path, content };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Stat a path. Returns `(size_bytes, is_dir)`.
    pub fn stat(&mut self, path: &str) -> ViResult<(u64, bool)> {
        let req = VfsRequest::Stat(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
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
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Data(data) => Ok(data.to_vec()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Create a directory at `path`.
    pub fn mkdir(&mut self, path: &str) -> ViResult<()> {
        let req = VfsRequest::Mkdir(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Delete a file at `path`.
    pub fn unlink(&mut self, path: &str) -> ViResult<()> {
        let req = VfsRequest::Unlink(path);
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
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
    fn default() -> Self { Self::new() }
}
