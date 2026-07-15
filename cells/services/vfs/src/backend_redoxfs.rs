//! RedoxFS backend for VFS `/srv` — CoW B-tree filesystem on MBR partition P5.
//!
//! `RedoxFsBackend` wraps a `FileSystem<VicellDisk>` behind a `Mutex` so that
//! both `&self` read operations and `&mut self` write operations share the same
//! open filesystem handle.  The filesystem is opened once on construction;
//! failure (e.g. unformatted partition) degrades gracefully to empty/false.
//!
//! Path convention: the VFS manager passes absolute VFS paths (e.g.
//! `/srv/foo/bar.txt`).  `rel_path()` strips the mount prefix so that all
//! RedoxFS tree operations start from `TreePtr::root()`.
//!
//! # Offline format
//! The P5 partition must be pre-formatted with `redoxfs-mkfs` before the first
//! boot.  `scripts/mksrv-img.sh` does this in CI (Phase 04).

use alloc::string::String;
use alloc::vec::Vec;

use ostd::prelude::Mutex;
use redoxfs::{DirEntry, FileSystem, Node, Transaction, TreePtr};

use crate::backend::FsBackend;
use crate::disk_redoxfs::VicellDisk;

pub struct RedoxFsBackend {
    prefix: &'static str,
    // Interior mutability: read and write operations both need &mut FileSystem.
    // Single-threaded cell — no real contention; Mutex is a ZST spin lock here.
    fs: Mutex<Option<FileSystem<VicellDisk>>>,
}

// SAFETY: VicellDisk is a ZST; FileSystem<VicellDisk> contains only alloc
// types (BTreeMap, Vec, Box<[u8]>) and Copy header data — all Send.
// The VFS cell is single-threaded: the Mutex is never actually contested.
unsafe impl Send for RedoxFsBackend {}

impl RedoxFsBackend {
    /// Open P5 and verify the RedoxFS superblock.  Logs a warning and stores
    /// `None` if the partition is blank or unformatted.
    pub fn mount(prefix: &'static str) -> Self {
        // block_opt=Some(0): header ring starts at block 0 of the partition.
        let fs = FileSystem::open(VicellDisk, None, Some(0), false)
            .map(Some)
            .unwrap_or_else(|_| {
                ostd::io::println("[vfs] WARNING: RedoxFS P5 open failed — /srv unavailable (format with redoxfs-mkfs)");
                None
            });
        if fs.is_some() {
            ostd::io::println("[vfs] RedoxFS /srv volume opened (P5)");
        }
        Self {
            prefix,
            fs: Mutex::new(fs),
        }
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Run a closure with mutable access to the open filesystem.
    /// Returns `None` if the filesystem was not available at mount time.
    fn with_fs<R>(&self, f: impl FnOnce(&mut FileSystem<VicellDisk>) -> Option<R>) -> Option<R> {
        let mut guard = self.fs.lock();
        guard.as_mut().and_then(f)
    }

    /// Strip the `/srv` prefix from `path`.
    /// Returns `None` if path contains `..` (security: prevent escape).
    fn rel_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        let r = path.strip_prefix(self.prefix).unwrap_or(path);
        if r.split('/').any(|c| c == "..") {
            return None;
        }
        Some(r)
    }

    /// Walk path string from root and return the node at that path.
    /// Empty path or `""` returns the root directory node.
    fn walk_to_node(
        tx: &mut Transaction<VicellDisk>,
        path: &str,
    ) -> Option<redoxfs::TreeData<Node>> {
        let mut ptr = TreePtr::<Node>::root();
        let parts: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();

        if parts.is_empty() {
            return tx.read_tree(ptr).ok();
        }

        let last_idx = parts.len() - 1;
        for (i, &component) in parts.iter().enumerate() {
            let node = tx.find_node(ptr, component).ok()?;
            if i == last_idx {
                return Some(node);
            }
            ptr = node.ptr();
        }
        unreachable!()
    }

    /// Return `(parent_dir_ptr, leaf_name)` for a path.
    /// Returns `None` for an empty path (cannot operate on root).
    fn parent_and_name<'a>(
        tx: &mut Transaction<VicellDisk>,
        path: &'a str,
    ) -> Option<(TreePtr<Node>, &'a str)> {
        let parts: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        if parts.is_empty() {
            return None;
        }

        let name = parts[parts.len() - 1];
        let mut ptr = TreePtr::<Node>::root();
        for &component in &parts[..parts.len() - 1] {
            let node = tx.find_node(ptr, component).ok()?;
            ptr = node.ptr();
        }
        Some((ptr, name))
    }

    /// Recursively remove a directory and all its contents.
    /// All mutations happen inside a single `fs.tx()` call from the caller.
    fn remove_recursive(tx: &mut Transaction<VicellDisk>, dir_ptr: TreePtr<Node>) -> bool {
        let mut entries: Vec<DirEntry> = Vec::new();
        if tx.child_nodes(dir_ptr, &mut entries).is_err() {
            return false;
        }
        // Collect names + ptrs before borrowing tx mutably again
        let children: Vec<(String, TreePtr<Node>, u16)> = entries
            .iter()
            .filter_map(|e| e.name().map(|n| (String::from(n), e.node_ptr(), 0u16)))
            .collect();

        for (name, child_ptr, _) in &children {
            // Re-read the node to get its current mode
            let child = match tx.read_tree(*child_ptr) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let mode = child.data().mode();
            if child.data().is_dir() && !Self::remove_recursive(tx, *child_ptr) {
                return false;
            }
            let _ = tx.remove_node(dir_ptr, name, mode);
        }
        true
    }
}

impl FsBackend for RedoxFsBackend {
    fn get_file_ptr(&self, _path: &str) -> Option<(usize, usize)> {
        None // disk-backed: no zero-copy pointer available
    }

    fn list(&self, path: &str, out: &mut [u8]) -> usize {
        let rel = match self.rel_path(path) {
            Some(r) => r,
            None => return 0,
        };
        self.with_fs(|fs| {
            let mut pos = 0usize;
            fs.tx(|tx| {
                let node = Self::walk_to_node(tx, rel).ok_or(redox_syscall::error::Error::new(
                    redox_syscall::error::ENOENT,
                ))?;
                if !node.data().is_dir() {
                    return Err(redox_syscall::error::Error::new(
                        redox_syscall::error::ENOTDIR,
                    ));
                }
                let mut entries: Vec<DirEntry> = Vec::new();
                tx.child_nodes(node.ptr(), &mut entries)?;
                for entry in &entries {
                    let name = match entry.name() {
                        Some(n) => n,
                        None => continue,
                    };
                    let child = match tx.find_node(node.ptr(), name) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let prefix: &[u8] = if child.data().is_dir() { b"d:" } else { b"f:" };
                    let nb = name.as_bytes();
                    let entry_len = 2 + nb.len() + 1; // "f:" + name + '\n'
                    if pos + entry_len > out.len() {
                        break;
                    }
                    out[pos..pos + 2].copy_from_slice(prefix);
                    out[pos + 2..pos + 2 + nb.len()].copy_from_slice(nb);
                    out[pos + 2 + nb.len()] = b'\n';
                    pos += entry_len;
                }
                Ok(())
            })
            .ok()?;
            Some(pos)
        })
        .unwrap_or(0)
    }

    fn stat(&self, path: &str) -> Option<(u64, bool)> {
        let rel = self.rel_path(path)?;
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let node = Self::walk_to_node(tx, rel).ok_or(redox_syscall::error::Error::new(
                    redox_syscall::error::ENOENT,
                ))?;
                Ok((node.data().size(), node.data().is_dir()))
            })
            .ok()
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
            let mut buf = Vec::new();
            fs.tx(|tx| {
                let node = Self::walk_to_node(tx, rel).ok_or(redox_syscall::error::Error::new(
                    redox_syscall::error::ENOENT,
                ))?;
                if node.data().is_dir() {
                    return Err(redox_syscall::error::Error::new(
                        redox_syscall::error::EISDIR,
                    ));
                }
                let size = node.data().size() as usize;
                buf = alloc::vec![0u8; size];
                let n = tx.read_node(node.ptr(), 0, &mut buf, 0, 0)?;
                buf.truncate(n);
                Ok(())
            })
            .ok()?;
            Some(buf)
        })
        .unwrap_or_default()
    }

    fn write(&mut self, path: &str, content: &[u8]) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;

                let node_ptr = match tx.find_node(parent_ptr, name) {
                    Ok(existing) => {
                        // Truncate to 0 before overwriting
                        tx.truncate_node(existing.ptr(), 0, 0, 0)?;
                        existing.ptr()
                    }
                    Err(_) => {
                        // Create new file
                        let node =
                            tx.create_node(parent_ptr, name, Node::MODE_FILE | 0o644, 0, 0)?;
                        node.ptr()
                    }
                };
                tx.write_node(node_ptr, 0, content, 0, 0)?;
                Ok(())
            })
            // Surface the errno instead of a silent bool — write failures on a
            // mounted volume indicate a tx/allocator problem worth diagnosing.
            .map_err(|e| {
                ostd::io::println(&alloc::format!(
                    "[vfs] redoxfs write '{rel}' failed: errno={}",
                    e.errno
                ));
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }

    fn append(&mut self, path: &str, content: &[u8]) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;

                let (node_ptr, offset) = match tx.find_node(parent_ptr, name) {
                    Ok(existing) => {
                        let off = existing.data().size();
                        (existing.ptr(), off)
                    }
                    Err(_) => {
                        let node =
                            tx.create_node(parent_ptr, name, Node::MODE_FILE | 0o644, 0, 0)?;
                        (node.ptr(), 0u64)
                    }
                };
                tx.write_node(node_ptr, offset, content, 0, 0)?;
                Ok(())
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }

    fn mkdir(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;
                tx.create_node(parent_ptr, name, Node::MODE_DIR | 0o755, 0, 0)?;
                Ok(())
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }

    fn rmdir(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;
                let node = tx.find_node(parent_ptr, name)?;
                tx.remove_node(parent_ptr, name, node.data().mode())?;
                Ok(())
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }

    fn unlink(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;
                let node = tx.find_node(parent_ptr, name)?;
                if node.data().is_dir() {
                    return Err(redox_syscall::error::Error::new(
                        redox_syscall::error::EISDIR,
                    ));
                }
                tx.remove_node(parent_ptr, name, node.data().mode())?;
                Ok(())
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }

    fn rmdir_recursive(&mut self, path: &str) -> bool {
        let rel = match self.rel_path(path) {
            Some(r) => String::from(r),
            None => return false,
        };
        self.with_fs(|fs| {
            fs.tx(|tx| {
                let (parent_ptr, name) = Self::parent_and_name(tx, &rel).ok_or(
                    redox_syscall::error::Error::new(redox_syscall::error::ENOENT),
                )?;
                let node = tx.find_node(parent_ptr, name)?;
                let dir_ptr = node.ptr();
                let mode = node.data().mode();
                // Remove all contents, then remove the directory entry itself
                if !Self::remove_recursive(tx, dir_ptr) {
                    return Err(redox_syscall::error::Error::new(redox_syscall::error::EIO));
                }
                tx.remove_node(parent_ptr, name, mode)?;
                Ok(())
            })
            .ok()?;
            Some(true)
        })
        .unwrap_or(false)
    }
}
