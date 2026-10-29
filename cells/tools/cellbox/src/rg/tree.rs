//! [`VfsTree`]: `vigrep::walk::ViSearchTree` over the VFS streaming client
//! (`OpenRootDir`, `ListDirPage`, `OpenFileAt`, `ReadFileHandle`, `FileState`).
//!
//! Every directory is reopened by path (H1, D74-5). Files are opened through
//! one cached parent directory handle (D76-14), so consecutive siblings cost
//! one `OpenFileAt` each. `CloseDir` revokes every handle derived from the
//! directory, so the cached parent is evicted only while no file opened through
//! it is open; the walker never holds two files at once. Peak: one listing
//! directory, the cached parent and one file. Authority is exactly the caller's
//! VFS table: no BootFS fallback, no grant, no seal.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::vfs_dir_page::EntryKind as VfsKind;
use ostd::clients::vfs::VFS_DIR_PAGE_MAX;
use ostd::clients::VfsClient;
use api::vfs_file_handles::ViVfsFileHandle;
use ostd::prelude::ViError;
use vigrep::io;
use vigrep::walk::{DirEntry, EntryKind, FileState, Handle, PageInfo, ViSearchTree};

/// An open directory being listed.
pub struct VfsDir {
    handle: ViDirHandle,
    cursor: u32,
}

/// An open file.
pub struct VfsFile {
    handle: ViVfsFileHandle,
}

pub struct VfsTree {
    client: VfsClient,
    /// The cached parent directory of the last opened file.
    parent: Option<(Vec<u8>, ViDirHandle)>,
    /// Listing page buffer (one page always fits).
    page: Vec<u8>,
    /// Files open through `parent`.
    open_files: usize,
}

impl VfsTree {
    pub fn new() -> VfsTree {
        VfsTree { client: VfsClient::new(), parent: None, page: vec![0; VFS_DIR_PAGE_MAX], open_files: 0 }
    }

    /// The cached handle of `dir`, opening it (and evicting the previous one) on a miss.
    fn parent_handle(&mut self, dir: &[u8]) -> Result<ViDirHandle, io::Error> {
        if let Some((path, handle)) = &self.parent {
            if path.as_slice() == dir {
                return Ok(*handle);
            }
        }
        debug_assert_eq!(self.open_files, 0, "a cached parent is evicted while a file from it is open");
        self.evict_parent();
        let handle = self.client.open_root_dir(utf8(dir)?).map_err(vfs_error)?;
        self.parent = Some((dir.to_vec(), handle));
        Ok(handle)
    }

    fn evict_parent(&mut self) {
        if let Some((_, handle)) = self.parent.take() {
            let _ = self.client.close_dir(handle);
        }
    }
}

impl Drop for VfsTree {
    fn drop(&mut self) {
        self.evict_parent();
    }
}

fn utf8(path: &[u8]) -> Result<&str, io::Error> {
    core::str::from_utf8(path).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path is not UTF-8"))
}

/// A VFS answer on a held handle or a path request as a walk error. VFS answers
/// `ERR_IO` both for a missing path and for a real I/O failure, so the message
/// says both.
fn vfs_error(error: ViError) -> io::Error {
    match error {
        ViError::PermissionDenied => io::ErrorKind::PermissionDenied.into(),
        ViError::IO => io::Error::new(io::ErrorKind::NotFound, "not found or I/O error"),
        ViError::OutOfMemory => io::Error::other("VFS handle quota"),
        ViError::NotFound => io::Error::other("handle revoked"),
        other => io::Error::other(format!("VFS error {:?}", other)),
    }
}

/// `(parent, name)` of a canonical path with at least one component.
fn split(path: &[u8]) -> Option<(&[u8], &[u8])> {
    let slash = path.iter().rposition(|&byte| byte == b'/')?;
    let name = &path[slash + 1..];
    if name.is_empty() {
        return None;
    }
    Some((if slash == 0 { b"/" } else { &path[..slash] }, name))
}

impl ViSearchTree for VfsTree {
    type Dir = VfsDir;
    type File = VfsFile;

    fn open_dir(&mut self, path: &[u8]) -> Result<VfsDir, io::Error> {
        let text = utf8(path)?;
        match self.client.open_root_dir(text) {
            Ok(handle) => Ok(VfsDir { handle, cursor: 0 }),
            // Missing and not-a-directory are both ERR_IO: a path `stat` tells a
            // file (searched as an explicit file root) from anything else.
            Err(ViError::IO) => match self.client.stat(text) {
                Ok((_, false)) => Err(io::ErrorKind::NotADirectory.into()),
                _ => Err(vfs_error(ViError::IO)),
            },
            Err(error) => Err(vfs_error(error)),
        }
    }

    fn next_page(&mut self, dir: &mut VfsDir, entries: &mut Vec<DirEntry>) -> Result<PageInfo, io::Error> {
        let page = self
            .client
            .list_page(dir.handle, dir.cursor, &mut self.page)
            .map_err(vfs_error)?;
        entries.extend(page.entries().map(|entry| DirEntry {
            kind: match entry.kind {
                VfsKind::File => EntryKind::File,
                VfsKind::Dir => EntryKind::Dir,
                VfsKind::Mount => EntryKind::Mount,
                VfsKind::Other => EntryKind::Other,
            },
            name: entry.name.as_bytes().to_vec(),
        }));
        dir.cursor = page.next;
        Ok(PageInfo { complete: page.complete, changed: page.changed })
    }

    fn open_file(&mut self, path: &[u8]) -> Result<VfsFile, io::Error> {
        let (dir, name) = split(path).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file path"))?;
        let name = String::from(utf8(name)?);
        let parent = self.parent_handle(dir)?;
        let handle = self.client.open_file_at(parent, &name).map_err(vfs_error)?;
        self.open_files += 1;
        Ok(VfsFile { handle })
    }

    fn read_chunk(&mut self, file: &mut VfsFile, offset: u64, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.client.read_chunk(file.handle, offset, buf).map_err(vfs_error)
    }

    fn file_state(&mut self, file: &mut VfsFile) -> Result<FileState, io::Error> {
        let state = self.client.file_state(file.handle).map_err(vfs_error)?;
        Ok(FileState { size: state.size, modified: state.modified })
    }

    fn close(&mut self, handle: Handle<VfsDir, VfsFile>) {
        match handle {
            Handle::Dir(dir) => {
                let _ = self.client.close_dir(dir.handle);
            }
            Handle::File(file) => {
                let _ = self.client.close_file(file.handle);
                self.open_files = self.open_files.saturating_sub(1);
            }
        }
    }
}
