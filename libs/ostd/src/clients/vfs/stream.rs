// SPDX-License-Identifier: MPL-2.0

//! Handle-addressed streaming access: paged directory listings, bounded
//! ranged file reads and the per-handle modification state.
//!
//! Every call is one request; nothing here buffers a whole directory or file.
//! The service re-authorizes each page, chunk and state query against the
//! handle's path.

use super::read_file::session::VfsReadOps;
use super::read_file::wire::{vfs_err_from_code, MAX_READ_CHUNK};
use crate::{ViError, ViResult};
use api::dir_handles::ViDirHandle;
use api::ipc::vfs_dir_page::{self, Entry, MalformedPage};
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;

/// Most bytes one [`read_chunk`][super::VfsClient::read_chunk] returns.
pub const VFS_READ_CHUNK_MAX: usize = MAX_READ_CHUNK as usize;
/// Most packed entry bytes one directory page carries; a
/// [`list_page`][super::VfsClient::list_page] buffer of this size always fits.
pub const VFS_DIR_PAGE_MAX: usize = MAX_READ_CHUNK as usize;

/// One page of a directory listing, borrowing the caller's buffer.
#[derive(Debug, Clone, Copy)]
pub struct VfsDirPage<'b> {
    bytes: &'b [u8],
    /// Cursor for the following page. Meaningless once `complete`.
    pub next: u32,
    /// This page ends the listing. A listing that never reached a complete
    /// page is partial.
    pub complete: bool,
    /// A child of the directory was created, removed or renamed after its
    /// handle was opened: the pages may not form one consistent listing.
    pub changed: bool,
}

impl<'b> VfsDirPage<'b> {
    /// Entries in service order. Validated when the page was received.
    pub fn entries(&self) -> impl Iterator<Item = Entry<'b>> + 'b {
        vfs_dir_page::entries(self.bytes).filter_map(Result::ok)
    }

    /// Packed entry bytes of this page.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// [`VfsResponse::FileState`] of one open file handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsFileState {
    /// Current size of the file behind the handle.
    pub size: u64,
    /// A content or binding mutation reached the path after the handle was
    /// opened (sticky; same-byte rewrites count).
    pub modified: bool,
}

pub(super) fn open_root_dir<T: VfsReadOps>(ops: &mut T, path: &str) -> ViResult<ViDirHandle> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::OpenRootDir { path }, &mut resp_buf)? {
        VfsResponse::DirHandle(dir) => Ok(dir),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

/// `out` must hold the page ([`VFS_DIR_PAGE_MAX`] always does); a smaller
/// buffer that the page outgrows is `OutOfMemory`, a malformed page `IO`.
pub(super) fn list_page<'b, T: VfsReadOps>(
    ops: &mut T,
    dir: ViDirHandle,
    cursor: u32,
    out: &'b mut [u8],
) -> ViResult<VfsDirPage<'b>> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let (next, flags, len) = match ops.call(&VfsRequest::ListAtPage { dir, cursor }, &mut resp_buf)? {
        VfsResponse::DirPage { next, flags, entries } => {
            if vfs_dir_page::entries(entries).any(|entry| entry == Err(MalformedPage)) {
                return Err(ViError::IO);
            }
            let target = out.get_mut(..entries.len()).ok_or(ViError::OutOfMemory)?;
            target.copy_from_slice(entries);
            (next, flags, entries.len())
        }
        VfsResponse::Err(code) => return Err(vfs_err_from_code(code)),
        _ => return Err(ViError::IO),
    };
    Ok(VfsDirPage {
        bytes: &out[..len],
        next,
        complete: flags & vfs_dir_page::COMPLETE != 0,
        changed: flags & vfs_dir_page::CHANGED != 0,
    })
}

pub(super) fn open_file_at<T: VfsReadOps>(
    ops: &mut T,
    dir: ViDirHandle,
    name: &str,
) -> ViResult<ViVfsFileHandle> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::OpenFileAt { dir, name }, &mut resp_buf)? {
        VfsResponse::FileHandle(file) => Ok(file),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

/// Read up to `min(buf.len(), VFS_READ_CHUNK_MAX)` bytes at `offset`.
/// `Ok(0)` for a non-empty `buf` is the end of the file.
pub(super) fn read_chunk<T: VfsReadOps>(
    ops: &mut T,
    file: ViVfsFileHandle,
    offset: u64,
    buf: &mut [u8],
) -> ViResult<usize> {
    let max = buf.len().min(VFS_READ_CHUNK_MAX);
    if max == 0 {
        return Ok(0);
    }
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let request = VfsRequest::ReadFileHandle { file, offset, max: max as u32 };
    match ops.call(&request, &mut resp_buf)? {
        VfsResponse::Data(chunk) if chunk.len() <= max => {
            buf[..chunk.len()].copy_from_slice(chunk);
            Ok(chunk.len())
        }
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

pub(super) fn file_state<T: VfsReadOps>(ops: &mut T, file: ViVfsFileHandle) -> ViResult<VfsFileState> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::FileHandleState { file }, &mut resp_buf)? {
        VfsResponse::FileState { size, modified } => Ok(VfsFileState { size, modified }),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

pub(super) fn close_file<T: VfsReadOps>(ops: &mut T, file: ViVfsFileHandle) -> ViResult<()> {
    expect_ok(ops, &VfsRequest::CloseFile { file })
}

pub(super) fn close_dir<T: VfsReadOps>(ops: &mut T, dir: ViDirHandle) -> ViResult<()> {
    expect_ok(ops, &VfsRequest::CloseDir { dir })
}

fn expect_ok<T: VfsReadOps>(ops: &mut T, req: &VfsRequest<'_>) -> ViResult<()> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(req, &mut resp_buf)? {
        VfsResponse::Ok => Ok(()),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}
