//! Tgetattr and Tsetattr, and the size changes FAT allows: empty a file, or
//! grow it with zeros.
//!
//! Attributes are fixed apart from kind and size: the guest checks
//! permissions itself from the mode, and every user of the share may read
//! and write everything in it. Owners, modes and times are not kept; setting
//! them succeeds and changes nothing.

use alloc::string::String;
use alloc::vec;

use super::backend::Backend;
use super::codec::{Reader, Writer};
use super::errno::{EFBIG, EISDIR, EOPNOTSUPP};
use super::server::Server;

/// Every field of P9_GETATTR_BASIC is answered.
const GETATTR_BASIC: u64 = 0x7ff;
const SETATTR_SIZE: u32 = 1 << 3;
const MODE_DIR: u32 = 0o040_777;
const MODE_FILE: u32 = 0o100_666;
/// Most zeros written to grow one file in one request: FAT stores a gap,
/// so a write far past the end costs every byte of it.
const ZERO_FILL_MAX: u64 = 64 << 20;
const ZERO_CHUNK: usize = 64 * 1024;

impl<B: Backend> Server<B> {
    pub(super) fn getattr(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, _mask) = (r.u32()?, r.u64()?);
        let entry = self.fids.get(fid)?;
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let size = match &entry.file {
            Some(file) => self.backend.size(file)?,
            None => self.backend.stat(&path)?.size,
        };
        w.u64(GETATTR_BASIC);
        w.qid(self.qids.qid(&path, is_dir));
        w.u32(if is_dir { MODE_DIR } else { MODE_FILE });
        w.u32(0); // uid
        w.u32(0); // gid
        w.u64(if is_dir { 2 } else { 1 }); // nlink
        w.u64(0); // rdev
        w.u64(size);
        w.u64(4096); // blksize
        w.u64(size.div_ceil(512)); // blocks
        for _ in 0..8 {
            w.u64(0); // atime, mtime, ctime, btime: seconds and nanoseconds
        }
        w.u64(0); // gen
        w.u64(0); // data_version
        Ok(())
    }

    pub(super) fn setattr(&mut self, r: &mut Reader, _w: &mut Writer) -> Result<(), u32> {
        let (fid, valid) = (r.u32()?, r.u32()?);
        let (_mode, _uid, _gid) = (r.u32()?, r.u32()?, r.u32()?);
        let size = r.u64()?;
        r.bytes(4 * 8)?; // atime and mtime, seconds and nanoseconds
        let entry = self.fids.get(fid)?;
        if valid & SETATTR_SIZE == 0 {
            return Ok(());
        }
        if entry.is_dir {
            return Err(EISDIR);
        }
        let path = entry.path.clone();
        let current = match &entry.file {
            Some(file) => self.backend.size(file)?,
            None => self.backend.stat(&path)?.size,
        };
        if size == current {
            return Ok(());
        }
        if size == 0 {
            return self.truncate_path(&path);
        }
        if size < current {
            return Err(EOPNOTSUPP);
        }
        let Self { backend, fids, .. } = self;
        let entry = fids.get_mut(fid)?;
        if let Some(file) = &entry.file {
            entry.dirty = true;
            return zero_fill(backend, file, current, size);
        }
        let file = backend.open(&path)?;
        let filled = zero_fill(backend, &file, current, size);
        let synced = filled.and_then(|()| backend.sync(&file));
        backend.close(file);
        synced
    }

    /// Empty the file at `path`, then reopen every fid's handle on it: the
    /// backend may have replaced the file.
    pub(super) fn truncate_path(&mut self, path: &[String]) -> Result<(), u32> {
        self.backend.truncate(path)?;
        self.reopen_under(path, path);
        Ok(())
    }

    /// Every fid at or below `old` now names the same file below `new`; open
    /// files are closed and opened again there.
    pub(super) fn reopen_under(&mut self, old: &[String], new: &[String]) {
        let Self { backend, fids, .. } = self;
        for entry in fids.iter_mut() {
            if !entry.path.starts_with(old) {
                continue;
            }
            let mut path = new.to_vec();
            path.extend_from_slice(&entry.path[old.len()..]);
            entry.path = path;
            if let Some(file) = entry.file.take() {
                if entry.dirty {
                    let _ = backend.sync(&file);
                }
                backend.close(file);
                entry.file = backend.open(&entry.path).ok();
            }
        }
    }
}

/// Write zeros over `from..to` of `file`.
pub(super) fn zero_fill<B: Backend>(
    backend: &mut B,
    file: &B::File,
    from: u64,
    to: u64,
) -> Result<(), u32> {
    if to - from > ZERO_FILL_MAX {
        return Err(EFBIG);
    }
    let zeros = vec![0u8; ZERO_CHUNK.min((to - from) as usize)];
    let mut at = from;
    while at < to {
        let len = (to - at).min(ZERO_CHUNK as u64) as usize;
        backend.write(file, at, &zeros[..len])?;
        at += len as u64;
    }
    Ok(())
}
