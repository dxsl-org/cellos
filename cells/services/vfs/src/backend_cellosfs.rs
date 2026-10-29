//! Native V1 filesystem backend. Observed sync is not crash-preserving acknowledgement.
use alloc::{format, string::String, vec::Vec};
use api::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome};
use cellos_fs::{CellosFs, Expect, disk::FsError};
use ostd::prelude::Mutex;
use crate::backend::{fill_page, BackendHealth, FsBackend, ListPage};
use api::ipc::vfs_dir_page::EntryKind;
use crate::disk_cellosfs::CellosPartitionDisk;

#[derive(Clone, Copy)]
enum Operation { Open, List, Stat, Read, Write, WriteAt, Append, Mkdir, Rmdir, Unlink, Rename, Sync, Stage, Publish }
#[derive(Clone, Copy)]
struct Failure { operation: Operation, error: FsError, path_tag: u64, sync_stage: bool }
fn path_tag(path: &str) -> u64 {
    // Diagnostic correlation only, never an authority identity.
    path.bytes().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3))
}
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
    pub fn mount(prefix: &'static str, base_lba: u64, total_sectors: u64) -> Self {
        let disk = CellosPartitionDisk::new(base_lba, total_sectors);
        let opened = if total_sectors % 8 != 0 || base_lba.checked_add(total_sectors).is_none() {
            Err(FsError::PartitionOverflow)
        } else { CellosFs::open(disk) };
        let state = match opened.and_then(|mut fs| clear_staging(&mut fs).map(|_| fs)) {
            Ok(fs) => {
                ostd::io::println("[vfs] CellosFS Native mounted at existing volume");
                NativeState::Ready(fs)
            }
            Err(error) => {
                ostd::io::println("[vfs] WARNING: CellosFS open failed — volume unavailable");
                NativeState::Unavailable(Failure { operation: Operation::Open, error, path_tag: path_tag(prefix), sync_stage: false })
            }
        };
        Self { prefix, fs: Mutex::new(state) }
    }
}

/// Directory, per CellosFS mount, that holds unpublished save transactions.
const STAGING_DIR: &str = "/.cbx-save";

/// Remove staging objects left by transactions that did not survive the last
/// mount. Nothing outside [`STAGING_DIR`] is touched.
pub(crate) fn clear_staging<D: cellos_fs::disk::BlockDevice>(fs: &mut CellosFs<D>) -> Result<(), FsError> {
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
impl<D: cellos_fs::disk::BlockDevice> CellosFsBackend<D> {
    fn rel_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        let rel = path.strip_prefix(self.prefix)?;
        if !rel.is_empty() && !rel.starts_with('/') || rel.split('/').any(|c| c == "..") { return None; }
        Some(rel)
    }
    fn read<R>(&self, operation: Operation, path: &str, f: impl FnOnce(&mut CellosFs<D>) -> Result<R, FsError>) -> Option<R> {
        let mut state = self.fs.lock();
        let result = match &mut *state { NativeState::Ready(fs) => f(fs), _ => return None };
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if matches!(error, FsError::IoError | FsError::Corrupted | FsError::PartitionOverflow) {
                    *state = NativeState::Poisoned(Failure { operation, error, path_tag: path_tag(path), sync_stage: false });
                }
                None
            }
        }
    }
    fn mutate(&self, operation: Operation, path: &str, f: impl FnOnce(&mut CellosFs<D>) -> Result<(), FsError>) -> bool {
        let mut state = self.fs.lock();
        let result = match &mut *state {
            NativeState::Ready(fs) => match f(fs) {
                Ok(()) => fs.sync().map_err(|error| (error, true)),
                Err(error) => Err((error, false)),
            },
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err((error, sync_stage)) => { *state = NativeState::Poisoned(Failure { operation, error, path_tag: path_tag(path), sync_stage }); false }
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
        if count == expected { Ok(()) } else { Err(FsError::Corrupted) }
    }
    /// A staging mutation: no sync, poisoned only on a storage-level error.
    fn stage(&self, path: &str, f: impl FnOnce(&mut CellosFs<D>) -> Result<(), FsError>) -> bool {
        self.read(Operation::Stage, path, f).is_some()
    }
}
impl<D: cellos_fs::disk::BlockDevice + Send> FsBackend for CellosFsBackend<D> {
    fn health(&self) -> BackendHealth {
        match &*self.fs.lock() {
            NativeState::Ready(_) => BackendHealth::Ready,
            NativeState::Unavailable(failure) => { let _ = (failure.operation, failure.error, failure.path_tag, failure.sync_stage); BackendHealth::Unavailable },
            NativeState::Poisoned(failure) => { let _ = (failure.operation, failure.error, failure.path_tag, failure.sync_stage); BackendHealth::Poisoned },
        }
    }
    fn get_file_ptr(&self, _: &str) -> Option<(usize, usize)> { None }
    fn list(&self, path: &str, out: &mut [u8]) -> usize {
        let Some(rel) = self.rel_path(path) else { return 0 };
        self.read(Operation::List, path, |fs| {
            let entries = fs.list_dir(rel)?;
            let mut written = 0;
            for (name, directory, _) in entries {
                let length = name.len() + 3;
                if length > out.len() - written { break; }
                out[written..written+2].copy_from_slice(if directory { b"d:" } else { b"f:" });
                written += 2;
                out[written..written+name.len()].copy_from_slice(name.as_bytes());
                written += name.len(); out[written] = b'\n'; written += 1;
            }
            Ok(written)
        }).unwrap_or(0)
    }
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Option<ListPage> {
        let rel = self.rel_path(path)?;
        let entries = self.read(Operation::List, path, |fs| fs.list_dir(rel))?;
        let entries = entries.iter().map(|(name, directory, _)| {
            Ok((if *directory { EntryKind::Dir } else { EntryKind::File }, name.as_bytes()))
        });
        fill_page(entries, start, out)
    }
    fn stat(&self, path: &str) -> Option<(u64, bool)> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Stat, path, |fs| fs.lookup(rel).map(|i| (i.size, i.is_dir())))
    }
    fn file_size(&self, path: &str) -> u64 { self.stat(path).map(|i| i.0).unwrap_or(0) }
    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        let Some(rel) = self.rel_path(path) else { return Vec::new() };
        self.read(Operation::Read, path, |fs| {
            let inode = fs.lookup(rel)?;
            if !inode.is_file() { return Err(FsError::IsADirectory); }
            let length = usize::try_from(inode.size).map_err(|_| FsError::Corrupted)?;
            let mut data = alloc::vec![0; length];
            Self::exact(fs.read_file(rel, 0, &mut data)?, length)?;
            Ok(data)
        }).unwrap_or_default()
    }
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        let Some(rel) = self.rel_path(path) else { return 0 };
        self.read(Operation::Read, path, |fs| {
            let inode = fs.lookup(rel)?;
            if !inode.is_file() { return Err(FsError::IsADirectory); }
            let expected = inode.size.saturating_sub(offset).min(buf.len() as u64) as usize;
            let count = fs.read_file(rel, offset, buf)?;
            Self::exact(count, expected)?; Ok(count)
        }).unwrap_or(0)
    }
    fn write(&mut self, path: &str, content: &[u8]) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        if CellosFs::<D>::validate_write_range(0, content.len()).is_err() { return false; }
        self.mutate(Operation::Write, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.replace_file(rel, content)?, content.len())
        })
    }
    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        if CellosFs::<D>::validate_write_range(offset, content.len()).is_err() { return false; }
        self.mutate(Operation::WriteAt, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.write_file(rel, offset, content)?, content.len())
        })
    }
    fn append(&mut self, path: &str, content: &[u8]) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        // Lookup and overflow validation are read-only and precede mutation entry.
        let Some(offset) = self.read(Operation::Stat, path, |fs| match fs.lookup(rel) {
            Ok(inode) if inode.is_file() => Ok(inode.size),
            Ok(_) => Err(FsError::IsADirectory),
            Err(FsError::NotFound) => Ok(0),
            Err(error) => Err(error),
        }) else { return false };
        if CellosFs::<D>::validate_write_range(offset, content.len()).is_err() { return false; }
        self.mutate(Operation::Append, path, |fs| {
            Self::ensure_file(fs, rel)?;
            Self::exact(fs.write_file(rel, offset, content)?, content.len())
        })
    }
    fn mkdir(&mut self, path: &str) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        if self.read(Operation::Mkdir, path, |fs| match fs.lookup(rel) {
            Ok(_) => Ok(false),
            Err(FsError::NotFound) => Ok(true),
            Err(error) => Err(error),
        }) != Some(true) { return false; }
        self.mutate(Operation::Mkdir, path, |fs| fs.create_dir(rel).map(|_| ()))
    }
    fn rmdir(&mut self, path: &str) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        if self.read(Operation::Rmdir, path, |fs| {
            let inode = fs.lookup(rel)?;
            Ok(inode.is_dir() && inode.extent_count == 0)
        }) != Some(true) { return false; }
        self.mutate(Operation::Rmdir, path, |fs| fs.rmdir(rel))
    }
    fn unlink(&mut self, path: &str) -> bool {
        let Some(rel) = self.rel_path(path) else { return false };
        if self.read(Operation::Unlink, path, |fs| fs.lookup(rel).map(|inode| inode.is_file())) != Some(true) {
            return false;
        }
        self.mutate(Operation::Unlink, path, |fs| fs.unlink(rel))
    }
    fn rmdir_recursive(&mut self, _: &str) -> bool { false }
    fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        let (Some(old), Some(new)) = (self.rel_path(old), self.rel_path(new)) else { return false };
        // Unsupported/type/duplicate checks are read-only before mutation entry.
        // Never infer no effect from an error returned by the mutator itself.
        if self.read(Operation::Rename, old, |fs| {
            if !fs.lookup(old)?.is_file() { return Ok(false); }
            if old == new { return Ok(true); }
            match fs.lookup(new) {
                Ok(_) => Ok(false),
                Err(FsError::NotFound) => Ok(true),
                Err(error) => Err(error),
            }
        }) != Some(true) { return false; }
        self.mutate(Operation::Rename, old, |fs| fs.rename(old, new))
    }
    fn sync(&mut self, path: &str) -> bool {
        if self.rel_path(path).is_none() { return false; }
        // mutate already executes exactly one engine sync, including both barriers.
        self.mutate(Operation::Sync, path, |_| Ok(()))
    }
    fn stage_path(&self, id: u32) -> Option<String> {
        Some(format!("{}{STAGING_DIR}/{id:08x}", self.prefix))
    }
    fn observe(&self, path: &str) -> Option<Observation> {
        let rel = self.rel_path(path)?;
        self.read(Operation::Stat, path, |fs| match fs.lookup(rel) {
            Ok(inode) => Ok(Observation::Present {
                inode: inode.inode_num, generation: inode.generation,
                size: inode.size, regular: inode.is_file(),
            }),
            Err(FsError::NotFound) => Ok(Observation::Absent),
            Err(error) => Err(error),
        })
    }
    fn stage_create(&mut self, stage: &str) -> bool {
        let Some(rel) = self.rel_path(stage) else { return false };
        self.stage(stage, |fs| {
            match fs.lookup(STAGING_DIR) {
                Ok(inode) if inode.is_dir() => {}
                Ok(_) => return Err(FsError::NotADirectory),
                Err(FsError::NotFound) => { fs.create_dir(STAGING_DIR)?; }
                Err(error) => return Err(error),
            }
            fs.create_file(rel).map(|_| ())
        })
    }
    fn stage_write(&mut self, stage: &str, offset: u64, bytes: &[u8]) -> bool {
        let Some(rel) = self.rel_path(stage) else { return false };
        if CellosFs::<D>::validate_write_range(offset, bytes.len()).is_err() { return false; }
        self.stage(stage, |fs| Self::exact(fs.write_file(rel, offset, bytes)?, bytes.len()))
    }
    fn publish_if(&mut self, stage: &str, sealed: (u64, u64), dst: &str, expect: SaveExpect) -> SaveOutcome {
        let refused = SaveOutcome::NotPublished(save_refusal::REFUSED);
        let (Some(stage_rel), Some(dst_rel)) = (self.rel_path(stage), self.rel_path(dst)) else { return refused };
        let expect = match expect {
            SaveExpect::Absent => Expect::Absent,
            SaveExpect::Present { inode, generation } => Expect::Present { inode_num: inode, generation },
        };
        let mut state = self.fs.lock();
        let NativeState::Ready(fs) = &mut *state else { return refused };
        // The stage check and the publication share this lock, so nothing can
        // change the sealed bytes in between.
        let staged = match fs.lookup(stage_rel) {
            Ok(inode) => (inode.inode_num, inode.generation) == sealed,
            Err(FsError::NotFound) => false,
            Err(error) => {
                *state = NativeState::Poisoned(Failure { operation: Operation::Publish, error, path_tag: path_tag(dst), sync_stage: false });
                return refused;
            }
        };
        if !staged { return SaveOutcome::NotPublished(save_refusal::STAGE_CHANGED); }
        let result = match fs.rename_replace_if(stage_rel, dst_rel, expect) {
            Ok(()) => fs.sync().map_err(|error| (error, true))
                .and_then(|()| fs.lookup(dst_rel).map_err(|error| (error, true))),
            Err(FsError::Conflict) => return SaveOutcome::Conflict,
            Err(error @ (FsError::IoError | FsError::Corrupted | FsError::PartitionOverflow)) => Err((error, false)),
            // Refused before anything was staged in the engine.
            Err(_) => return refused,
        };
        match result {
            Ok(inode) => SaveOutcome::Committed { inode: inode.inode_num, generation: inode.generation },
            Err((error, sync_stage)) => {
                *state = NativeState::Poisoned(Failure { operation: Operation::Publish, error, path_tag: path_tag(dst), sync_stage });
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
        Self { prefix, fs: Mutex::new(NativeState::Ready(fs)) }
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
        reads: usize, writes: usize, flushes: usize,
        fail_read: bool, fail_flush: Option<usize>,
    }
    impl BlockDevice for Disk {
        fn total_blocks(&self) -> u64 { self.0.lock().unwrap().blocks.len() as u64 }
        fn read_block(&mut self, block: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap(); medium.reads += 1;
            if medium.fail_read { return Err(FsError::IoError); }
            out.copy_from_slice(medium.blocks.get(block as usize).ok_or(FsError::PartitionOverflow)?); Ok(())
        }
        fn write_block(&mut self, block: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap(); medium.writes += 1;
            *medium.blocks.get_mut(block as usize).ok_or(FsError::PartitionOverflow)? = *data; Ok(())
        }
        fn flush(&mut self) -> Result<(), FsError> {
            let mut medium = self.0.lock().unwrap(); medium.flushes += 1;
            if medium.fail_flush == Some(medium.flushes) { Err(FsError::IoError) } else { Ok(()) }
        }
    }
    fn ready() -> (CellosFsBackend<Disk>, Disk) {
        let disk = Disk(Arc::new(HostMutex::new(Medium {
            blocks: alloc::vec![[0; BLOCK_SIZE]; 128], reads: 0, writes: 0, flushes: 0,
            fail_read: false, fail_flush: None,
        })));
        let fs = CellosFs::format(disk.clone(), 128).unwrap();
        (CellosFsBackend { prefix: "/srv", fs: Mutex::new(NativeState::Ready(fs)) }, disk)
    }
    #[test]
    fn every_mutation_flush_failure_closes_instance_without_retry() {
        for operation in 0..7 {
            for barrier in 1..=2 {
                let (mut backend, disk) = ready();
                assert!(backend.write("/srv/a", b"old"));
                assert!(backend.mkdir("/srv/d"));
                { let mut m = disk.0.lock().unwrap(); m.fail_flush = Some(m.flushes + barrier); }
                let success = match operation {
                    0 => backend.write("/srv/a", b"replacement"),
                    1 => backend.write_at("/srv/a", 1, b"X"),
                    2 => backend.append("/srv/a", b"tail"),
                    3 => backend.mkdir("/srv/new"),
                    4 => backend.rmdir("/srv/d"),
                    5 => backend.unlink("/srv/a"),
                    _ => backend.rename_no_replace("/srv/a", "/srv/b"),
                };
                assert!(!success, "operation {operation}, barrier {barrier}");
                assert_eq!(backend.health(), BackendHealth::Poisoned);
                let counters = { let m = disk.0.lock().unwrap(); (m.reads, m.writes, m.flushes) };
                assert!(!backend.write("/srv/after", b"no"));
                assert!(!backend.sync("/srv"));
                assert_eq!(backend.stat("/srv/a"), None);
                assert_eq!(backend.read_at("/srv/a", 0, &mut [0; 8]), 0);
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
        assert!(backend.write("/srv/a", &original));
        assert_eq!(backend.read_to_vec("/srv/a"), original);
        assert!(backend.write("/srv/a", b"small"));
        assert_eq!(backend.stat("/srv/a"), Some((5, false)));
        assert_eq!(backend.read_to_vec("/srv/a"), b"small");
        assert!(backend.write("/srv/b", &alloc::vec![0x42; BLOCK_SIZE * 2]));
        disk.0.lock().unwrap().fail_read = true;
        assert_eq!(backend.stat("/srv/b"), None);
        assert_eq!(backend.health(), BackendHealth::Poisoned);
        assert!(!backend.append("/srv/b", b"retry"));
    }
    #[test]
    fn local_offset_rejection_does_not_poison_or_touch_disk() {
        let (mut backend, disk) = ready();
        let counters = { let m = disk.0.lock().unwrap(); (m.reads, m.writes, m.flushes) };
        assert!(!backend.write_at("/srv/a", u64::MAX, b"x"));
        assert_eq!(backend.health(), BackendHealth::Ready);
        let m = disk.0.lock().unwrap();
        assert_eq!((m.reads, m.writes, m.flushes), counters);
    }
    #[test]
    fn unsupported_directory_rename_does_not_poison_following_operations() {
        let (mut backend, _) = ready();
        assert!(backend.mkdir("/srv/d"));
        assert!(!backend.rename_no_replace("/srv/d", "/srv/new"));
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert!(backend.rmdir("/srv/d"));
        assert!(backend.write("/srv/persist", b"retained"));
        assert_eq!(backend.read_to_vec("/srv/persist"), b"retained");
    }
    #[test]
    fn ordinary_namespace_rejections_leave_ready_and_contents_unchanged() {
        let (mut backend, _) = ready();
        assert!(!backend.rmdir("/srv/missing"));
        assert!(backend.write("/srv/a", b"retained"));
        assert!(!backend.rmdir("/srv/a"));
        assert!(backend.mkdir("/srv/d"));
        assert!(!backend.mkdir("/srv/d"));
        assert!(backend.write("/srv/d/child", b"child"));
        assert!(!backend.rmdir("/srv/d"));
        assert!(!backend.unlink("/srv/d"));
        assert_eq!(backend.health(), BackendHealth::Ready);
        assert_eq!(backend.read_to_vec("/srv/a"), b"retained");
        assert_eq!(backend.read_to_vec("/srv/d/child"), b"child");
        assert!(backend.unlink("/srv/d/child"));
        assert!(backend.rmdir("/srv/d"));
    }
}

