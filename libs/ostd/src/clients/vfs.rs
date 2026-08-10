// SPDX-License-Identifier: MPL-2.0

//! VFS service client — ergonomic file-system access.

extern crate alloc;

mod read_file;

use crate::service::VfsRef;
use crate::{ViError, ViResult};
use alloc::vec::Vec;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use read_file::wire::vfs_err_from_code;

const DEFAULT_READ_FILE_MAX_BYTES: usize = 64 * 1024;

/// Ergonomic client for the VFS service.
///
/// Wraps [`VfsRef`] and hides request construction + postcard encoding.
/// Each method allocates a 4 KiB response buffer on the stack for the duration
/// of the call (freed on return).
///
pub struct VfsClient {
    svc: VfsRef,
}

impl VfsClient {
    /// Create a new unresolved client. Resolution is lazy (first call).
    pub fn new() -> Self {
        Self { svc: VfsRef::new() }
    }

    /// Read the contents of a file at `path`, capped at 64 KiB.
    ///
    /// Uses handle-addressed reads only: `OpenRootDir("/")`, validated `OpenDir`
    /// traversal, `OpenFileAt`, repeated `ReadFileHandle { max <= 4000 }`, and
    /// `CloseFile`/`CloseDir` cleanup on every path.
    ///
    /// This is a bounded multi-chunk read, not a snapshot read: each chunk is
    /// re-authorized independently by VFS, so concurrent same-size writers may
    /// produce mixed old/new content across chunk boundaries.
    pub fn read_file(&mut self, path: &str) -> ViResult<Vec<u8>> {
        self.read_file_bounded(path, DEFAULT_READ_FILE_MAX_BYTES)
    }

    /// Read the contents of a file at `path`, refusing payloads above `max_bytes`.
    ///
    /// Returns `ViError::OutOfMemory` when the file would exceed `max_bytes`.
    /// Like [`read_file`][Self::read_file], this is bounded and weakly
    /// consistent across chunks rather than a snapshot.
    pub fn read_file_bounded(&mut self, path: &str, max_bytes: usize) -> ViResult<Vec<u8>> {
        read_file::read_file(&mut self.svc, path, max_bytes)
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
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
