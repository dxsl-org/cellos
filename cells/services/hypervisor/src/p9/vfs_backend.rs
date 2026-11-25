//! [`Backend`] over the VFS share root ([`VfsShare`]).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use abi::dir_handles::ViDirHandle;
use wire::ipc::vfs_dir_page::{self, EntryKind};
use wire::ipc::{VfsRequest, VfsResponse};

use super::backend::{Backend, Entry, Stat};
use super::errno::{EIO, ENOENT};
use super::vfs_io::{VfsFile, DIR_TICKS};
use super::vfs_share::{refusal, VfsShare};

/// Most entries one listing returns.
const MAX_ENTRIES: usize = 4096;

impl VfsShare {
    /// Every page of `dir`'s listing; mounts count as directories, entries
    /// that are neither file nor directory are left out.
    fn list_handle(&mut self, dir: ViDirHandle) -> Result<Vec<Entry>, u32> {
        let mut listing = Vec::new();
        let mut cursor = 0;
        while listing.len() < MAX_ENTRIES {
            let request = VfsRequest::ListAtPage { dir, cursor };
            let (next, flags, entries) = match self.call(&request, DIR_TICKS)? {
                VfsResponse::DirPage {
                    next,
                    flags,
                    entries,
                } => (next, flags, entries),
                other => return Err(refusal(&other, EIO)),
            };
            for entry in vfs_dir_page::entries(entries) {
                let entry = entry.map_err(|_| EIO)?;
                let is_dir = match entry.kind {
                    EntryKind::File => false,
                    EntryKind::Dir | EntryKind::Mount => true,
                    EntryKind::Other => continue,
                };
                listing.push(Entry {
                    name: entry.name.to_string(),
                    is_dir,
                });
            }
            if flags & vfs_dir_page::COMPLETE != 0 {
                break;
            }
            cursor = next;
        }
        listing.truncate(MAX_ENTRIES);
        Ok(listing)
    }

    fn rename_at(&mut self, old: &[String], new: &[String]) -> Result<(), u32> {
        let ((old_name, old_parent), (new_name, new_parent)) =
            match (old.split_last(), new.split_last()) {
                (Some(old), Some(new)) => (old, new),
                _ => return Err(EIO),
            };
        let (old_dir, old_first) = self.open_dir(old_parent)?;
        let opened = self.open_dir(new_parent);
        let result = opened.and_then(|(new_dir, _)| {
            self.done(
                &VfsRequest::RenameAt {
                    old_dir,
                    old: old_name,
                    new_dir,
                    new: new_name,
                },
                EIO,
            )
        });
        if let Ok((_, new_first)) = opened {
            self.close_dir(new_first);
        }
        self.close_dir(old_first);
        result
    }
}

impl Backend for VfsShare {
    type File = VfsFile;

    fn attach(&mut self) -> Result<(), u32> {
        self.attach_root()
    }

    fn fat_names(&self) -> bool {
        self.fat()
    }

    fn stat(&mut self, path: &[String]) -> Result<Stat, u32> {
        if path.is_empty() {
            return Ok(Stat {
                is_dir: true,
                size: 0,
            });
        }
        self.in_parent(path, |s, dir, name| {
            match s.call(&VfsRequest::StatAt { dir, name }, DIR_TICKS)? {
                VfsResponse::Stat { size, is_dir } => Ok(Stat { is_dir, size }),
                other => Err(refusal(&other, ENOENT)),
            }
        })
    }

    fn list(&mut self, dir: &[String]) -> Result<Vec<Entry>, u32> {
        let (handle, first) = self.open_dir(dir)?;
        let listing = self.list_handle(handle);
        self.close_dir(first);
        listing
    }

    fn truncate(&mut self, path: &[String]) -> Result<(), u32> {
        self.in_parent(path, |s, dir, name| {
            s.done(
                &VfsRequest::WriteAt {
                    dir,
                    name,
                    content: &[],
                },
                EIO,
            )
        })
    }

    fn open(&mut self, path: &[String]) -> Result<VfsFile, u32> {
        self.open_file(path)
    }

    fn close(&mut self, file: VfsFile) {
        self.close_file(file);
    }

    fn size(&mut self, file: &VfsFile) -> Result<u64, u32> {
        self.file_size(file)
    }

    fn read(&mut self, file: &VfsFile, offset: u64, out: &mut [u8]) -> Result<usize, u32> {
        self.read_at(file, offset, out)
    }

    fn write(&mut self, file: &VfsFile, offset: u64, data: &[u8]) -> Result<(), u32> {
        self.write_at(file, offset, data)
    }

    fn sync(&mut self, file: &VfsFile) -> Result<(), u32> {
        self.sync_file(file)
    }

    fn mkdir(&mut self, path: &[String]) -> Result<(), u32> {
        self.in_parent(path, |s, dir, name| {
            s.done(&VfsRequest::MkdirAt { dir, name }, EIO)
        })
    }

    fn remove(&mut self, path: &[String], is_dir: bool) -> Result<(), u32> {
        self.in_parent(path, |s, dir, name| {
            if is_dir {
                s.done(&VfsRequest::RmdirAt { dir, name }, EIO)
            } else {
                s.done(&VfsRequest::UnlinkAt { dir, name }, EIO)
            }
        })
    }

    fn rename(&mut self, old: &[String], new: &[String]) -> Result<(), u32> {
        self.rename_at(old, new)
    }
}
