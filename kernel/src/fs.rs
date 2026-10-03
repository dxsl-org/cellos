//! Filesystem Subsystem

pub mod fat;

use crate::sync::Spinlock;
use alloc::boxed::Box;
use alloc::vec::Vec;
use api::fs::{OpenMode, ViFileSystem};
use types::{ViError, ViResult};

/// Global BootFS / initramfs instance — the FAT16 `kernel_fs.img` baked into
/// the kernel binary. Solves the chicken-and-egg of loading the VFS service
/// binary before the VFS service exists; the VFS cell also proxies `/bin`
/// reads here via the FD syscalls (specs/09-vfs.md v0.5 §2).
///
/// Naming note: the spec term "viFS1" (a planned RedoxFS fork) was dropped
/// 2026-06-10 — this static is unrelated to it despite the shared name.
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
        fs.open(path, OpenMode::Read)
            .or_else(|_| fs.open(upper_path, OpenMode::Read))?
    };

    let size = usize::try_from(file.size()?).map_err(|_| ViError::InvalidInput)?;
    if size == 0 {
        return Err(ViError::NotFound);
    }
    let mut buf = Vec::new();
    buf.try_reserve_exact(size).map_err(|_| {
        log::error!("[fs] OOM: VIFS1 read of {} bytes for {:?}", size, path);
        ViError::OutOfMemory
    })?;
    buf.resize(size, 0);
    let mut read = 0usize;
    while read < size {
        let end = read.saturating_add(4096).min(size);
        let want = end - read;
        match file.read(&mut buf[read..end]) {
            Ok(0) => break,
            Ok(n) => {
                if n < want {
                    log::error!(
                        "[fs] {}: short FAT chunk at {}: {} of {} bytes",
                        path,
                        read,
                        n,
                        want
                    );
                }
                read += n;
            }
            Err(ViError::NotFound) => break, // EOF sentinel on some FAT impls
            Err(e) => return Err(e),
        }
    }
    if read == size {
        // Capacity == len, so the box conversion is a pointer move: no allocation.
        return Ok(buf.into_boxed_slice());
    }
    if read == 0 {
        return Err(ViError::NotFound);
    }
    // Short read: the FAT chain ended before the size the directory entry declares
    // (measured 2026-10-03: 78 760 of 78 824 bytes for `/bin/bench-probe`). Keep the
    // tolerant behaviour, but never let `Vec::into_boxed_slice` do the shrink: it
    // reallocates when `capacity != len`, that allocation is infallible, and on a
    // full kernel heap it reaches the alloc error handler, which halts the kernel —
    // that is what killed the capacity sweep at 204 cells instead of returning a
    // typed `OutOfMemory` to the spawner.
    log::error!("[fs] {}: short read {} of {} bytes", path, read, size);
    let mut exact = Vec::new();
    exact.try_reserve_exact(read).map_err(|_| {
        log::error!(
            "[fs] OOM: short-read copy of {} bytes for {:?}",
            read,
            path
        );
        ViError::OutOfMemory
    })?;
    exact.extend_from_slice(&buf[..read]);
    Ok(exact.into_boxed_slice())
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
