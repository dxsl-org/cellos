//! The FAT rename over an in-memory volume, with just enough of the rest of a
//! backend to create and inspect entries through the dispatcher.

use fatfs::Write as _;

use crate::backend::{FsBackend, ListPage};
use crate::backend_fat::rename::tests::MemFs;
use crate::backend_fat::{read_to_end, replace_file};
use crate::fs_error::FsError;

pub(super) struct MemFat {
    pub(super) prefix: &'static str,
    pub(super) fs: MemFs,
}

impl MemFat {
    fn rel<'a>(&self, path: &'a str) -> Result<&'a str, FsError> {
        path.strip_prefix(self.prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|rest| !rest.is_empty())
            .ok_or(FsError::InvalidPath)
    }
}

impl FsBackend for MemFat {
    fn list(&self, _: &str, _: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::Unsupported)
    }
    fn list_page(&self, _: &str, _: u32, _: &mut [u8]) -> Result<ListPage, FsError> {
        Err(FsError::Unsupported)
    }
    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        let Ok(rel) = self.rel(path) else {
            return Ok(Some((0, true)));
        };
        let root = self.fs.root_dir();
        if root.open_dir(rel).is_ok() {
            return Ok(Some((0, true)));
        }
        Ok(root
            .open_file(rel)
            .ok()
            .map(|mut file| (read_to_end(&mut file).map_or(0, |b| b.len() as u64), false)))
    }
    fn file_size(&self, path: &str) -> Result<u64, FsError> {
        match self.stat(path)? {
            Some((size, false)) => Ok(size),
            Some(_) => Err(FsError::IsADirectory),
            None => Err(FsError::NotFound),
        }
    }
    fn read_to_vec(&self, path: &str) -> Result<alloc::vec::Vec<u8>, FsError> {
        let rel = self.rel(path)?;
        let mut file = self
            .fs
            .root_dir()
            .open_file(rel)
            .map_err(|_| FsError::NotFound)?;
        read_to_end(&mut file)
    }
    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let rel = self.rel(path)?;
        let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let root = self.fs.root_dir();
        let dir = if parent.is_empty() {
            root
        } else {
            root.open_dir(parent).map_err(|_| FsError::NotFound)?
        };
        let mut file = replace_file(&dir, name)?;
        file.write_all(content).map_err(|_| FsError::Io)
    }
    fn append(&mut self, _: &str, _: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let rel = self.rel(path)?;
        self.fs
            .root_dir()
            .create_dir(rel)
            .map(|_| ())
            .map_err(|_| FsError::Io)
    }
    fn rmdir(&mut self, _: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn unlink(&mut self, _: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn rmdir_recursive(&mut self, _: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn rename_replace(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        crate::backend_fat::rename::rename(&self.fs, self.rel(old)?, self.rel(new)?)
    }
}
