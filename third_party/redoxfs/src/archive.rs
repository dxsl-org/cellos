use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::{Disk, FileSystem, Node, Transaction, TreePtr, BLOCK_SIZE};

/// Windows shim for the Unix `MetadataExt` accessors archive_at relies on.
///
/// Archiving on a non-Unix host only serves TEST-image seeding (ViCell's
/// scripts/mksrv-img.sh); the host filesystem has no POSIX ownership or mode
/// bits, so fixed defaults (root:root, 0755/0644) and mtime-derived
/// timestamps are used instead.
#[cfg(not(unix))]
trait MetadataExt {
    fn mode(&self) -> u32;
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
    fn ctime(&self) -> i64;
    fn ctime_nsec(&self) -> i64;
    fn mtime(&self) -> i64;
    fn mtime_nsec(&self) -> i64;
}

#[cfg(not(unix))]
impl MetadataExt for fs::Metadata {
    fn mode(&self) -> u32 {
        if self.is_dir() {
            0o755
        } else {
            0o644
        }
    }
    fn uid(&self) -> u32 {
        0
    }
    fn gid(&self) -> u32 {
        0
    }
    fn ctime(&self) -> i64 {
        self.mtime()
    }
    fn ctime_nsec(&self) -> i64 {
        self.mtime_nsec()
    }
    fn mtime(&self) -> i64 {
        self.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    fn mtime_nsec(&self) -> i64 {
        self.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.subsec_nanos() as i64)
            .unwrap_or(0)
    }
}

fn syscall_err(err: syscall::Error) -> io::Error {
    io::Error::from_raw_os_error(err.errno)
}

pub fn archive_at<D: Disk, P: AsRef<Path>>(
    tx: &mut Transaction<D>,
    parent_path: P,
    parent_ptr: TreePtr<Node>,
) -> io::Result<()> {
    for entry_res in fs::read_dir(parent_path)? {
        let entry = entry_res?;

        let metadata = entry.metadata()?;
        let file_type = metadata.file_type();

        let name = entry.file_name().into_string().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "filename is not valid UTF-8")
        })?;

        let mode_type = if file_type.is_dir() {
            Node::MODE_DIR
        } else if file_type.is_file() {
            Node::MODE_FILE
        } else if file_type.is_symlink() {
            Node::MODE_SYMLINK
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Does not support parsing {:?}", file_type),
            ));
        };

        let node_ptr;
        {
            let mode = mode_type | (metadata.mode() as u16 & Node::MODE_PERM);
            let mut node = tx
                .create_node(
                    parent_ptr,
                    &name,
                    mode,
                    metadata.ctime() as u64,
                    metadata.ctime_nsec() as u32,
                )
                .map_err(syscall_err)?;

            node_ptr = node.ptr();

            if node.data().uid() != metadata.uid() || node.data().gid() != metadata.gid() {
                node.data_mut().set_uid(metadata.uid());
                node.data_mut().set_gid(metadata.gid());
                tx.sync_tree(node).map_err(syscall_err)?;
            }
        }

        let path = entry.path();
        if file_type.is_dir() {
            archive_at(tx, path, node_ptr)?;
        } else if file_type.is_file() {
            let data = fs::read(path)?;
            let count = tx
                .write_node(
                    node_ptr,
                    0,
                    &data,
                    metadata.mtime() as u64,
                    metadata.mtime_nsec() as u32,
                )
                .map_err(syscall_err)?;
            if count != data.len() {
                panic!("file write count {} != {}", count, data.len());
            }
        } else if file_type.is_symlink() {
            let destination = fs::read_link(path)?;
            #[cfg(unix)]
            let data = destination.as_os_str().as_bytes();
            // Non-Unix hosts (test-image seeding only): symlink targets must
            // be UTF-8 — OsStr bytes are not exposed portably.
            #[cfg(not(unix))]
            let data = destination
                .to_str()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "symlink target is not UTF-8")
                })?
                .as_bytes();
            let count = tx
                .write_node(
                    node_ptr,
                    0,
                    data,
                    metadata.mtime() as u64,
                    metadata.mtime_nsec() as u32,
                )
                .map_err(syscall_err)?;
            if count != data.len() {
                panic!("symlink write count {} != {}", count, data.len());
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Does not support creating {:?}", file_type),
            ));
        }
    }

    Ok(())
}

pub fn archive<D: Disk, P: AsRef<Path>>(fs: &mut FileSystem<D>, parent_path: P) -> io::Result<u64> {
    let end_block = fs
        .tx(|tx| {
            // Archive_at root node
            archive_at(tx, parent_path, TreePtr::root())
                .map_err(|err| syscall::Error::new(err.raw_os_error().unwrap()))?;

            // Squash alloc log
            tx.sync(true)?;

            let end_block = tx.header.size() / BLOCK_SIZE;
            /* TODO: Cut off any free blocks at the end of the filesystem
            let mut end_changed = true;
            while end_changed {
                end_changed = false;

                let allocator = fs.allocator();
                let levels = allocator.levels();
                for level in 0..levels.len() {
                    let level_size = 1 << level;
                    for &block in levels[level].iter() {
                        if block < end_block && block + level_size >= end_block {
                            end_block = block;
                            end_changed = true;
                        }
                    }
                }
            }
            */

            // Update header
            tx.header.size = (end_block * BLOCK_SIZE).into();
            tx.header_changed = true;
            tx.sync(false)?;

            Ok(end_block)
        })
        .map_err(syscall_err)?;

    Ok((fs.block + end_block) * BLOCK_SIZE)
}
