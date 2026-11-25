//! One 9P2000.L session: request dispatch, version, clunk, statfs.
//!
//! Requests are served one at a time to completion, so Tflush has nothing to
//! cancel. A request that does not parse is `EINVAL`; one whose type this
//! server does not serve is `EOPNOTSUPP`.

use alloc::string::String;
use alloc::vec::Vec;

use super::backend::{component, fat_holds, Backend};
use super::codec::{header, lerror, Reader, Writer, HEADER_LEN, NOTAG};
use super::errno::*;
use super::fids::{Fid, FidTable, QidTable};
use super::{msg, MAX_MSIZE};

/// Smallest msize worth serving: a Twalk of a few long names.
const MIN_MSIZE: u32 = 1024;

pub struct Server<B: Backend> {
    pub(super) backend: B,
    pub(super) fids: FidTable<B::File>,
    pub(super) qids: QidTable,
    msize: u32,
    /// Bytes the reply to the current request may take.
    pub(super) room: usize,
}

impl<B: Backend> Server<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            fids: FidTable::new(),
            qids: QidTable::new(),
            msize: MAX_MSIZE,
            room: MAX_MSIZE as usize,
        }
    }

    /// The largest request accepted now.
    pub fn msize(&self) -> usize {
        self.msize as usize
    }

    #[cfg(test)]
    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Answer the request at the start of `msg` with a reply of at most
    /// `room` bytes (an Rlerror always fits the guest's smallest buffer).
    pub fn handle(&mut self, msg: &[u8], room: usize) -> Vec<u8> {
        let Some((size, kind, tag)) = header(msg) else {
            return lerror(NOTAG, EINVAL);
        };
        let size = size as usize;
        if size < HEADER_LEN || size > msg.len() || size > self.msize() {
            return lerror(tag, EINVAL);
        }
        self.room = room.min(self.msize());
        let mut r = Reader::new(&msg[HEADER_LEN..size]);
        let mut w = Writer::message(kind.wrapping_add(1), tag);
        match self.dispatch(kind, &mut r, &mut w) {
            Ok(()) if w.len() <= self.room => w.finish(),
            Ok(()) => lerror(tag, ERANGE),
            Err(errno) => lerror(tag, errno),
        }
    }

    fn dispatch(&mut self, kind: u8, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        match kind {
            msg::TVERSION => self.version(r, w),
            msg::TATTACH => self.attach(r, w),
            msg::TWALK => self.walk(r, w),
            msg::TCLUNK => {
                let fid = self.fids.remove(r.u32()?).ok_or(EBADF)?;
                self.release(fid);
                Ok(())
            }
            // Served requests are already answered: nothing to cancel.
            msg::TFLUSH => r.u16().map(drop),
            msg::TSTATFS => self.statfs(r, w),
            msg::TGETATTR => self.getattr(r, w),
            msg::TSETATTR => self.setattr(r, w),
            msg::TREADDIR => self.readdir(r, w),
            msg::TMKDIR => self.mkdir(r, w),
            msg::TUNLINKAT => self.unlinkat(r),
            msg::TRENAMEAT => self.renameat(r),
            msg::TRENAME => self.rename(r),
            msg::TLOPEN => self.lopen(r, w),
            msg::TLCREATE => self.lcreate(r, w),
            msg::TREAD => self.read(r, w),
            msg::TWRITE => self.write(r, w),
            msg::TFSYNC => self.fsync(r),
            // Locks are not kept (one guest, one share): granting every
            // request lets flock()/fcntl() users go on, as on a local file.
            msg::TLOCK => {
                self.fids.get(r.u32()?)?;
                w.u8(0); // LOCK_SUCCESS
                Ok(())
            }
            msg::TXATTRWALK
            | msg::TREADLINK
            | msg::TSYMLINK
            | msg::TMKNOD
            | msg::TLINK
            | msg::TGETLOCK => Err(EOPNOTSUPP),
            _ => Err(EOPNOTSUPP),
        }
    }

    /// Tversion ends every fid of the session.
    fn version(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        let msize = r.u32()?;
        let version = r.string()?;
        if msize < MIN_MSIZE {
            return Err(EINVAL);
        }
        self.reset();
        self.msize = msize.min(MAX_MSIZE);
        w.u32(self.msize);
        w.string(if version.starts_with(b"9P2000.L") {
            b"9P2000.L"
        } else {
            b"unknown"
        });
        Ok(())
    }

    /// Close every fid (guest reset or new session).
    pub fn reset(&mut self) {
        for fid in self.fids.drain() {
            self.release(fid);
        }
        self.msize = MAX_MSIZE;
    }

    fn statfs(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), u32> {
        self.fids.get(r.u32()?)?;
        w.u32(0x0102_1997); // V9FS_MAGIC
        w.u32(4096); // bsize
        for count in [1 << 20, 1 << 19, 1 << 19, 1 << 16, 1 << 15, 0] {
            w.u64(count); // blocks, bfree, bavail, files, ffree, fsid
        }
        w.u32(255); // namelen
        Ok(())
    }

    /// Give up `fid`'s file, syncing what was written through it first.
    pub(super) fn release(&mut self, fid: Fid<B::File>) {
        if let Some(file) = fid.file {
            if fid.dirty {
                let _ = self.backend.sync(&file);
            }
            self.backend.close(file);
        }
    }

    /// The directory fid `dfid`'s path with `raw` appended. A new name
    /// (`create`) must also be one the backend can hold.
    pub(super) fn child(&self, dfid: u32, raw: &[u8], create: bool) -> Result<Vec<String>, u32> {
        let dir = self.fids.get(dfid)?;
        if !dir.is_dir {
            return Err(ENOTDIR);
        }
        let name = component(raw)?;
        if create && self.backend.fat_names() && !fat_holds(name) {
            return Err(EINVAL);
        }
        let mut path = dir.path.clone();
        path.push(String::from(name));
        Ok(path)
    }
}
