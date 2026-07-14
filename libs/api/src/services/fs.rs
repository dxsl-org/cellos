// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Filesystem API traits.

#![allow(unsafe_code)] // Allow unsafe for buffer slicing in async shim

use crate::*;
use alloc::boxed::Box;
use core::future::Future;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
// use types::*;

/// Type alias for boxed futures.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Result type for async file operations that take ownership of the file handle.
pub type FileResult<T> = (Box<dyn ViFile + Send + Sync>, ViResult<T>);

/// Metadata about a file or directory.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Stat {
    /// Total size in bytes (0 for directories).
    pub size: u64,
    /// True if this is a directory.
    pub is_dir: bool,
    /// True if the path exists.
    pub exists: bool,
    /// Padding to reach 16-byte size for IPC serialisation.
    pub _pad: [u8; 6],
}

/// Filesystem interface.
pub trait ViFileSystem: Send + Sync {
    /// Open a file at the given path.
    fn open(&self, path: &str, mode: OpenMode) -> ViResult<Box<dyn ViFile + Send + Sync>>;

    /// Create a directory (and any missing parent components).
    fn mkdir(&self, path: &str) -> ViResult<()>;

    /// Remove a file.
    fn remove(&self, path: &str) -> ViResult<()>;

    /// Return metadata for `path`.  Returns `Err(NotFound)` if absent.
    fn stat(&self, path: &str) -> ViResult<Stat> {
        let _ = path;
        Err(ViError::NotSupported)
    }

    /// Remove an empty directory.
    fn rmdir(&self, path: &str) -> ViResult<()> {
        let _ = path;
        Err(ViError::NotSupported)
    }

    /// List directory entries at `path`.
    ///
    /// Returns `Err(NotADirectory)` if path exists but is a file.
    /// Returns `Err(NotFound)` if path does not exist.
    /// Returns `Err(NotSupported)` if the filesystem does not implement directory listing.
    fn readdir(&self, path: &str) -> ViResult<alloc::vec::Vec<DirEntry>> {
        let _ = path;
        Err(ViError::NotSupported)
    }
}

/// File interface.
pub trait ViFile: Send + Sync {
    /// Read data into buffer.
    fn read(&mut self, buf: &mut [u8]) -> ViResult<usize>;

    /// Write data from buffer.
    fn write(&mut self, buf: &[u8]) -> ViResult<usize>;

    /// Seek to position.
    fn seek(&mut self, pos: SeekFrom) -> ViResult<u64>;

    /// Check if this is a directory.
    fn is_dir(&self) -> bool {
        false
    }

    /// Read next directory entry.
    /// Returns Ok(None) if end of directory.
    fn read_dir(&mut self) -> ViResult<Option<DirEntry>> {
        Err(ViError::NotSupported)
    }

    /// Return the file's current size in bytes without changing the cursor position.
    ///
    /// The default saves the current position, seeks to EOF, then restores.
    /// Override in stateless implementations (e.g. `FatFile`) to avoid the extra seek.
    /// Returns `ViError::IsADirectory` for directories.
    fn size(&mut self) -> ViResult<u64> {
        let cur = self.seek(SeekFrom::Current(0))?;
        let end = self.seek(SeekFrom::End(0))?;
        self.seek(SeekFrom::Start(cur))?;
        Ok(end)
    }

    /// Truncate the file to exactly `len` bytes.
    ///
    /// Returns `ViError::InvalidArgument` if `len > current_size`; use `write` to
    /// extend.  Returns `ViError::NotSupported` if the backend does not implement
    /// truncation.
    fn truncate(&mut self, _len: u64) -> ViResult<()> {
        Err(ViError::NotSupported)
    }

    /// Flush all dirty pages to the underlying block device (fsync).
    ///
    /// No-op on write-through implementations; wires into device flush on NVMe (G2).
    fn sync(&mut self) -> ViResult<()> {
        Ok(())
    }

    // --- Async Methods (Rule 7 & 8) ---

    /// Async Read: Takes ownership of the file handle and returns a Future.
    ///
    /// Implementations MUST return the file handle back in the result tuple.
    ///
    /// The buffer is passed as raw pointer/len because the Future is 'static and we cannot
    /// easily bind the lifetime of a user-space slice to it safely without complex logic.
    /// The caller (Kernel) ensures safety.
    fn read_async(
        self: Box<Self>,
        buf_ptr: usize,
        buf_len: usize,
    ) -> BoxFuture<'static, FileResult<usize>>;
}

/// A handle to an open file.
///
/// Wraps the low-level `ViFile` trait object and ensures usage of Drop
/// for resource cleanup.
pub struct FileHandle {
    pub file: Option<Box<dyn ViFile + Send + Sync>>,
}

impl FileHandle {
    pub fn new(file: Box<dyn ViFile + Send + Sync>) -> Self {
        Self { file: Some(file) }
    }

    pub fn into_inner(mut self) -> Box<dyn ViFile + Send + Sync> {
        self.file.take().expect("FileHandle already consumed")
    }
}

impl Deref for FileHandle {
    type Target = dyn ViFile + Send + Sync;

    fn deref(&self) -> &Self::Target {
        // Double deref: &Box<T> -> &T
        &**self.file.as_ref().expect("FileHandle use after consume")
    }
}

// ─── Capability-based file handle ────────────────────────────────────────────

/// A capability-scoped handle to an open file.
///
/// Wraps a kernel-assigned [`CapId`].  The capability is single-owner: moving
/// `ViFileHandle` transfers ownership; `close` revokes the capability.  Dropping
/// without calling `close` is safe but leaks the kernel resource until the cell exits.
///
/// Operations are synchronous in the current implementation.  Future versions
/// may offer async variants once the async executor is extended.
#[must_use = "dropping a ViFileHandle without calling close() leaks the kernel capability"]
pub struct ViFileHandle(crate::cap::CapId);

impl ViFileHandle {
    /// Create from a raw capability ID returned by the kernel.
    pub fn from_cap(id: crate::cap::CapId) -> Self {
        Self(id)
    }

    /// Return the underlying capability ID.
    pub fn cap_id(&self) -> crate::cap::CapId {
        self.0
    }
}

impl DerefMut for FileHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Double deref: &mut Box<T> -> &mut T
        &mut **self.file.as_mut().expect("FileHandle use after consume")
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        if let Some(_file) = self.file.take() {
            // Resource reclamation happens here.
            // _file drop will be called.
        }
    }
}

/// A file lease for exclusive access or locking.
///
/// Ensures the lease is released (revoked) when dropped.
pub struct Lease {
    pub id: u64,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // In the future, this would notify the Lock Manager to release the lease.
    }
}

/// File open mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum OpenMode {
    Read,
    Write,
    ReadWrite,
}

/// Seek position.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}
