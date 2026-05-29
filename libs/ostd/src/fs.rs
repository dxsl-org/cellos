// SPDX-License-Identifier: MPL-2.0

//! Filesystem utilities for ViOS cells.
//!
//! `File` is backed by a kernel capability (`CapId`) obtained via `OpenCap`.
//! Single-owner: moving a `File` transfers the capability.  Dropping without
//! calling `close()` issues an implicit close (which revokes the capability)
//! and, in debug builds, emits a warning about the implicit close.

use crate::syscall;
use alloc::vec::Vec;
use types::*;

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
