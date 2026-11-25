//! Open files of the VFS share: handles, sizes, and bytes moved through
//! Grants in `VFS_GRANT_CHUNK` pieces, as `virtio_blk` moves its disk.
//!
//! A Grant is allocated, shared, used and dropped inside one call, on the
//! thread serving the request: the kernel keys a Grant by the task that
//! registered it.

use alloc::string::String;

use abi::dir_handles::ViDirHandle;
use wire::ipc::{VfsRequest, VfsResponse};
use wire::proto::vfs::VfsError;
use wire::vfs_file_handles::ViVfsFileHandle;

use super::errno::{EACCES, EBADF, EIO, ENOSPC, EOPNOTSUPP};
use super::vfs_share::{refusal, VfsShare};
use crate::virtio_blk::{alloc_chunk_grant, chunk_timeout_ticks};

/// IPC budget of a request that moves no file bytes, in scheduler ticks.
pub(super) const DIR_TICKS: u64 = 2000;
/// Grant sharing modes: the VFS fills a read Grant and reads a write Grant.
const GRANT_VFS_WRITES: u8 = 2;
const GRANT_VFS_READS: u8 = 1;

pub struct VfsFile {
    handle: ViVfsFileHandle,
    /// The first directory handle derived to reach the file, held while it
    /// is open.
    chain: Option<ViDirHandle>,
}

/// Linux errno of a VFS error byte; `Io` (not found, device failure or a
/// backend refusal: the VFS does not tell) as `io`.
pub(super) fn vfs_errno(code: u8, io: u32) -> u32 {
    match VfsError::from_u8(code) {
        Some(VfsError::Io) => io,
        Some(VfsError::Quota) => ENOSPC,
        Some(VfsError::Denied) => EACCES,
        Some(VfsError::Handle) => EBADF,
        Some(VfsError::Unsupported) => EOPNOTSUPP,
        Some(VfsError::Malformed) | None => EIO,
    }
}

impl VfsShare {
    pub(super) fn open_file(&mut self, path: &[String]) -> Result<VfsFile, u32> {
        let (name, parent) = path.split_last().ok_or(EIO)?;
        let (dir, chain) = self.open_dir(parent)?;
        let opened = match self.call(&VfsRequest::OpenFileAt { dir, name }, DIR_TICKS) {
            Ok(VfsResponse::FileHandle(handle)) => Ok(handle),
            Ok(other) => Err(refusal(&other, EIO)),
            Err(errno) => Err(errno),
        };
        match opened {
            Ok(handle) => Ok(VfsFile { handle, chain }),
            Err(errno) => {
                self.close_dir(chain);
                Err(errno)
            }
        }
    }

    pub(super) fn close_file(&mut self, file: VfsFile) {
        let _ = self.call(&VfsRequest::CloseFile { file: file.handle }, DIR_TICKS);
        self.close_dir(file.chain);
    }

    pub(super) fn file_size(&mut self, file: &VfsFile) -> Result<u64, u32> {
        let request = VfsRequest::FileHandleState { file: file.handle };
        match self.call(&request, DIR_TICKS)? {
            VfsResponse::FileState { size, .. } => Ok(size),
            other => Err(refusal(&other, EIO)),
        }
    }

    /// RamFS (`/tmp`) has no device to sync to and answers `Io`.
    pub(super) fn sync_file(&mut self, file: &VfsFile) -> Result<(), u32> {
        let request = VfsRequest::SyncHandle { file: file.handle };
        let ticks = chunk_timeout_ticks(wire::ipc::VFS_GRANT_CHUNK);
        let volatile = self.volatile;
        match self.call(&request, ticks)? {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) if volatile && code == VfsError::Io as u8 => Ok(()),
            other => Err(refusal(&other, EIO)),
        }
    }

    pub(super) fn read_at(
        &mut self,
        file: &VfsFile,
        offset: u64,
        out: &mut [u8],
    ) -> Result<usize, u32> {
        let mut done = 0;
        while done < out.len() {
            let (mut grant, chunk) = alloc_chunk_grant(out.len() - done).ok_or(EIO)?;
            if !ostd::syscall::sys_grant_share(grant.id(), self.tid, GRANT_VFS_WRITES) {
                return Err(EIO);
            }
            let request = VfsRequest::ReadHandleGrant {
                file: file.handle,
                offset: offset + done as u64,
                size: chunk,
                grant: grant.id(),
            };
            let got = match self.call(&request, chunk_timeout_ticks(chunk))? {
                VfsResponse::GrantDone { bytes } if bytes <= chunk => bytes,
                other => return Err(refusal(&other, EIO)),
            };
            grant.with_bytes(|data| out[done..done + got].copy_from_slice(&data[..got]));
            done += got;
            if got < chunk {
                break;
            }
        }
        Ok(done)
    }

    pub(super) fn write_at(&mut self, file: &VfsFile, offset: u64, data: &[u8]) -> Result<(), u32> {
        let mut done = 0;
        while done < data.len() {
            let (mut grant, chunk) = alloc_chunk_grant(data.len() - done).ok_or(EIO)?;
            grant.with_bytes_mut(|bytes| bytes[..chunk].copy_from_slice(&data[done..done + chunk]));
            if !ostd::syscall::sys_grant_share(grant.id(), self.tid, GRANT_VFS_READS) {
                return Err(EIO);
            }
            let request = VfsRequest::WriteHandleGrant {
                file: file.handle,
                offset: offset + done as u64,
                grant: grant.id(),
                bytes: chunk,
            };
            match self.call(&request, chunk_timeout_ticks(chunk))? {
                VfsResponse::GrantDone { bytes } if bytes == chunk => {}
                other => return Err(refusal(&other, EIO)),
            }
            done += chunk;
        }
        Ok(())
    }
}
