use alloc::vec::Vec;

use crate::backend::FsBackend;

pub(crate) struct GuestDiskStub(pub(crate) u64);

impl FsBackend for GuestDiskStub {
    fn get_file_ptr(&self, _: &str) -> Option<(usize, usize)> {
        None
    }

    fn list(&self, _: &str, _: &mut [u8]) -> usize {
        0
    }

    fn stat(&self, _: &str) -> Option<(u64, bool)> {
        Some((self.0, false))
    }

    fn file_size(&self, _: &str) -> u64 {
        self.0
    }

    fn read_to_vec(&self, _: &str) -> Vec<u8> {
        Vec::new()
    }

    fn write(&mut self, _: &str, _: &[u8]) -> bool {
        false
    }

    fn append(&mut self, _: &str, _: &[u8]) -> bool {
        false
    }

    fn mkdir(&mut self, _: &str) -> bool {
        false
    }

    fn rmdir(&mut self, _: &str) -> bool {
        false
    }

    fn unlink(&mut self, _: &str) -> bool {
        false
    }

    fn rmdir_recursive(&mut self, _: &str) -> bool {
        false
    }
}
