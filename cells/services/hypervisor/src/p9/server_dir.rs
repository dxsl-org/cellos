//! Directory requests: Treaddir, Tmkdir, Tunlinkat, Trenameat, Trename.

use alloc::string::String;
use alloc::vec::Vec;

use super::backend::Backend;
use super::codec::{Reader, Writer};
use super::errno::{EBUSY, EEXIST, EINVAL, EISDIR, ENOTDIR, ENOTEMPTY};
use super::server::Server;

/// Rreaddir's `count[4]` after the header.
const RREADDIR_HEAD: usize = 7 + 4;
const AT_REMOVEDIR: u32 = 0x200;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

impl<B: Backend> Server<B> {
    /// Entries from cookie `offset` on. Offset 0 takes a fresh snapshot of
    /// the directory; later offsets index that snapshot, so a listing read
    /// in several replies has every entry exactly once.
    pub(super) fn readdir(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, offset, count) = (r.u32()?, r.u64()?, r.u32()?);
        let limit = (count as usize).min(self.room.saturating_sub(RREADDIR_HEAD));
        let Self {
            backend,
            fids,
            qids,
            ..
        } = self;
        let entry = fids.get_mut(fid)?;
        if !entry.is_dir {
            return Err(ENOTDIR);
        }
        if offset == 0 || entry.listing.is_none() {
            entry.listing = Some(backend.list(&entry.path)?);
        }
        let listing = entry.listing.as_deref().unwrap_or_default();
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let mut data = Writer::message(0, 0);
        let mut path = entry.path.clone();
        for (index, item) in listing.iter().enumerate().skip(start) {
            // qid[13] offset[8] type[1] name[s]
            let len = 13 + 8 + 1 + 2 + item.name.len();
            if data.len() - 7 + len > limit {
                break;
            }
            path.push(item.name.clone());
            data.qid(qids.qid(&path, item.is_dir));
            path.pop();
            data.u64(index as u64 + 1);
            data.u8(if item.is_dir { DT_DIR } else { DT_REG });
            data.string(item.name.as_bytes());
        }
        let data = data.finish();
        w.u32((data.len() - 7) as u32);
        w.bytes(&data[7..]);
        Ok(())
    }

    pub(super) fn mkdir(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (dfid, name) = (r.u32()?, r.string()?);
        let (_mode, _gid) = (r.u32()?, r.u32()?);
        let path = self.child(dfid, name, true)?;
        if self.backend.stat(&path).is_ok() {
            return Err(EEXIST);
        }
        self.backend.mkdir(&path)?;
        w.qid(self.qids.qid(&path, true));
        Ok(())
    }

    pub(super) fn unlinkat(&mut self, r: &mut Reader) -> Result<(), u32> {
        let (dfid, name, flags) = (r.u32()?, r.string()?, r.u32()?);
        let path = self.child(dfid, name, false)?;
        let stat = self.backend.stat(&path)?;
        if flags & AT_REMOVEDIR != 0 {
            if !stat.is_dir {
                return Err(ENOTDIR);
            }
            if !self.backend.list(&path)?.is_empty() {
                return Err(ENOTEMPTY);
            }
        } else if stat.is_dir {
            return Err(EISDIR);
        }
        self.backend.remove(&path, stat.is_dir)?;
        self.qids.forget(&path);
        Ok(())
    }

    pub(super) fn renameat(&mut self, r: &mut Reader) -> Result<(), u32> {
        let (old_dfid, old_name) = (r.u32()?, r.string()?);
        let (new_dfid, new_name) = (r.u32()?, r.string()?);
        let old = self.child(old_dfid, old_name, false)?;
        let new = self.child(new_dfid, new_name, true)?;
        self.rename_path(&old, &new)
    }

    /// Legacy Trename: the fid names what moves.
    pub(super) fn rename(&mut self, r: &mut Reader) -> Result<(), u32> {
        let (fid, dfid, name) = (r.u32()?, r.u32()?, r.string()?);
        let old: Vec<String> = self.fids.get(fid)?.path.clone();
        if old.is_empty() {
            return Err(EBUSY);
        }
        let new = self.child(dfid, name, true)?;
        self.rename_path(&old, &new)
    }

    fn rename_path(&mut self, old: &[String], new: &[String]) -> Result<(), u32> {
        self.backend.stat(old)?;
        if old == new {
            return Ok(());
        }
        if new.starts_with(old) {
            return Err(EINVAL);
        }
        self.backend.rename(old, new)?;
        self.qids.moved(old, new);
        self.reopen_under(old, new);
        self.flush_after_rename(new);
        Ok(())
    }

    /// Put a rename on the device. A download ends in one (Chromium renames
    /// its `.crdownload`), after the file's last sync at clunk, and nothing
    /// else would flush the directory change before the machine stops. The
    /// VFS flushes a whole device through any file's handle, so a moved
    /// directory is flushed through nothing: best effort, errors ignored.
    fn flush_after_rename(&mut self, new: &[String]) {
        if !matches!(self.backend.stat(new), Ok(stat) if !stat.is_dir) {
            return;
        }
        if let Ok(file) = self.backend.open(new) {
            let _ = self.backend.sync(&file);
            self.backend.close(file);
        }
    }
}
