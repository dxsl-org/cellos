//! Filesystem Subsystem

pub mod fat;

use crate::sync::Spinlock;
use alloc::boxed::Box;
use alloc::vec::Vec;
use api::fs::{OpenMode, ViFileSystem};
use types::{ViError, ViResult};

/// Global Filesystem Instance (viFS1)
pub static VIFS1: Spinlock<Option<Box<dyn ViFileSystem>>> = Spinlock::new(None);

/// Read a complete file from the embedded FAT filesystem into a heap buffer.
///
/// Path components are uppercased to match FAT16's all-caps storage convention
/// (e.g. `/bin/vfs` → opened as `/BIN/VFS`).  Returns `ViError::NotFound`
/// when VIFS1 is not mounted or the path does not exist.
pub fn read_file_from_vifs1(path: &str) -> ViResult<Box<[u8]>> {
    // Build an uppercase copy of the path: FAT16 names are uppercase.
    let mut upper = Vec::with_capacity(path.len());
    for b in path.bytes() {
        upper.push(b.to_ascii_uppercase());
    }
    let upper_path = core::str::from_utf8(&upper).map_err(|_| ViError::InvalidInput)?;

    let mut file = {
        let guard = VIFS1.lock();
        let fs = guard.as_ref().ok_or(ViError::NotFound)?;
        fs.open(upper_path, OpenMode::Read)?
    };

    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(ViError::NotFound) => break, // EOF sentinel on some FAT impls
            Err(e) => return Err(e),
        }
    }
    if buf.is_empty() {
        return Err(ViError::NotFound);
    }
    Ok(buf.into_boxed_slice())
}

pub fn init() {
    log::info!("Filesystem: Initializing...");

    // Attempt to mount the embedded FAT filesystem (FAT16) from the RAM disk.
    match fat::ViFatFS::new() {
        Ok(fs) => {
            log::info!("Filesystem: FAT16 mounted successfully.");
            *VIFS1.lock() = Some(Box::new(fs));
        }
        Err(e) => {
            log::error!("Filesystem: Failed to mount FAT: {:?}", e);
        }
    }
}
