//! File requests: Tlopen, Tlcreate, Tread, Twrite, Tfsync.
//!
//! What was written through a fid is synced when the fid is fsynced or
//! clunked, so a file the guest has closed is on the device.

use alloc::vec;

use super::backend::Backend;
use super::codec::{Reader, Writer};
use super::errno::{EBADF, EEXIST, EINVAL, EISDIR};
use super::server::Server;
use super::server_attr::zero_fill;

/// Rread's `count[4]` after the header.
const RREAD_HEAD: usize = 7 + 4;
const O_ACCMODE: u32 = 0o3;
const O_TRUNC: u32 = 0o1000;

impl<B: Backend> Server<B> {
    pub(super) fn lopen(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, flags) = (r.u32()?, r.u32()?);
        let entry = self.fids.get(fid)?;
        if entry.opened {
            return Err(EINVAL);
        }
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let truncate = !is_dir && flags & O_TRUNC != 0 && flags & O_ACCMODE != 0;
        if truncate {
            self.truncate_path(&path)?;
        }
        let file = if is_dir {
            None
        } else {
            Some(self.backend.open(&path)?)
        };
        let entry = self.fids.get_mut(fid)?;
        entry.opened = true;
        entry.file = file;
        entry.dirty = truncate;
        w.qid(self.qids.qid(&path, is_dir));
        w.u32(0); // iounit: up to msize
        Ok(())
    }

    /// Create the file `name` in directory fid `fid`, which then names the
    /// new file, open.
    pub(super) fn lcreate(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, name) = (r.u32()?, r.string()?);
        let (_flags, _mode, _gid) = (r.u32()?, r.u32()?, r.u32()?);
        let path = self.child(fid, name, true)?;
        if self.fids.get(fid)?.opened {
            return Err(EINVAL);
        }
        if self.backend.stat(&path).is_ok() {
            return Err(EEXIST);
        }
        let file = self.backend.create(&path)?;
        let entry = self.fids.get_mut(fid)?;
        entry.path = path;
        entry.is_dir = false;
        entry.opened = true;
        entry.file = Some(file);
        entry.dirty = true;
        entry.listing = None;
        w.qid(self.qids.qid(&entry.path, false));
        w.u32(0);
        Ok(())
    }

    pub(super) fn read(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, offset, count) = (r.u32()?, r.u64()?, r.u32()?);
        let limit = (count as usize).min(self.room.saturating_sub(RREAD_HEAD));
        let entry = self.fids.get(fid)?;
        let file = open_file(entry.file.as_ref(), entry.is_dir)?;
        let size = self.backend.size(file)?;
        let len = size.saturating_sub(offset).min(limit as u64) as usize;
        let mut data = vec![0u8; len];
        let got = if len == 0 {
            0
        } else {
            self.backend.read(file, offset, &mut data)?
        };
        w.u32(got as u32);
        w.bytes(&data[..got]);
        Ok(())
    }

    /// FAT puts a write past the end of a file at its end, so the gap is
    /// written with zeros first.
    pub(super) fn write(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, offset, count) = (r.u32()?, r.u64()?, r.u32()?);
        let data = r.bytes(count as usize)?;
        let Self { backend, fids, .. } = self;
        let entry = fids.get_mut(fid)?;
        let file = open_file(entry.file.as_ref(), entry.is_dir)?;
        let size = backend.size(file)?;
        if offset > size {
            entry.dirty = true;
            zero_fill(backend, file, size, offset)?;
        }
        if !data.is_empty() {
            entry.dirty = true;
            backend.write(file, offset, data)?;
        }
        w.u32(count);
        Ok(())
    }

    pub(super) fn fsync(&mut self, r: &mut Reader) -> Result<(), u32> {
        let (fid, _datasync) = (r.u32()?, r.u32()?);
        let Self { backend, fids, .. } = self;
        let entry = fids.get_mut(fid)?;
        if let (Some(file), true) = (&entry.file, entry.dirty) {
            backend.sync(file)?;
            entry.dirty = false;
        }
        Ok(())
    }
}

/// The open file of a fid, if it has one.
fn open_file<F>(file: Option<&F>, is_dir: bool) -> Result<&F, u32> {
    match file {
        Some(file) => Ok(file),
        None if is_dir => Err(EISDIR),
        None => Err(EBADF),
    }
}
