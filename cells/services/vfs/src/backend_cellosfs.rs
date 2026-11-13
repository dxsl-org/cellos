//! Native V1 filesystem backend. Observed sync is not crash-preserving acknowledgement.
use crate::backend::{fill_page, BackendHealth, FsBackend, ListPage};
use crate::blk_router::DeviceRef;
use crate::disk_cellosfs::CellosPartitionDisk;
use crate::fs_error::FsError as VfsFsError;
use alloc::{format, string::String, vec::Vec};
use cellos_fs::{disk::FsError, CellosFs, Expect};
use ostd::prelude::Mutex;
use wire::ipc::vfs_dir_page::EntryKind;
use wire::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome};

#[derive(Clone, Copy)]
enum Operation {
    Open,
    List,
    Stat,
    Read,
    Write,
    WriteAt,
    Append,
    Mkdir,
    Rmdir,
    Unlink,
    Rename,
    Sync,
    Stage,
    Publish,
}
#[derive(Clone, Copy)]
struct Failure {
    operation: Operation,
    error: FsError,
    path_tag: u64,
    sync_stage: bool,
}
fn path_tag(path: &str) -> u64 {
    // Diagnostic correlation only, never an authority identity.
    path.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
// reason: `Ready` is the steady state and each backend holds exactly one `NativeState`, so
// boxing the filesystem would add a heap indirection to every operation without saving memory.
#[allow(clippy::large_enum_variant)]
enum NativeState<D: cellos_fs::disk::BlockDevice> {
    Unavailable(Failure),
    Ready(CellosFs<D>),
    Poisoned(Failure),
}
pub struct CellosFsBackend<D: cellos_fs::disk::BlockDevice = CellosPartitionDisk> {
    prefix: &'static str,
    fs: Mutex<NativeState<D>>,
}
impl CellosFsBackend {
    pub fn mount(prefix: &'static str, dev: DeviceRef, base_lba: u64, total_sectors: u64) -> Self {
        let disk = CellosPartitionDisk::new(dev, base_lba, total_sectors);
        let opened =
            if !total_sectors.is_multiple_of(8) || base_lba.checked_add(total_sectors).is_none() {
                Err(FsError::PartitionOverflow)
            } else {
                CellosFs::open(disk)
            };
        let state = match opened.and_then(|mut fs| clear_staging(&mut fs).map(|_| fs)) {
            Ok(fs) => {
                ostd::io::println("[vfs] CellosFS Native mounted at existing volume");
                NativeState::Ready(fs)
            }
            Err(error) => {
                ostd::io::println("[vfs] WARNING: CellosFS open failed — volume unavailable");
                NativeState::Unavailable(Failure {
                    operation: Operation::Open,
                    error,
                    path_tag: path_tag(prefix),
                    sync_stage: false,
                })
            }
        };
        Self {
            prefix,
            fs: Mutex::new(state),
        }
    }
}

/// Directory, per CellosFS mount, that holds unpublished save transactions.
const STAGING_DIR: &str = "/.cbx-save";

/// Remove staging objects left by transactions that did not survive the last
/// mount. Nothing outside [`STAGING_DIR`] is touched.
pub(crate) fn clear_staging<D: cellos_fs::disk::BlockDevice>(
    fs: &mut CellosFs<D>,
) -> Result<(), FsError> {
    let entries = match fs.list_dir(STAGING_DIR) {
        Ok(entries) => entries,
        Err(FsError::NotFound) => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut removed = 0usize;
    for (name, directory, _) in entries {
        if !directory {
            fs.unlink(&format!("{STAGING_DIR}/{name}"))?;
            removed += 1;
        }
    }
    if removed != 0 {
        fs.sync()?;
        ostd::io::println(&format!("[vfs] CellosFS staging cleanup removed {removed}"));
    }
    Ok(())
}
/// Directory levels below the root of one recursive removal.
const RECURSIVE_MAX_DEPTH: usize = 32;
/// Entries, the root included, that one recursive removal may delete.
const RECURSIVE_MAX_ENTRIES: usize = 4096;

/// Append `dir`'s subtree to `out` children-first, ending with `dir` itself.
/// Returns `Ok(false)` when the tree exceeds a bound; `out` is then discarded.
fn collect_tree<D: cellos_fs::disk::BlockDevice>(
    fs: &mut CellosFs<D>,
    dir: &str,
    depth: usize,
    out: &mut Vec<(String, bool)>,
) -> Result<bool, FsError> {
    for (name, directory, _) in fs.list_dir(dir)? {
        let child = format!("{dir}/{name}");
        if directory {
            if depth == 0 || !collect_tree(fs, &child, depth - 1, out)? {
                return Ok(false);
            }
        } else {
            out.push((child, false));
        }
        if out.len() >= RECURSIVE_MAX_ENTRIES {
            return Ok(false);
        }
    }
    out.push((String::from(dir), true));
    Ok(true)
}

/// The VFS cause for an engine error. Storage faults (the medium failed or
/// holds state the engine cannot trust) are all `Io`; refusals keep their kind.
fn vfs_error(error: FsError) -> VfsFsError {
    match error {
        FsError::IoError | FsError::Corrupted | FsError::PartitionOverflow => VfsFsError::Io,
        FsError::NotFound => VfsFsError::NotFound,
        FsError::AlreadyExists => VfsFsError::AlreadyExists,
        FsError::NoSpace => VfsFsError::NoSpace,
        FsError::InvalidName | FsError::InvalidOffset => VfsFsError::InvalidPath,
        FsError::NotADirectory => VfsFsError::NotADirectory,
        FsError::IsADirectory => VfsFsError::IsADirectory,
        FsError::DirectoryNotEmpty => VfsFsError::DirectoryNotEmpty,
        // Only conditional publication compares state; elsewhere a mismatch is
        // a refusal nothing more specific describes.
        FsError::Conflict => VfsFsError::Io,
    }
}

impl<D: cellos_fs::disk::BlockDevice> CellosFsBackend<D> {
    fn rel_path<'a>(&self, path: &'a str) -> Result<&'a str, VfsFsError> {
        let rel = path
            .strip_prefix(self.prefix)
            .ok_or(VfsFsError::InvalidPath)?;
        if !rel.is_empty() && !rel.starts_with('/') || rel.split('/').any(|c| c == "..") {
            return Err(VfsFsError::InvalidPath);
        }
        Ok(rel)
    }
    /// Run `f` against the ready engine. A storage fault poisons the mount;
    /// a closed (unavailable or poisoned) mount is `Unavailable`.
    fn read<R>(
        &self,
        operation: Operation,
        path: &str,
        f: impl FnOnce(&mut CellosFs<D>) -> Result<R, FsError>,
    ) -> Result<R, VfsFsError> {
        let mut state = self.fs.lock();
        let result = match &mut *state {
            NativeState::Ready(fs) => f(fs),
            _ => return Err(VfsFsError::Unavailable),
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if error.is_storage_fault() {
                    *state = NativeState::Poisoned(Failure {
                        operation,
                        error,
                        path_tag: path_tag(path),
                        sync_stage: false,
                    });
                }
                Err(vfs_error(error))
            }
        }
    }
    /// Apply `f` and sync as one engine transaction. A refusal (NotFound, a
    /// full directory, NoSpace, …) is rolled back by the engine and leaves the
    /// mount serving; only a storage fault poisons it, because the medium may
    /// then hold a partial update.
    fn mutate(
        &self,
        operation: Operation,
        path: &str,
        f: impl FnOnce(&mut CellosFs<D>) -> Result<(), FsError>,
    ) -> Result<(), VfsFsError> {
        let mut state = self.fs.lock();
        let mut sync_stage = false;
        let result = match &mut *state {
            NativeState::Ready(fs) => fs.atomically(|fs| {
                f(fs)?;
                sync_stage = true;
                fs.sync()
            }),
            _ => return Err(VfsFsError::Unavailable),
        };
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                if error.is_storage_fault() {
                    *state = NativeState::Poisoned(Failure {
                        operation,
                        error,
                        path_tag: path_tag(path),
                        sync_stage,
                    });
                }
                Err(vfs_error(error))
            }
        }
    }
    fn ensure_file(fs: &mut CellosFs<D>, path: &str) -> Result<(), FsError> {
        match fs.lookup(path) {
            Ok(inode) if inode.is_file() => Ok(()),
            Ok(_) => Err(FsError::IsADirectory),
            Err(FsError::NotFound) => fs.create_file(path).map(|_| ()),
            Err(error) => Err(error),
        }
    }
    fn exact(count: usize, expected: usize) -> Result<(), FsError> {
        if count == expected {
            Ok(())
        } else {
            Err(FsError::Corrupted)
        }
    }
    /// A staging mutation: no sync, all-or-nothing, poisoned only on a
    /// storage-level error.
    fn stage(
        &self,
        path: &str,
        f: impl FnOnce(&mut CellosFs<D>) -> Result<(), FsError>,
    ) -> Result<(), VfsFsError> {
        self.read(Operation::Stage, path, |fs| fs.atomically(f))
    }
    /// `validate_write_range` refuses offsets and lengths past the engine's
    /// file-size bound before anything touches the disk.
    fn check_range(offset: u64, len: usize) -> Result<(), VfsFsError> {
        CellosFs::<D>::validate_write_range(offset, len)
            .map(|_| ())
            .map_err(vfs_error)
    }
}
impl<D: cellos_fs::disk::BlockDevice + Send> FsBackend for CellosFsBackend<D> {
    fn health(&self) -> BackendHealth {
        match &*self.fs.lock() {
            NativeState::Ready(_) => BackendHealth::Ready,
            NativeState::Unavailable(failure) => {
                let _ = (
                    failure.operation,
                    failure.error,
                    failure.path_tag,
                    failure.sync_stage,
                );
                BackendHealth::Unavailable
            }
            NativeState::Poisoned(failure) => {
                let _ = (
                    failure.operation,
                    failure.error,
                    failure.path_tag,
                    failure.sync_stage,
                );
                BackendHealth::Poisoned
            }
        }
    }
    fn get_file_ptr(&self, _: &str) -> Option<(usize, usize)> {
        None
    }
    fn list(&self, path: &str, out: &mut [u8]) -> Result<usize, VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::List, path, |fs| {
            let entries = fs.list_dir(rel)?;
            let mut written = 0;
            for (name, directory, _) in entries {
                let length = name.len() + 3;
                if length > out.len() - written {
                    break;
                }
                out[written..written + 2].copy_from_slice(if directory { b"d:" } else { b"f:" });
                written += 2;
                out[written..written + name.len()].copy_from_slice(name.as_bytes());
                written += name.len();
                out[written] = b'\n';
                written += 1;
            }
            Ok(written)
        })
    }
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Result<ListPage, VfsFsError> {
        let rel = self.rel_path(path)?;
        let entries = self.read(Operation::List, path, |fs| fs.list_dir(rel))?;
        let entries = entries.iter().map(|(name, directory, _)| {
            Ok((
                if *directory {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                },
                name.as_bytes(),
            ))
        });
        fill_page(entries, start, out)
    }
    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, VfsFsError> {
        let rel = match self.rel_path(path) {
            Ok(rel) => rel,
            // Nothing outside the mount (or through `..`) exists here.
            Err(_) => return Ok(None),
        };
        self.read(Operation::Stat, path, |fs| match fs.lookup(rel) {
            Ok(inode) => Ok(Some((inode.size, inode.is_dir()))),
            Err(FsError::NotFound) => Ok(None),
            Err(error) => Err(error),
        })
    }
    fn file_size(&self, path: &str) -> Result<u64, VfsFsError> {
        self.stat(path)?
            .map(|(size, _)| size)
            .ok_or(VfsFsError::NotFound)
    }
    fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Read, path, |fs| {
            let inode = fs.lookup(rel)?;
            if !inode.is_file() {
                return Err(FsError::IsADirectory);
            }
            let length = usize::try_from(inode.size).map_err(|_| FsError::Corrupted)?;
            let mut data = alloc::vec![0; length];
            Self::exact(fs.read_file(rel, 0, &mut data)?, length)?;
            Ok(data)
        })
    }
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Read, path, |fs| {
            let inode = fs.lookup(rel)?;
            if !inode.is_file() {
                return Err(FsError::IsADirectory);
            }
            let expected = inode.size.saturating_sub(offset).min(buf.len() as u64) as usize;
            let count = fs.read_file(rel, offset, buf)?;
            Self::exact(count, expected)?;
            Ok(count)
        })
    }
    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        Self::check_range(0, content.len())?;
        self.mutate(Operation::Write, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.replace_file(rel, content)?, content.len())
        })
    }
    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        Self::check_range(offset, content.len())?;
        self.mutate(Operation::WriteAt, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.write_file(rel, offset, content)?, content.len())
        })
    }
    fn append(&mut self, path: &str, content: &[u8]) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        // Lookup and overflow validation are read-only and precede mutation entry.
        let offset = self.read(Operation::Stat, path, |fs| match fs.lookup(rel) {
            Ok(inode) if inode.is_file() => Ok(inode.size),
            Ok(_) => Err(FsError::IsADirectory),
            Err(FsError::NotFound) => Ok(0),
            Err(error) => Err(error),
        })?;
        Self::check_range(offset, content.len())?;
        self.mutate(Operation::Append, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.write_file(rel, offset, content)?, content.len())
        })
    }
    fn mkdir(&mut self, path: &str) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Mkdir, path, |fs| match fs.lookup(rel) {
            Ok(_) => Err(FsError::AlreadyExists),
            Err(FsError::NotFound) => Ok(()),
            Err(error) => Err(error),
        })?;
        self.mutate(Operation::Mkdir, path, |fs| fs.create_dir(rel).map(|_| ()))
    }
    fn rmdir(&mut self, path: &str) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Rmdir, path, |fs| {
            let inode = fs.lookup(rel)?;
            if !inode.is_dir() {
                Err(FsError::NotADirectory)
            } else if inode.extent_count != 0 {
                Err(FsError::DirectoryNotEmpty)
            } else {
                Ok(())
            }
        })?;
        self.mutate(Operation::Rmdir, path, |fs| fs.rmdir(rel))
    }
    fn unlink(&mut self, path: &str) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Unlink, path, |fs| {
            if fs.lookup(rel)?.is_file() {
                Ok(())
            } else {
                Err(FsError::IsADirectory)
            }
        })?;
        self.mutate(Operation::Unlink, path, |fs| fs.unlink(rel))
    }
    /// Collect the whole tree read-only, then remove it child-first through the
    /// same per-entry `unlink`/`rmdir` mutations, each synced. A tree over the
    /// depth or entry bound is refused (`Unsupported`) before anything is
    /// removed. If a removal fails, the entries removed before it stay removed,
    /// the entries after it (always including the root) remain, and the failing
    /// entry is unchanged (refusal) or the instance is poisoned (storage
    /// fault), as in `mutate`.
    fn rmdir_recursive(&mut self, path: &str) -> Result<(), VfsFsError> {
        let rel = self.rel_path(path)?;
        if rel.trim_matches('/').is_empty() {
            return Err(VfsFsError::InvalidPath);
        }
        let entries = self.read(Operation::Rmdir, path, |fs| {
            if !fs.lookup(rel)?.is_dir() {
                return Err(FsError::NotADirectory);
            }
            let mut entries = Vec::new();
            Ok(collect_tree(
                fs,
                rel.trim_end_matches('/'),
                RECURSIVE_MAX_DEPTH,
                &mut entries,
            )?
            .then_some(entries))
        })?;
        let entries = entries.ok_or(VfsFsError::Unsupported)?;
        for (child, directory) in entries {
            let full = format!("{}{child}", self.prefix);
            if directory {
                self.mutate(Operation::Rmdir, &full, |fs| fs.rmdir(&child))?;
            } else {
                self.mutate(Operation::Unlink, &full, |fs| fs.unlink(&child))?;
            }
        }
        Ok(())
    }
    fn rename_no_replace(&mut self, old: &str, new: &str) -> Result<(), VfsFsError> {
        let (old, new) = (self.rel_path(old)?, self.rel_path(new)?);
        // Unsupported/type/duplicate checks are read-only before mutation entry;
        // a refusal inside the mutator is rolled back by `mutate` anyway.
        let renamable = self.read(Operation::Rename, old, |fs| {
            if !fs.lookup(old)?.is_file() {
                // Directory rename is not implemented.
                return Ok(Err(VfsFsError::Unsupported));
            }
            if old == new {
                return Ok(Ok(()));
            }
            match fs.lookup(new) {
                Ok(_) => Err(FsError::AlreadyExists),
                Err(FsError::NotFound) => Ok(Ok(())),
                Err(error) => Err(error),
            }
        })?;
        renamable?;
        self.mutate(Operation::Rename, old, |fs| fs.rename(old, new))
    }
    fn sync(&mut self, path: &str) -> Result<(), VfsFsError> {
        self.rel_path(path)?;
        // mutate already executes exactly one engine sync, including both barriers.
        self.mutate(Operation::Sync, path, |_| Ok(()))
    }
    fn stage_path(&self, id: u32) -> Option<String> {
        Some(format!("{}{STAGING_DIR}/{id:08x}", self.prefix))
    }
    fn observe(&self, path: &str) -> Result<Option<Observation>, VfsFsError> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Stat, path, |fs| match fs.lookup(rel) {
            Ok(inode) => Ok(Observation::Present {
                inode: inode.inode_num,
                generation: inode.generation,
                size: inode.size,
                regular: inode.is_file(),
            }),
            Err(FsError::NotFound) => Ok(Observation::Absent),
            Err(error) => Err(error),
        })
        .map(Some)
    }
    fn stage_create(&mut self, stage: &str) -> Result<(), VfsFsError> {
        let rel = self.rel_path(stage)?;
        self.stage(stage, |fs| {
            match fs.lookup(STAGING_DIR) {
                Ok(inode) if inode.is_dir() => {}
                Ok(_) => return Err(FsError::NotADirectory),
                Err(FsError::NotFound) => {
                    fs.create_dir(STAGING_DIR)?;
                }
                Err(error) => return Err(error),
            }
            fs.create_file(rel).map(|_| ())
        })
    }
    fn stage_write(&mut self, stage: &str, offset: u64, bytes: &[u8]) -> Result<(), VfsFsError> {
        let rel = self.rel_path(stage)?;
        Self::check_range(offset, bytes.len())?;
        self.stage(stage, |fs| {
            Self::exact(fs.write_file(rel, offset, bytes)?, bytes.len())
        })
    }
    fn publish_if(
        &mut self,
        stage: &str,
        sealed: (u64, u64),
        dst: &str,
        expect: SaveExpect,
    ) -> SaveOutcome {
        let refused = SaveOutcome::NotPublished(save_refusal::REFUSED);
        let (Ok(stage_rel), Ok(dst_rel)) = (self.rel_path(stage), self.rel_path(dst)) else {
            return refused;
        };
        let expect = match expect {
            SaveExpect::Absent => Expect::Absent,
            SaveExpect::Present { inode, generation } => Expect::Present {
                inode_num: inode,
                generation,
            },
        };
        let mut state = self.fs.lock();
        let NativeState::Ready(fs) = &mut *state else {
            return refused;
        };
        // The stage check and the publication share this lock, so nothing can
        // change the sealed bytes in between.
        let staged = match fs.lookup(stage_rel) {
            Ok(inode) => (inode.inode_num, inode.generation) == sealed,
            Err(error) if error.is_storage_fault() => {
                *state = NativeState::Poisoned(Failure {
                    operation: Operation::Publish,
                    error,
                    path_tag: path_tag(dst),
                    sync_stage: false,
                });
                return refused;
            }
            Err(_) => false,
        };
        if !staged {
            return SaveOutcome::NotPublished(save_refusal::STAGE_CHANGED);
        }
        let mut sync_stage = false;
        let published = fs.atomically(|fs| {
            fs.rename_replace_if(stage_rel, dst_rel, expect)?;
            sync_stage = true;
            fs.sync()
        });
        // After a successful sync the publication is on disk: any failure to
        // observe it is Indeterminate, never a refusal.
        let result = match published {
            Ok(()) => fs.lookup(dst_rel).map_err(|error| (error, true)),
            Err(FsError::Conflict) => return SaveOutcome::Conflict,
            Err(error) if error.is_storage_fault() => Err((error, sync_stage)),
            // Rolled back by the engine: nothing was published.
            Err(_) => return refused,
        };
        match result {
            Ok(inode) => SaveOutcome::Committed {
                inode: inode.inode_num,
                generation: inode.generation,
            },
            Err((error, sync_stage)) => {
                *state = NativeState::Poisoned(Failure {
                    operation: Operation::Publish,
                    error,
                    path_tag: path_tag(dst),
                    sync_stage,
                });
                SaveOutcome::Indeterminate
            }
        }
    }
}

/// A ready backend over a freshly formatted private device (host tests and the
/// `test-hooks` boot self-tests only).
#[cfg(any(test, feature = "test-hooks"))]
impl<D: cellos_fs::disk::BlockDevice> CellosFsBackend<D> {
    pub(crate) fn formatted_for_test(prefix: &'static str, disk: D, blocks: u64) -> Self {
        let fs = CellosFs::format(disk, blocks).expect("format test volume");
        Self {
            prefix,
            fs: Mutex::new(NativeState::Ready(fs)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cellos_fs::{disk::BlockDevice, format::BLOCK_SIZE};
    use std::sync::{Arc, Mutex as HostMutex};
    #[derive(Clone)]
    struct Disk(Arc<HostMutex<Medium>>);
    struct Medium {
        blocks: Vec<[u8; BLOCK_SIZE]>,
        reads: usize,
        writes: usize,
        flushes: usize,
        fail_read: bool,
        fail_flush: Option<usize>,
    }
    impl BlockDevice for Disk {
        fn total_blocks(&self) -> u64 {
            self.0.lock().unwrap().blocks.len() as u64
        }
        fn read_block(&mut self, block: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap();
            medium.reads += 1;
            if medium.fail_read {
                return Err(FsError::IoError);
            }
            out.copy_from_slice(
                medium
                    .blocks
                    .get(block as usize)
                    .ok_or(FsError::PartitionOverflow)?,
            );
            Ok(())
        }
        fn write_block(&mut self, block: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap();
            medium.writes += 1;
            *medium
                .blocks
                .get_mut(block as usize)
                .ok_or(FsError::PartitionOverflow)? = *data;
            Ok(())
        }
        fn flush(&mut self) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap();
            medium.flushes += 1;
            if medium.fail_flush == Some(medium.flushes) {
                Err(FsError::IoError)
            } else {
                Ok(())
            }
        }
    }
    fn ready() -> (CellosFsBackend<Disk>, Disk) {
        ready_sized(128)
    }
    fn ready_sized(blocks: usize) -> (CellosFsBackend<Disk>, Disk) {
        let disk = Disk(Arc::new(HostMutex::new(Medium {
            blocks: alloc::vec![[0; BLOCK_SIZE]; blocks],
            reads: 0,
            writes: 0,
            flushes: 0,
            fail_read: false,
            fail_flush: None,
        })));
        let fs = CellosFs::format(disk.clone(), blocks as u64).unwrap();
        (
            CellosFsBackend {
                prefix: "/srv",
                fs: Mutex::new(NativeState::Ready(fs)),
            },
            disk,
        )
    }
    #[test]
    fn every_mutation_flush_failure_closes_instance_without_retry() {
        for operation in 0..7 {
            for barrier in 1..=2 {
                let (mut backend, disk) = ready();
                assert!(backend.write("/srv/a", b"old").is_ok());
                assert!(backend.mkdir("/srv/d").is_ok());
                {
                    let mut m = disk.0.lock().unwrap();
                    m.fail_flush = Some(m.flushes + barrier);
                }
                let result = match operation {
                    0 => backend.write("/srv/a", b"replacement"),
                    1 => backend.write_at("/srv/a", 1, b"X"),
                    2 => backend.append("/srv/a", b"tail"),
                    3 => backend.mkdir("/srv/new"),
                    4 => backend.rmdir("/srv/d"),
                    5 => backend.unlink("/srv/a"),
                    _ => backend.rename_no_replace("/srv/a", "/srv/b"),
                };
                assert_eq!(
                    result,
                    Err(VfsFsError::Io),
                    "operation {operation}, barrier {barrier}"
                );
                assert_eq!(backend.health(), BackendHealth::Poisoned);
                let counters = {
                    let m = disk.0.lock().unwrap();
                    (m.reads, m.writes, m.flushes)
                };
                assert!(backend.write("/srv/after", b"no").is_err());
                assert!(backend.sync("/srv").is_err());
                assert_eq!(backend.stat("/srv/a"), Err(VfsFsError::Unavailable));
                assert_eq!(
                    backend.read_at("/srv/a", 0, &mut [0; 8]),
                    Err(VfsFsError::Unavailable)
                );
                let m = disk.0.lock().unwrap();
                assert_eq!((m.reads, m.writes, m.flushes), counters);
                drop(m);
                let state = backend.fs.lock();
                match &*state {
                    NativeState::Poisoned(first) => assert_eq!(first.error, FsError::IoError),
                    _ => panic!("lost poison"),
                }
            }
        }
    }
    #[test]
    fn replacement_truncates_and_read_fault_never_becomes_empty_success() {
        let (mut backend, disk) = ready();
        let original = alloc::vec![0x73; BLOCK_SIZE * 2 + 19];
        assert!(backend.write("/srv/a", &original).is_ok());
        assert_eq!(backend.read_to_vec("/srv/a").as_deref(), Ok(&original[..]));
        assert!(backend.write("/srv/a", b"small").is_ok());
        assert_eq!(backend.stat("/srv/a"), Ok(Some((5, false))));
        assert_eq!(backend.read_to_vec("/srv/a").as_deref(), Ok(&b"small"[..]));
        assert!(backend
            .write("/srv/b", &alloc::vec![0x42; BLOCK_SIZE * 2])
            .is_ok());
        disk.0.lock().unwrap().fail_read = true;
        assert_eq!(backend.stat("/srv/b"), Err(VfsFsError::Io));
        assert_eq!(backend.health(), BackendHealth::Poisoned);
        assert!(backend.append("/srv/b", b"retry").is_err());
    }
    #[test]
    fn local_offset_rejection_does_not_poison_or_touch_disk() {
        let (mut backend, disk) = ready();
        let counters = {
            let m = disk.0.lock().unwrap();
            (m.reads, m.writes, m.flushes)
        };
        assert!(backend.write_at("/srv/a", u64::MAX, b"x").is_err());
        assert_eq!(backend.health(), BackendHealth::Ready);
        let m = disk.0.lock().unwrap();
        assert_eq!((m.reads, m.writes, m.flushes), counters);
    }
    #[test]
    fn unsupported_directory_rename_does_not_poison_following_operations() {
        let (mut backend, _) = ready();
        assert!(backend.mkdir("/srv/d").is_ok());
        assert_eq!(
            backend.rename_no_replace("/srv/d", "/srv/new"),
            Err(VfsFsError::Unsupported)
        );
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert!(backend.rmdir("/srv/d").is_ok());
        assert!(backend.write("/srv/persist", b"retained").is_ok());
        assert_eq!(
            backend.read_to_vec("/srv/persist").as_deref(),
            Ok(&b"retained"[..])
        );
    }
    #[test]
    fn ordinary_namespace_rejections_leave_ready_and_contents_unchanged() {
        let (mut backend, _) = ready();
        assert_eq!(backend.rmdir("/srv/missing"), Err(VfsFsError::NotFound));
        assert!(backend.write("/srv/a", b"retained").is_ok());
        assert_eq!(backend.rmdir("/srv/a"), Err(VfsFsError::NotADirectory));
        assert!(backend.mkdir("/srv/d").is_ok());
        assert_eq!(backend.mkdir("/srv/d"), Err(VfsFsError::AlreadyExists));
        assert!(backend.write("/srv/d/child", b"child").is_ok());
        assert_eq!(backend.rmdir("/srv/d"), Err(VfsFsError::DirectoryNotEmpty));
        assert_eq!(backend.unlink("/srv/d"), Err(VfsFsError::IsADirectory));
        assert_eq!(backend.read_to_vec("/srv/d"), Err(VfsFsError::IsADirectory));
        assert_eq!(
            backend.write("/other/x", b"x"),
            Err(VfsFsError::InvalidPath)
        );
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert_eq!(
            backend.read_to_vec("/srv/a").as_deref(),
            Ok(&b"retained"[..])
        );
        assert_eq!(
            backend.read_to_vec("/srv/d/child").as_deref(),
            Ok(&b"child"[..])
        );
        assert!(backend.unlink("/srv/d/child").is_ok());
        assert!(backend.rmdir("/srv/d").is_ok());
    }
    /// Audit H-23: any cell could disable the mount with one refused request
    /// or 254 create/unlink pairs.
    #[test]
    fn refused_mutations_never_poison_and_inode_numbers_recycle() {
        let (mut backend, _) = ready();
        assert!(backend.write("/srv/keep", b"kept").is_ok());
        assert!(backend.write("/srv/nodir/x", b"x").is_err());
        assert!(backend
            .write(&format!("/srv/{}", "n".repeat(47)), b"x")
            .is_err());
        assert!(backend.append("/srv", b"x").is_err());
        assert!(backend.mkdir("/srv/d").is_ok());
        for i in 0..7 {
            assert!(backend.write(&format!("/srv/d/{i}"), b"x").is_ok());
        }
        assert!(backend.write("/srv/d/eighth", b"x").is_err());
        assert_eq!(
            backend.write("/srv/big", &alloc::vec![1; 127 * BLOCK_SIZE]),
            Err(VfsFsError::NoSpace)
        );
        assert!(backend.write("/srv/.cbx-save", b"not a directory").is_ok());
        assert!(backend.stage_create("/srv/.cbx-save/00000001").is_err());
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert_eq!(backend.stat("/srv/big"), Ok(None));
        for _ in 0..300 {
            assert!(backend.write("/srv/cycle", b"c").is_ok());
            assert!(backend.unlink("/srv/cycle").is_ok());
        }
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert_eq!(
            backend.read_to_vec("/srv/keep").as_deref(),
            Ok(&b"kept"[..])
        );
    }
    #[test]
    fn recursive_removal_deletes_nested_tree_and_keeps_siblings() {
        let (mut backend, _) = ready();
        assert!(backend.mkdir("/srv/d").is_ok());
        assert!(backend.mkdir("/srv/d/e").is_ok());
        assert!(backend.mkdir("/srv/d/e/empty").is_ok());
        assert!(backend.write("/srv/d/a", b"a").is_ok());
        assert!(backend.write("/srv/d/e/b", b"b").is_ok());
        assert!(backend.write("/srv/keep", b"kept").is_ok());
        assert!(backend.rmdir_recursive("/srv/d").is_ok());
        assert_eq!(backend.health(), BackendHealth::Ready);
        for gone in [
            "/srv/d",
            "/srv/d/a",
            "/srv/d/e",
            "/srv/d/e/b",
            "/srv/d/e/empty",
        ] {
            assert_eq!(backend.stat(gone), Ok(None), "{gone}");
        }
        assert_eq!(
            backend.read_to_vec("/srv/keep").as_deref(),
            Ok(&b"kept"[..])
        );
        assert!(backend.mkdir("/srv/d").is_ok());
        assert!(backend.rmdir_recursive("/srv/d/").is_ok());
        assert_eq!(backend.stat("/srv/d"), Ok(None));
    }
    #[test]
    fn recursive_removal_refusals_leave_ready_and_contents_unchanged() {
        let (mut backend, disk) = ready();
        assert!(backend.write("/srv/file", b"file").is_ok());
        assert!(backend.mkdir("/srv/d").is_ok());
        assert!(backend.write("/srv/d/child", b"child").is_ok());
        let counters = {
            let m = disk.0.lock().unwrap();
            (m.writes, m.flushes)
        };
        for refused in [
            "/srv",
            "/srv/",
            "/srv/file",
            "/srv/missing",
            "/srv/d/../file",
            "/srvx/d",
            "/other/d",
        ] {
            assert!(backend.rmdir_recursive(refused).is_err(), "{refused}");
        }
        assert_eq!(backend.health(), BackendHealth::Ready);
        let m = disk.0.lock().unwrap();
        assert_eq!((m.writes, m.flushes), counters);
        drop(m);
        assert_eq!(
            backend.read_to_vec("/srv/file").as_deref(),
            Ok(&b"file"[..])
        );
        assert_eq!(
            backend.read_to_vec("/srv/d/child").as_deref(),
            Ok(&b"child"[..])
        );
    }
    #[test]
    fn recursive_removal_over_depth_bound_removes_nothing() {
        let (mut backend, disk) = ready_sized(512);
        let mut path = String::from("/srv/deep");
        assert!(backend.mkdir(&path).is_ok());
        for _ in 0..=RECURSIVE_MAX_DEPTH {
            path.push_str("/n");
            assert!(backend.mkdir(&path).is_ok(), "{path}");
        }
        assert!(backend.write(&format!("{path}/leaf"), b"leaf").is_ok());
        let counters = {
            let m = disk.0.lock().unwrap();
            (m.writes, m.flushes)
        };
        assert!(backend.rmdir_recursive("/srv/deep").is_err());
        assert_eq!(backend.health(), BackendHealth::Ready);
        let m = disk.0.lock().unwrap();
        assert_eq!((m.writes, m.flushes), counters);
        drop(m);
        assert_eq!(
            backend.read_to_vec(&format!("{path}/leaf")).as_deref(),
            Ok(&b"leaf"[..])
        );
        // One level shallower fits the bound and is removed whole.
        assert!(backend.rmdir_recursive("/srv/deep/n").is_ok());
        assert_eq!(backend.stat("/srv/deep/n"), Ok(None));
        assert!(backend.rmdir("/srv/deep").is_ok());
    }
    #[test]
    fn recursive_removal_failure_keeps_unattempted_entries_and_poisons() {
        let (mut backend, disk) = ready();
        assert!(backend.mkdir("/srv/d").is_ok());
        assert!(backend.write("/srv/d/a", b"a").is_ok());
        assert!(backend.write("/srv/d/b", b"b").is_ok());
        let order = {
            let mut state = backend.fs.lock();
            let NativeState::Ready(fs) = &mut *state else {
                panic!("not ready")
            };
            let mut order = Vec::new();
            assert!(collect_tree(fs, "/d", RECURSIVE_MAX_DEPTH, &mut order).unwrap());
            order
        };
        assert_eq!(order.len(), 3);
        assert_eq!(order[2], (String::from("/d"), true));
        // Each per-entry removal syncs with two barriers; fail the second
        // removal's second barrier so the volume stays openable afterwards.
        let start = {
            let mut m = disk.0.lock().unwrap();
            m.fail_flush = Some(m.flushes + 4);
            m.flushes
        };
        assert!(backend.rmdir_recursive("/srv/d").is_err());
        assert_eq!(backend.health(), BackendHealth::Poisoned);
        // The root removal is never attempted after the failure.
        assert_eq!(disk.0.lock().unwrap().flushes, start + 4);
        let mut reopened = CellosFs::open(disk.clone()).unwrap();
        assert_eq!(reopened.lookup(&order[0].0).err(), Some(FsError::NotFound));
        assert!(reopened.lookup("/d").unwrap().is_dir());
    }
}
