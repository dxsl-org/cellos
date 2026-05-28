// SPDX-License-Identifier: MPL-2.0

use crate::*;
use alloc::vec::Vec;

pub struct ReadDir {
    fd: usize,
}

impl Iterator for ReadDir {
    type Item = DirEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let mut entry = DirEntry::default();
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

/// A reference to an open file on the filesystem.
pub struct File {
    fd: usize,
}

impl File {
    /// Open a file in read-only mode.
    pub fn open(path: &str) -> ViResult<Self> {
        let fd = syscall::sys_open(path).map_err(|_| ViError::NotFound)?;
        Ok(Self { fd })
    }

    /// Read all bytes until EOF into the specified vector.
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> ViResult<usize> {
        let mut temp = [0u8; 512];
        let mut total = 0;
        loop {
            match syscall::sys_read(self.fd, &mut temp) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    buf.extend_from_slice(&temp[..n]);
                    total += n;
                }
                Err(_) => return Err(ViError::IO),
            }
        }
        Ok(total)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = syscall::sys_close(self.fd);
    }
}
