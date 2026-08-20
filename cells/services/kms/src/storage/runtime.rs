extern crate alloc;

use alloc::vec::Vec;

use super::backend::{StoreError, StoreIo};
use super::record::STORE_DIR;

pub(crate) struct VfsJournalStore {
    client: ostd::clients::vfs::VfsClient,
}

impl VfsJournalStore {
    pub(crate) fn new() -> Self {
        Self {
            client: ostd::clients::vfs::VfsClient::new(),
        }
    }
}

impl StoreIo for VfsJournalStore {
    fn ensure_dir(&mut self, path: &str) -> Result<(), StoreError> {
        if path == STORE_DIR {
            self.ensure_one("/srv/cellos")?;
        }
        self.ensure_one(path)
    }

    fn stat(&mut self, path: &str) -> Result<Option<u64>, StoreError> {
        match self.client.stat(path) {
            Ok((size, _)) => Ok(Some(size)),
            Err(ostd::ViError::NotFound) => Ok(None),
            Err(err) => Err(map_vfs_error(err)),
        }
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, StoreError> {
        match self.client.read_file_bounded(path, max_bytes) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(ostd::ViError::NotFound) => Ok(None),
            Err(err) => Err(map_vfs_error(err)),
        }
    }

    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), StoreError> {
        self.client.write_file(path, content).map_err(map_vfs_error)
    }

    fn unlink(&mut self, path: &str) -> Result<(), StoreError> {
        match self.client.unlink(path) {
            Ok(()) | Err(ostd::ViError::NotFound) => Ok(()),
            Err(err) => Err(map_vfs_error(err)),
        }
    }
}

impl VfsJournalStore {
    fn ensure_one(&mut self, path: &str) -> Result<(), StoreError> {
        if matches!(self.client.stat(path), Ok((_, true))) {
            return Ok(());
        }
        self.client.mkdir(path).map_err(map_vfs_error)
    }
}

fn map_vfs_error(err: ostd::ViError) -> StoreError {
    match err {
        ostd::ViError::PermissionDenied => StoreError::PermissionDenied,
        ostd::ViError::OutOfMemory => StoreError::TooLarge,
        _ => StoreError::Io,
    }
}
