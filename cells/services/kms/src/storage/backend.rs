#![cfg_attr(not(test), allow(dead_code))]

extern crate alloc;

use alloc::vec::Vec;

#[cfg_attr(test, allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreError {
    Io,
    PermissionDenied,
    ReadbackMismatch,
    TooLarge,
}

pub(crate) trait StoreIo {
    fn ensure_dir(&mut self, path: &str) -> Result<(), StoreError>;
    fn stat(&mut self, path: &str) -> Result<Option<u64>, StoreError>;
    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, StoreError>;
    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), StoreError>;
}
