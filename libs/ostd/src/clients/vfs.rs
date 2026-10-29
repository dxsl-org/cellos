// SPDX-License-Identifier: MPL-2.0

//! VFS service client — ergonomic file-system access.

extern crate alloc;

mod read_file;
mod stream;

use crate::service::VfsRef;
use crate::{ViError, ViResult};
use alloc::vec::Vec;
use api::cell_session::ObjectRefV1;
use api::dir_handles::ViDirHandle;
use api::ipc::{Observation, SaveExpect, SaveOutcome, VfsRequest, VfsResponse, IPC_BUF_SIZE, VFS_SAVE_CHUNK};
use api::vfs_file_handles::ViVfsFileHandle;
use read_file::wire::vfs_err_from_code;
pub use stream::{VfsDirPage, VfsFileState, VFS_DIR_PAGE_MAX, VFS_READ_CHUNK_MAX};

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

    /// Acquire a directory handle for the absolute `path` (`OpenRootDir`).
    pub fn open_root_dir(&mut self, path: &str) -> ViResult<ViDirHandle> {
        stream::open_root_dir(&mut self.svc, path)
    }

    /// One page of `dir`'s entries, copied into `out` (pass
    /// [`VFS_DIR_PAGE_MAX`] bytes). Start with `cursor` 0 and pass each page's
    /// `next` until a page is `complete`; a listing that stops earlier is
    /// partial. Holds no service state between pages.
    pub fn list_page<'b>(
        &mut self,
        dir: ViDirHandle,
        cursor: u32,
        out: &'b mut [u8],
    ) -> ViResult<VfsDirPage<'b>> {
        stream::list_page(&mut self.svc, dir, cursor, out)
    }

    /// Open the regular file `name` inside `dir` for ranged reads.
    pub fn open_file_at(&mut self, dir: ViDirHandle, name: &str) -> ViResult<ViVfsFileHandle> {
        stream::open_file_at(&mut self.svc, dir, name)
    }

    /// Read at most `min(buf.len(), VFS_READ_CHUNK_MAX)` bytes at `offset`
    /// into `buf`; returns the count, 0 at end of file.
    pub fn read_chunk(&mut self, file: ViVfsFileHandle, offset: u64, buf: &mut [u8]) -> ViResult<usize> {
        stream::read_chunk(&mut self.svc, file, offset, buf)
    }

    /// Current size of `file` and whether its path was mutated since it was
    /// opened.
    pub fn file_state(&mut self, file: ViVfsFileHandle) -> ViResult<VfsFileState> {
        stream::file_state(&mut self.svc, file)
    }

    /// Give up `file`.
    pub fn close_file(&mut self, file: ViVfsFileHandle) -> ViResult<()> {
        stream::close_file(&mut self.svc, file)
    }

    /// Give up `dir` and every handle derived from it.
    pub fn close_dir(&mut self, dir: ViDirHandle) -> ViResult<()> {
        stream::close_dir(&mut self.svc, dir)
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

    /// Remove an empty directory at `path`.
    pub fn rmdir(&mut self, path: &str) -> ViResult<()> {
        self.call_ok(&VfsRequest::Rmdir(path))
    }

    /// Remove the directory tree at `path` (VFS policy decides which trees qualify).
    pub fn rmdir_recursive(&mut self, path: &str) -> ViResult<()> {
        self.call_ok(&VfsRequest::RmdirRecursive(path))
    }

    /// Rename `old` to `new` (VFS accepts only a regular file on the same `/srv` mount
    /// and an absent destination).
    pub fn rename(&mut self, old: &str, new: &str) -> ViResult<()> {
        self.call_ok(&VfsRequest::Rename { old, new })
    }

    // ── D03 conditional save. `NotSupported` means the mount has none. ──

    /// Exact identity of `path` for a later conditional save.
    pub fn observe(&mut self, path: &str) -> ViResult<Observation> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&VfsRequest::Observe { path }, &mut resp_buf)? {
            VfsResponse::Observed(observation) => Ok(observation),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Stage `bytes` for `dst` and seal them; returns the transaction id. On any
    /// failure the transaction is aborted and nothing at `dst` changed.
    pub fn save_stage(&mut self, dst: &str, bytes: &[u8], sha256: [u8; 32]) -> ViResult<u32> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        let id = match self.svc.call::<VfsRequest, VfsResponse>(&VfsRequest::SaveBegin { dst }, &mut resp_buf)? {
            VfsResponse::SaveId(id) => id,
            VfsResponse::Err(code) => return Err(vfs_err_from_code(code)),
            _ => return Err(ViError::IO),
        };
        let staged = bytes.chunks(VFS_SAVE_CHUNK).enumerate().try_for_each(|(index, piece)| {
            let offset = (index * VFS_SAVE_CHUNK) as u64;
            self.call_ok(&VfsRequest::SaveChunk { id, offset, bytes: piece })
        }).and_then(|()| self.call_ok(&VfsRequest::SaveSeal { id, len: bytes.len() as u64, sha256 }));
        match staged {
            Ok(()) => Ok(id),
            Err(error) => {
                let _ = self.save_abort(id);
                Err(error)
            }
        }
    }

    /// Publish a sealed transaction if `dst` still matches `expect`. An IPC
    /// error here does not say whether publication happened: ask
    /// [`save_query`](Self::save_query), never commit again.
    pub fn save_commit(&mut self, id: u32, expect: SaveExpect) -> ViResult<SaveOutcome> {
        self.save_outcome(&VfsRequest::SaveCommit { id, expect })
    }

    /// The recorded outcome of this transaction's commit.
    pub fn save_query(&mut self, id: u32) -> ViResult<SaveOutcome> {
        self.save_outcome(&VfsRequest::SaveQuery { id })
    }

    pub fn save_abort(&mut self, id: u32) -> ViResult<()> {
        self.call_ok(&VfsRequest::SaveAbort { id })
    }

    /// Phase 09 host-trust lookup: one of `api::ipc::ssh_trust` for the configured
    /// `host`/`port` and the complete SSH key blob `key`.
    pub fn trust_lookup(&mut self, host: &str, port: u16, algorithm: &str, key: &[u8]) -> ViResult<u8> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        let req = VfsRequest::TrustLookup { host, port, algorithm, key };
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::Trust(code) => Ok(code),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Phase 09 S9-4c: ask for a first-trust challenge for a host and port the store does
    /// not hold. `Ok(Ok((confirmation, challenge)))` to begin and commit; `Ok(Err(code))`
    /// with an `api::ipc::ssh_trust` refusal (`KNOWN_EXACT`/`MISMATCH` need no prompt).
    /// `session`/`epoch` are the caller's own terminal session the confirmation runs on.
    pub fn trust_challenge(&mut self, host: &str, port: u16, algorithm: &str, key: &[u8], session: ObjectRefV1, epoch: u64)
        -> ViResult<Result<(ObjectRefV1, ObjectRefV1), u8>> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        let req = VfsRequest::TrustChallenge { host, port, algorithm, key, session, epoch };
        match self.svc.call::<VfsRequest, VfsResponse>(&req, &mut resp_buf)? {
            VfsResponse::TrustChallenged { confirmation, challenge } => Ok(Ok((confirmation, challenge))),
            VfsResponse::Trust(code) => Ok(Err(code)),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Phase 09 S9-4c: complete the caller's own challenge; one of `api::ipc::ssh_trust`
    /// (`PENDING` until the confirmation is decided).
    pub fn trust_commit(&mut self, challenge: ObjectRefV1) -> ViResult<u8> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(&VfsRequest::TrustCommit { challenge }, &mut resp_buf)? {
            VfsResponse::Trust(code) => Ok(code),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn save_outcome(&mut self, req: &VfsRequest<'_>) -> ViResult<SaveOutcome> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<VfsRequest, VfsResponse>(req, &mut resp_buf)? {
            VfsResponse::SaveOutcome(outcome) => Ok(outcome),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn call_ok(&mut self, req: &VfsRequest<'_>) -> ViResult<()> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<VfsRequest, VfsResponse>(req, &mut resp_buf)?
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
