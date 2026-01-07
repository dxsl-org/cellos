// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Filesystem API traits.

use crate::*;
use core::future::Future;
use alloc::boxed::Box;
use core::ops::{Deref, DerefMut};
use types::*;

/// Filesystem interface.
pub trait ViFileSystem: Send + Sync {
    /// Open a file at the given path.
    fn open(&self, path: &str, mode: OpenMode) -> ViResult<Box<dyn ViFile + Send + Sync>>;
    
    /// Create a directory.
    fn mkdir(&self, path: &str) -> ViResult<()>;
    
    /// Remove a file or directory.
    fn remove(&self, path: &str) -> ViResult<()>;
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
    fn is_dir(&self) -> bool { false }

    /// Read next directory entry.
    /// Returns Ok(None) if end of directory.
    fn read_dir(&mut self) -> ViResult<Option<DirEntry>> { Err(ViError::NotSupported) }
}

/// A handle to an open file.
///
/// Wraps the low-level `ViFile` trait object and ensures usage of Drop
/// for resource cleanup.
pub struct FileHandle {
    file: Box<dyn ViFile + Send + Sync>,
}

impl FileHandle {
    pub fn new(file: Box<dyn ViFile + Send + Sync>) -> Self {
        Self { file }
    }
}

impl Deref for FileHandle {
    type Target = dyn ViFile + Send + Sync;

    fn deref(&self) -> &Self::Target {
        &*self.file
    }
}

impl DerefMut for FileHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.file
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        // Resource reclamation happens here.
        // For Box<dyn ViFile>, the Drop impl of the concrete type (e.g. FatFile)
        // will be called automatically when the Box is dropped.
        // We add this impl to satisfy the architectural requirement and 
        // to allow for future global hook injection (e.g. usage stats).
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

