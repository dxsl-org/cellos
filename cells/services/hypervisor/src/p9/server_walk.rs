//! Tattach and Twalk: the only ways a fid comes to name a file.
//!
//! A walk moves one checked component at a time from where its fid is: a
//! component never holds a separator, and `..` at the root stays at the
//! root, so no walk names anything above the share root.

use alloc::string::String;
use alloc::vec::Vec;

use super::backend::{component, Backend};
use super::codec::{Qid, Reader, Writer};
use super::errno::{EBADF, EINVAL, EIO, ENOTDIR};
use super::fids::Fid;
use super::server::Server;

/// Most names in one Twalk.
const MAX_WALK: usize = 16;

impl<B: Backend> Server<B> {
    pub(super) fn attach(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, _afid) = (r.u32()?, r.u32()?);
        let (_uname, _aname, _n_uname) = (r.string()?, r.string()?, r.u32()?);
        if self.fids.contains(fid) {
            return Err(EBADF);
        }
        self.backend.attach().map_err(|_| EIO)?;
        self.fids.insert(fid, Fid::new(Vec::new(), true))?;
        w.qid(self.qids.qid(&[], true));
        Ok(())
    }

    pub(super) fn walk(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let (fid, newfid, count) = (r.u32()?, r.u32()?, usize::from(r.u16()?));
        if count > MAX_WALK {
            return Err(EINVAL);
        }
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            names.push(r.string()?);
        }
        let from = self.fids.get(fid)?;
        let (mut path, mut is_dir) = (from.path.clone(), from.is_dir);
        if newfid != fid && self.fids.contains(newfid) {
            return Err(EBADF);
        }
        let mut qids: Vec<Qid> = Vec::with_capacity(count);
        for name in names {
            match self.step(&mut path, is_dir, name) {
                Ok(dir) => is_dir = dir,
                Err(errno) if qids.is_empty() => return Err(errno),
                Err(_) => break,
            }
            qids.push(self.qids.qid(&path, is_dir));
        }
        if qids.len() == count {
            if let Some(old) = self.fids.remove(newfid) {
                self.release(old);
            }
            self.fids.insert(newfid, Fid::new(path, is_dir))?;
        }
        w.u16(qids.len() as u16);
        for qid in qids {
            w.qid(qid);
        }
        Ok(())
    }

    /// Walk `path` (a directory when `is_dir`) one name on; whether it then
    /// names a directory. `..` at the root stays at the root.
    fn step(&mut self, path: &mut Vec<String>, is_dir: bool, name: &[u8]) -> Result<bool, u32> {
        if !is_dir {
            return Err(ENOTDIR);
        }
        match name {
            b".." => {
                path.pop();
                return Ok(true);
            }
            b"." => return Ok(true),
            _ => {}
        }
        path.push(String::from(component(name)?));
        let stat = self.backend.stat(path);
        if stat.is_err() {
            path.pop();
        }
        Ok(stat?.is_dir)
    }
}
