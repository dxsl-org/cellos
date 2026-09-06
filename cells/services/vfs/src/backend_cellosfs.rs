//! CellosFS Native backend for VFS.
//!
//! Replaces both RedoxFS and LittleFS with a unified, pure-Rust,
//! crash-resilient CoW Extent engine.

use alloc::vec::Vec;
use cellos_fs::CellosFs;
use ostd::prelude::Mutex;

use crate::backend::FsBackend;
use crate::disk_cellosfs::CellosPartitionDisk;

pub struct CellosFsBackend {
    prefix: &'static str,
    fs: Mutex<Option<CellosFs<CellosPartitionDisk>>>,
}


impl CellosFsBackend {
    /// Mount a CellosFS partition at `prefix`.
    /// Attempts `open`; if unformatted, automatically formats with `format`.
    /// Degrades gracefully to `None` if the underlying disk is absent.
    pub fn mount(prefix: &'static str, base_lba: u64, total_sectors: u64) -> Self {
        let total_blocks = total_sectors / 8;
        let disk = CellosPartitionDisk::new(base_lba, total_sectors);

        let fs = match CellosFs::open(disk) {
            Ok(fs) => {
                ostd::io::println("[vfs] CellosFS Native mounted at existing volume");
                Some(fs)
            }
            Err(_) => {
                // Try format
                let disk = CellosPartitionDisk::new(base_lba, total_sectors);
                match CellosFs::format(disk, total_blocks) {
                    Ok(fs) => {
                        ostd::io::println("[vfs] CellosFS Native formatted and mounted volume");
                        Some(fs)
                    }
                    Err(_) => {
                        ostd::io::println(
                            "[vfs] WARNING: CellosFS mount/format failed — volume unavailable",
                        );
                        None
                    }
                }
            }
        };

        Self {
            prefix,
            fs: Mutex::new(fs),
        }
    }

    fn with_fs<R>(
        &self,
        f: impl FnOnce(&mut CellosFs<CellosPartitionDisk>) -> Option<R>,
    ) -> Option<R> {
        let mut guard = self.fs.lock();
        guard.as_mut().and_then(f)
    }

    fn rel_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        let r = path.strip_prefix(self.prefix).unwrap_or(path);
        if r.split('/').any(|c| c == "..") {
            return None;
        }
        Some(r)
    }
}

impl FsBackend for CellosFsBackend {
    fn get_file_ptr(&self, _path: &str) -> Option<(usize, usize)> {
        None // Disk-backed filesystem
    }

    fn list(&self, path: &str, out: &mut [u8]) -> usize {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return 0,
        };

        self.with_fs(|fs| {
            let entries = fs.list_dir(rel).ok()?;
            let mut written = 0usize;
            for (name, is_dir, _) in entries {
                let tag = if is_dir { b"d:" } else { b"f:" };
                let line_len = 2 + name.len() + 1;
                if written + line_len > out.len() {
                    break;
                }
                out[written..written + 2].copy_from_slice(tag);
                written += 2;
                out[written..written + name.len()].copy_from_slice(name.as_bytes());
                written += name.len();
                out[written] = b'\n';
                written += 1;
            }
            Some(written)
        })
        .unwrap_or(0)
    }

    fn stat(&self, path: &str) -> Option<(u64, bool)> {
        let rel = self.rel_path(path)?;
        self.with_fs(|fs| {
            let inode = fs.lookup(rel).ok()?;
            Some((inode.size, inode.is_dir()))
        })
    }

    fn file_size(&self, path: &str) -> u64 {
        self.stat(path).map(|(s, _)| s).unwrap_or(0)
    }

    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return Vec::new(),
        };

        self.with_fs(|fs| {
            let inode = fs.lookup(rel).ok()?;
            if !inode.is_file() {
                return None;
            }
            let mut data = alloc::vec![0u8; inode.size as usize];
            let n = fs.read_file(rel, 0, &mut data).ok()?;
            data.truncate(n);
            Some(data)
        })
        .unwrap_or_default()
    }

    fn write(&mut self, path: &str, content: &[u8]) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            if fs.lookup(rel).is_err() {
                let _ = fs.create_file(rel);
            }
            fs.write_file(rel, 0, content).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return 0,
        };

        self.with_fs(|fs| fs.read_file(rel, offset, buf).ok())
            .unwrap_or(0)
    }

    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            if fs.lookup(rel).is_err() {
                let _ = fs.create_file(rel);
            }
            fs.write_file(rel, offset, content).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn sync(&mut self, _path: &str) -> bool {
        self.with_fs(|fs| fs.sync().ok().map(|_| true))
            .unwrap_or(false)
    }

    fn append(&mut self, path: &str, content: &[u8]) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            let offset = match fs.lookup(rel) {
                Ok(inode) => inode.size,
                Err(_) => {
                    let _ = fs.create_file(rel);
                    0
                }
            };
            fs.write_file(rel, offset, content).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn mkdir(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            fs.create_dir(rel).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn rmdir(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            fs.rmdir(rel).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn unlink(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            fs.unlink(rel).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }

    fn rmdir_recursive(&mut self, path: &str) -> bool {
        self.unlink(path)
    }

    fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        let old_rel = match self.rel_path(old) {
            Some(r) => r,
            None => return false,
        };
        let new_rel = match self.rel_path(new) {
            Some(r) => r,
            None => return false,
        };

        self.with_fs(|fs| {
            fs.rename(old_rel, new_rel).ok()?;
            let _ = fs.sync();
            Some(true)
        })
        .unwrap_or(false)
    }
}
