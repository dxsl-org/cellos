//! What the 9P server needs from the files behind the share root.
//!
//! A path is the components below the root, the root itself empty. Every
//! component already passed [`component`]. Errors are Linux errno values.

use alloc::string::String;
use alloc::vec::Vec;

use super::errno::{EINVAL, ENAMETOOLONG};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

pub trait Backend {
    /// An open regular file; it stays valid until [`Backend::close`].
    type File;

    /// Make the root reachable; fails when no share root can be opened.
    fn attach(&mut self) -> Result<(), u32>;
    /// Whether names must be ones FAT can hold ([`fat_holds`]).
    fn fat_names(&self) -> bool;
    /// `ENOENT` when `path` does not exist.
    fn stat(&mut self, path: &[String]) -> Result<Stat, u32>;
    /// The entries of directory `dir` in a stable order, without `.`/`..`.
    fn list(&mut self, dir: &[String]) -> Result<Vec<Entry>, u32>;
    /// Make `path` an empty regular file, creating it when missing.
    fn truncate(&mut self, path: &[String]) -> Result<(), u32>;
    /// Open the existing regular file `path`.
    fn open(&mut self, path: &[String]) -> Result<Self::File, u32>;
    /// Create (or empty) `path` and open it.
    fn create(&mut self, path: &[String]) -> Result<Self::File, u32> {
        self.truncate(path)?;
        self.open(path)
    }
    fn close(&mut self, file: Self::File);
    fn size(&mut self, file: &Self::File) -> Result<u64, u32>;
    /// Bytes at `offset`, fewer only at end of file.
    fn read(&mut self, file: &Self::File, offset: u64, out: &mut [u8]) -> Result<usize, u32>;
    /// All of `data` at `offset`. Like FAT, a backend may put bytes meant past
    /// the end of the file at its end: the caller fills the gap first.
    fn write(&mut self, file: &Self::File, offset: u64, data: &[u8]) -> Result<(), u32>;
    /// The file's bytes reach the device.
    fn sync(&mut self, file: &Self::File) -> Result<(), u32>;
    fn mkdir(&mut self, path: &[String]) -> Result<(), u32>;
    /// Remove a file, or an empty directory when `is_dir`.
    fn remove(&mut self, path: &[String], is_dir: bool) -> Result<(), u32>;
    /// Move `old` to `new`, replacing a file at `new`.
    fn rename(&mut self, old: &[String], new: &[String]) -> Result<(), u32>;
}

/// `raw` as one directory component (the VFS's own rule: UTF-8, 1-255 bytes,
/// no separator, control byte, `.` or `..`).
pub fn component(raw: &[u8]) -> Result<&str, u32> {
    wire::dir_name::validate_dir_component(raw).map_err(|error| match error {
        wire::dir_name::DirNameError::TooLong => ENAMETOOLONG,
        _ => EINVAL,
    })
}

/// Bytes a FAT long name cannot hold.
const FAT_RESERVED: &[u8] = b"\"*/:<>?\\|";

/// Whether a FAT long name can be `name`: none of [`FAT_RESERVED`], and no
/// trailing dot or space (FAT drops them, so the stored name would differ).
pub fn fat_holds(name: &str) -> bool {
    !name.ends_with(['.', ' ']) && !name.bytes().any(|b| FAT_RESERVED.contains(&b))
}
