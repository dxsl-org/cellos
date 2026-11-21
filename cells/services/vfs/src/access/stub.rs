use alloc::vec::Vec;

use crate::backend::{FsBackend, ListPage};
use crate::fs_error::FsError;

pub(crate) struct GuestDiskStub(pub(crate) u64);

impl FsBackend for GuestDiskStub {
    fn list(&self, _: &str, _: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NotADirectory)
    }

    fn list_page(&self, _: &str, _: u32, _: &mut [u8]) -> Result<ListPage, FsError> {
        Err(FsError::NotADirectory)
    }

    fn stat(&self, _: &str) -> Result<Option<(u64, bool)>, FsError> {
        Ok(Some((self.0, false)))
    }

    fn file_size(&self, _: &str) -> Result<u64, FsError> {
        Ok(self.0)
    }

    fn read_to_vec(&self, _: &str) -> Result<Vec<u8>, FsError> {
        Ok(Vec::new())
    }

    fn write(&mut self, _: &str, _: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }

    fn append(&mut self, _: &str, _: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }

    fn mkdir(&mut self, _: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
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
}
