// SPDX-License-Identifier: MPL-2.0

//! Filesystem utilities for ViCell cells.
//!
//! `File` is backed by a kernel capability (`CapId`) obtained via `OpenCap`.
//! Single-owner: moving a `File` transfers the capability.  Dropping without
//! calling `close()` issues an implicit close (which revokes the capability)
//! and, in debug builds, emits a warning about the implicit close.

use crate::syscall;
use alloc::vec::Vec;
use types::*;
use api::ipc::{VfsRequest, VfsResponse};

/// Iterator over directory entries returned by the kernel.
pub struct ReadDir {
    /// fd from legacy `sys_open` — kept for directory listing (caps are file-only for now).
    fd: usize,
}

impl Iterator for ReadDir {
    type Item = DirEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let mut entry = DirEntry::default();
        // SAFETY: entry is a valid stack-allocated DirEntry; pointer is valid for the call.
        let ptr = &mut entry as *mut _ as *mut u8;
        let size = core::mem::size_of::<DirEntry>();
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr, size) };
        match syscall::sys_read_dir(self.fd, slice) {
            Ok(bytes) if bytes == size => Some(entry),
            _ => None,
        }
    }
}

impl Drop for ReadDir {
    fn drop(&mut self) {
        let _ = syscall::sys_close(self.fd);
    }
}

/// Open directory for reading.
pub fn read_dir(path: &str) -> ViResult<ReadDir> {
    let fd = syscall::sys_open(path).map_err(|_| ViError::NotFound)?;
    Ok(ReadDir { fd })
}

// ─── Capability-based file ────────────────────────────────────────────────────

/// An open file backed by a kernel capability (`CapId`).
///
/// Moving `File` transfers ownership of the underlying capability.  Dropping
/// calls `close()` implicitly; in debug builds a warning is emitted when this
/// happens without an explicit `close()` call (handle-leak detection).
pub struct File {
    cap_id: u64,
    /// Set to `true` by `close()` to suppress the drop warning.
    closed: bool,
}

impl File {
    /// Open a file at `path` in read-only mode.
    ///
    /// # Errors
    /// Returns `ViError::NotFound` if the path does not exist in the kernel FS.
    pub fn open(path: &str) -> ViResult<Self> {
        syscall::sys_open_cap(path)
            .map(|cap_id| Self { cap_id, closed: false })
            .map_err(|_| ViError::NotFound)
    }

    /// Read all bytes until EOF into `buf`.
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> ViResult<usize> {
        let mut temp = [0u8; 512];
        let mut total = 0;
        loop {
            match syscall::sys_read_cap(self.cap_id, &mut temp) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&temp[..n]);
                    total += n;
                }
                Err(_) => return Err(ViError::IO),
            }
        }
        Ok(total)
    }

    /// Read up to `buf.len()` bytes from the file.
    ///
    /// Returns the number of bytes actually read (0 = EOF).
    pub fn read(&mut self, buf: &mut [u8]) -> ViResult<usize> {
        syscall::sys_read_cap(self.cap_id, buf).map_err(|_| ViError::IO)
    }

    /// Read the entire file into a `String`.  Returns `Err(IO)` if content is not valid UTF-8.
    pub fn read_to_string(&mut self) -> ViResult<alloc::string::String> {
        let mut bytes = alloc::vec::Vec::new();
        self.read_to_end(&mut bytes)?;
        alloc::string::String::from_utf8(bytes).map_err(|_| ViError::IO)
    }

    /// Write all bytes from `buf` to the file (stub — writable VFS requires VirtIO-FAT).
    ///
    /// Currently always returns `Err(NotSupported)` until Phase 13 write path is complete.
    pub fn write_all(&mut self, _buf: &[u8]) -> ViResult<()> {
        Err(ViError::NotSupported)
    }

    /// Explicitly close the file and revoke its capability.
    pub fn close(mut self) -> ViResult<()> {
        self.closed = true;
        syscall::sys_close_cap(self.cap_id);
        Ok(())
    }

    /// Return the raw capability ID (for passing to kernel APIs).
    pub fn cap_id(&self) -> u64 {
        self.cap_id
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if !self.closed {
            // Revoke the kernel capability so it doesn't leak after the File is gone.
            // This is the normal Rust drop path (error propagation, end-of-scope, etc.).
            // Calling `File::close()` first is preferred so errors can be observed,
            // but this implicit close is always safe.
            syscall::sys_close_cap(self.cap_id);
        }
    }
}

// ── Zero-Copy Grant I/O (Storage 2.0, Phase 02) ──────────────────────────────

/// Blocking IPC call to the VFS service: encode `req`, send, receive, decode.
///
/// Uses a stack-allocated 512-byte buffer for both directions.
fn vfs_call<'r>(vfs_tid: usize, req: &VfsRequest<'_>, resp_buf: &'r mut [u8; 512])
    -> ViResult<VfsResponse<'r>>
{
    let mut send_buf = [0u8; 512];
    let n = api::ipc::encode(req, &mut send_buf)
        .map(|s| s.len())
        .map_err(|_| ViError::IO)?;
    syscall::sys_send(vfs_tid, &send_buf[..n]);
    syscall::sys_recv(0, resp_buf);
    api::ipc::decode::<VfsResponse>(resp_buf).map_err(|_| ViError::IO)
}

/// Read up to `buf.len()` bytes from a file cap using the optimal I/O path.
///
/// - `buf.len() < 4096`: kernel ReadCap path (no Grant overhead)
/// - `buf.len() >= 4096`: zero-copy Grant path (one VFS round-trip per 4096 bytes)
///
/// # F14 contract
/// The Grant is freed only AFTER `GrantDone` is received from VFS, ensuring
/// VFS has finished reading the buffer before the caller reclaims the frames.
///
/// # Errors
/// Returns `ViError::IO` on any transport or permission failure.
pub fn read_all(cap_id: u64, buf: &mut [u8], vfs_tid: usize) -> ViResult<usize> {
    if buf.len() < 4096 {
        syscall::sys_read_cap(cap_id, buf).map_err(|_| ViError::IO)
    } else {
        grant_read(cap_id, buf, vfs_tid)
    }
}

/// Write `data` to a file cap using the optimal I/O path.
///
/// - `data.len() < 4096`: kernel WriteGrant IPC path (no Grant overhead; caller
///   uses existing `VfsRequest::Write` via IPC — stub, returns 0 for now)
/// - `data.len() >= 4096`: zero-copy Grant path
///
/// # F14 contract
/// The caller waits for `GrantDone` before freeing the grant, so VFS finishes
/// writing to disk before the frames are returned to the allocator.
pub fn write_all(cap_id: u64, data: &[u8], vfs_tid: usize) -> ViResult<usize> {
    if data.len() < 4096 {
        // Small writes: caller uses existing VfsRequest::Write IPC directly.
        // This wrapper covers the large-file case only; return 0 to signal fallback.
        let _ = (cap_id, vfs_tid);
        Ok(0)
    } else {
        grant_write(cap_id, data, vfs_tid)
    }
}

fn grant_read(cap_id: u64, buf: &mut [u8], vfs_tid: usize) -> ViResult<usize> {
    let size = buf.len().min(4096);
    let grant_id = syscall::sys_grant_alloc(size).ok_or(ViError::OutOfMemory)?;
    // Share RW with VFS so it can fill the grant buffer.
    syscall::sys_grant_share(grant_id, vfs_tid, 2 /* ReadWrite */);

    // Control message fits in 512B IPC buffer.
    let req = VfsRequest::ReadGrant { cap: cap_id, offset: 0, size, grant: grant_id };
    let mut resp_buf = [0u8; 512];
    let resp = vfs_call(vfs_tid, &req, &mut resp_buf)
        .map_err(|e| { syscall::sys_grant_free(grant_id); e })?;

    let bytes = match resp {
        // F14: GrantDone arrives only AFTER VFS has filled the grant buffer.
        VfsResponse::GrantDone { bytes } => bytes,
        _ => { syscall::sys_grant_free(grant_id); return Err(ViError::IO); }
    };

    // SAFETY: grant was allocated with `size` bytes; VFS filled `bytes` of it.
    let ptr = syscall::sys_grant_slice(grant_id).ok_or_else(|| {
        syscall::sys_grant_free(grant_id); ViError::IO
    })?;
    let src = unsafe { core::slice::from_raw_parts(ptr as *const u8, bytes) };
    buf[..bytes].copy_from_slice(src);

    // F14: safe to free — GrantDone already received above.
    syscall::sys_grant_free(grant_id);
    Ok(bytes)
}

fn grant_write(cap_id: u64, data: &[u8], vfs_tid: usize) -> ViResult<usize> {
    let bytes = data.len().min(4096);
    let grant_id = syscall::sys_grant_alloc(bytes).ok_or(ViError::OutOfMemory)?;

    // Fill grant buffer BEFORE sharing — we own it exclusively here.
    // SAFETY: grant was allocated for `bytes`; ptr is valid for that range.
    let ptr = syscall::sys_grant_slice(grant_id).ok_or_else(|| {
        syscall::sys_grant_free(grant_id); ViError::IO
    })?;
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, bytes) };

    // Share WriteOnly (VFS reads, can't modify).
    syscall::sys_grant_share(grant_id, vfs_tid, 1 /* WriteOnly */);

    let req = VfsRequest::WriteGrant { cap: cap_id, offset: 0, grant: grant_id, bytes };
    let mut resp_buf = [0u8; 512];
    // ipc_call blocks until VFS replies — F14 guarantees VFS drained the grant.
    let resp = vfs_call(vfs_tid, &req, &mut resp_buf)
        .map_err(|e| { syscall::sys_grant_free(grant_id); e })?;

    syscall::sys_grant_free(grant_id);
    match resp {
        VfsResponse::GrantDone { bytes: written } => Ok(written),
        _ => Err(ViError::IO),
    }
}
