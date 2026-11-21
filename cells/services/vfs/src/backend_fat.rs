//! FAT backend over the VirtIO block device (fatfs crate + CachedBlockStream).
//!
//! Path contract: receives absolute VFS paths and strips `self.prefix` itself.
//! Two strip modes mirror the pre-MountTable dispatch exactly:
//! - ops (`write`/`unlink`/…) require a NON-EMPTY relative path — operating on
//!   the mount root is rejected;
//! - `list`/`stat` accept the mount root (`/data` → volume root).
//!
//! fatfs uses interior mutability, so all operations go through `&self`
//! internally; `&mut self` on the trait is only for RamFS symmetry.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::backend::{fill_page, FsBackend, ListPage};
use crate::blk_router::DeviceRef;
use crate::block_stream::{BlockStream, CachedBlockStream, StreamError};
use crate::fat_extent_map::{ExtentCache, ExtentMap, FileKey};
use crate::fs_error::FsError;
use crate::page_cache::CacheBudget;
use alloc::sync::Arc;
use core::cell::RefCell;
use ostd::io::println;
use wire::ipc::vfs_dir_page::EntryKind;

/// Concrete `fatfs::FileSystem` type for the VirtIO FAT volume.
///
/// `NullTimeProvider` and `LossyOemCpConverter` are the fatfs defaults;
/// using them avoids needing a RTC or a UTF-8↔OEM code-page converter.
type DataFs =
    fatfs::FileSystem<CachedBlockStream, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;
/// Convenience alias for a FAT directory handle — avoids repeating the full generic.
type DataDir<'a> =
    fatfs::Dir<'a, CachedBlockStream, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;
type DataFile<'a> =
    fatfs::File<'a, CachedBlockStream, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;

pub struct FatBackend {
    fs: Option<DataFs>,
    prefix: Box<str>,
    /// Device the volume lives on; `sync` flushes only this device.
    dev: DeviceRef,
    /// The partition, for reads that bypass fatfs (see [`Self::read_mapped`]).
    volume: BlockStream,
    extents: RefCell<ExtentCache>,
}

/// Reads sector 0 of the FAT partition and returns `true` when the volume is
/// exFAT — identified by OEM-Name `"EXFAT   "` at BPB offset 3 (exFAT spec
/// §3.1). FAT12/16/32 never use this string.
///
/// A temporary `BlockStream` is used so the probe does not interfere with the
/// `CachedBlockStream` fatfs will create afterwards (positions are per-instance;
/// both start at offset 0).
fn probe_exfat(dev: DeviceRef, base_lba: u64, sectors: u64) -> bool {
    use fatfs::Read;
    let mut bs = BlockStream::new(dev, base_lba, sectors);
    let mut sec = [0u8; 512];
    // Guard against short reads — need at least 11 bytes for the OEM-Name field.
    match bs.read(&mut sec) {
        Ok(n) if n >= 11 => &sec[3..11] == b"EXFAT   ",
        _ => false,
    }
}

impl FatBackend {
    /// Mount the FAT volume whose partition begins at absolute `base_lba` on
    /// `dev`: `/mnt/sd` (P1, `PART_FAT32_BASE_LBA`) and the read-only `/bin`
    /// cell-store (P6, `PART_CELLSTORE_BASE_LBA`) on the primary disk, and
    /// `/mnt/disk/<name>` (P1 offset) on a secondary disk. On failure (bad BPB
    /// or exFAT) the backend stays in fallback mode — every op fails cleanly
    /// while other mounts keep working; see [`Self::is_mounted`]. Every sector
    /// fatfs touches is bounded to `sectors` from `base_lba`. The sector cache
    /// draws on `budget`, shared with every other FAT mount.
    pub fn mount(
        prefix: &str,
        dev: DeviceRef,
        base_lba: u64,
        sectors: u64,
        budget: CacheBudget,
    ) -> Self {
        if probe_exfat(dev, base_lba, sectors) {
            println("[vfs] exFAT volume detected — not supported; reformat to FAT32 or use /data (littlefs)");
            return Self {
                fs: None,
                prefix: prefix.into(),
                dev,
                volume: BlockStream::new(dev, base_lba, sectors),
                extents: RefCell::new(ExtentCache::new()),
            };
        }
        let opts = fatfs::FsOptions::new().update_accessed_date(false);
        let stream = CachedBlockStream::new(dev, base_lba, sectors, budget);
        let fs = match fatfs::FileSystem::new(stream, opts) {
            Ok(fs) => {
                // Include the mount prefix so every FAT volume is distinguishable
                // in the boot log.
                ostd::io::print("[vfs] FAT32 ");
                ostd::io::print(prefix);
                println(" volume mounted");
                for entry in dir_entries(&fs.root_dir()).flatten() {
                    let name = entry.file_name();
                    if name.eq_ignore_ascii_case("guest_disk.img")
                        || name.eq_ignore_ascii_case("guest.img")
                    {
                        println(&alloc::format!(
                            "[vfs-fat-mount] {} entry='{}' (sfn='{}') len={}",
                            prefix,
                            name,
                            entry.short_file_name(),
                            entry.len(),
                        ));
                    }
                }
                Some(fs)
            }
            Err(_) => {
                ostd::io::print("[vfs] WARNING: FAT32 ");
                ostd::io::print(prefix);
                println(" mount failed");
                None
            }
        };
        Self {
            fs,
            prefix: prefix.into(),
            dev,
            volume: BlockStream::new(dev, base_lba, sectors),
            extents: RefCell::new(ExtentCache::new()),
        }
    }

    /// Read file bytes through the file's cached [`ExtentMap`] straight from
    /// the device: no seek, no chain walk per read. `None` when the file
    /// cannot be mapped (missing, empty, or too fragmented) and fatfs must
    /// serve it; `Some(Err(Io))` when the device failed part way.
    fn read_mapped(
        &self,
        fs: &DataFs,
        rel: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Option<Result<usize, FsError>> {
        let mut file = fs.root_dir().open_file(rel).ok()?;
        let key = file_key(&mut file)?;
        let cached = self.extents.borrow_mut().get(key);
        let map = match cached {
            Some(entry) => entry?,
            None => {
                let map = map_file(&mut file).map(Arc::new);
                self.extents.borrow_mut().insert(key, map.clone());
                map?
            }
        };
        let mut total = 0usize;
        for (disk, len) in map.pieces(offset, buf.len() as u64) {
            let len = len as usize;
            if !self
                .volume
                .read_bytes_at(disk, &mut buf[total..total + len])
            {
                println(&alloc::format!(
                    "[vfs] read error path={} offset={} got={} want={}",
                    rel,
                    offset,
                    total,
                    buf.len()
                ));
                return Some(Err(FsError::Io));
            }
            total += len;
        }
        if total < buf.len() {
            println(&alloc::format!(
                "[vfs] short read path={} offset={} got={} want={} size={}",
                rel,
                offset,
                total,
                buf.len(),
                map.size()
            ));
        }
        Some(Ok(total))
    }

    /// Whether a FAT32 volume was opened.
    pub fn is_mounted(&self) -> bool {
        self.fs.is_some()
    }

    /// The mounted volume; `Unavailable` when the mount failed.
    fn volume_fs(&self) -> Result<&DataFs, FsError> {
        self.fs.as_ref().ok_or(FsError::Unavailable)
    }

    /// Strip the mount prefix; reject the mount root itself.
    /// `/data/x` → `x`; `/data` and `/data/` → `InvalidPath`.
    fn rel_nonempty<'a>(&self, path: &'a str) -> Result<&'a str, FsError> {
        path.strip_prefix(&*self.prefix)
            .and_then(|r| r.strip_prefix('/'))
            .filter(|r| !r.is_empty())
            .ok_or(FsError::InvalidPath)
    }

    /// Strip the mount prefix; the mount root maps to the volume root (`""`).
    fn rel_allow_root<'a>(&self, path: &'a str) -> &'a str {
        let r = path.strip_prefix(&*self.prefix).unwrap_or(path);
        r.strip_prefix('/').unwrap_or(r)
    }

    /// [`FsBackend::list_page`], leaving out entries for which `skip` is true
    /// (they take no cursor index). `BinOverlay` hides cell-store names that
    /// VIFS1 already lists this way.
    pub(crate) fn list_page_filtered(
        &self,
        path: &str,
        start: u32,
        out: &mut [u8],
        skip: &dyn Fn(&[u8]) -> bool,
    ) -> Result<ListPage, FsError> {
        let fs = self.volume_fs()?;
        let dir = open_dir_allow_root(fs, self.rel_allow_root(path))?;
        page_dir(&dir, start, out, skip)
    }
}

/// `rel` as a directory; `""` is the volume root.
fn open_dir_allow_root<'a>(fs: &'a DataFs, rel: &str) -> Result<DataDir<'a>, FsError> {
    let root = fs.root_dir();
    if rel.is_empty() {
        Ok(root)
    } else {
        root.open_dir(rel).map_err(dir_error)
    }
}

/// A device error under fatfs: whether it left a mutation's outcome unknown.
pub(crate) trait DeviceError {
    fn indeterminate(&self) -> bool;
}

impl DeviceError for StreamError {
    fn indeterminate(&self) -> bool {
        matches!(self, StreamError::Indeterminate)
    }
}

/// Host-test images: a failed read or write is never indeterminate.
#[cfg(test)]
impl DeviceError for () {
    fn indeterminate(&self) -> bool {
        false
    }
}

/// fatfs's error in VFS terms. `InvalidInput` means a path named the wrong
/// kind of entry; [`dir_error`] and [`file_error`] say which where it is known.
fn fat_error<E: DeviceError>(error: fatfs::Error<E>) -> FsError {
    if let fatfs::Error::Io(device) = &error {
        if device.indeterminate() {
            return FsError::Indeterminate;
        }
    }
    match error {
        fatfs::Error::NotFound => FsError::NotFound,
        fatfs::Error::AlreadyExists => FsError::AlreadyExists,
        fatfs::Error::DirectoryIsNotEmpty => FsError::DirectoryNotEmpty,
        fatfs::Error::NotEnoughSpace => FsError::NoSpace,
        fatfs::Error::InvalidInput
        | fatfs::Error::InvalidFileNameLength
        | fatfs::Error::UnsupportedFileNameCharacter => FsError::InvalidPath,
        // Device failures, a cluster chain shorter than its entry, corrupt
        // structures, and anything fatfs adds later.
        _ => FsError::Io,
    }
}

/// [`fat_error`] for an operation that needs a directory.
fn dir_error<E: DeviceError>(error: fatfs::Error<E>) -> FsError {
    match error {
        fatfs::Error::InvalidInput => FsError::NotADirectory,
        error => fat_error(error),
    }
}

/// [`fat_error`] for an operation that needs a regular file.
fn file_error<E: DeviceError>(error: fatfs::Error<E>) -> FsError {
    match error {
        fatfs::Error::InvalidInput => FsError::IsADirectory,
        error => fat_error(error),
    }
}

/// Read `file` from its position to end of file. A device failure part way
/// through is `Io`, never the bytes read so far.
pub(crate) fn read_to_end<IO, TP, OCC>(
    file: &mut fatfs::File<'_, IO, TP, OCC>,
) -> Result<Vec<u8>, FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
{
    use fatfs::Read as _;
    let mut buf = alloc::vec![0u8; 4096];
    let mut result = Vec::new();
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(result),
            Ok(n) => result.extend_from_slice(&buf[..n]),
            Err(_) => return Err(FsError::Io),
        }
    }
}

/// The file's [`FileKey`]: where its first cluster lies; `None` when empty.
fn file_key(file: &mut DataFile<'_>) -> Option<FileKey> {
    file.extents().next()?.ok().map(|extent| extent.offset)
}

/// Walk `file`'s cluster chain once into an [`ExtentMap`]; `None` when a FAT
/// read fails or the file is too fragmented to map.
fn map_file(file: &mut DataFile<'_>) -> Option<ExtentMap> {
    let mut failed = false;
    let map = ExtentMap::build(file.extents().map_while(|extent| match extent {
        Ok(extent) => Some((extent.offset, u64::from(extent.size))),
        Err(_) => {
            failed = true;
            None
        }
    }));
    if failed {
        return None;
    }
    map
}

/// Split `"sub/dir/file"` into `("sub/dir", "file")`. `"file"` → `("", "file")`.
fn split_last(rel: &str) -> (&str, &str) {
    match rel.rfind('/') {
        Some(i) => (&rel[..i], &rel[i + 1..]),
        None => ("", rel),
    }
}

/// Most entries one FAT directory can hold: 65,536 32-byte slots (FAT spec).
/// A walk that yields more is going round a looped cluster chain.
const MAX_DIR_ENTRIES: usize = 65_536;

/// Bounds of a recursive delete, as CellosFS's. A directory whose cluster
/// points back at an ancestor makes the tree endless, so it hits these.
const REMOVE_MAX_DEPTH: usize = 32;
const REMOVE_MAX_ENTRIES: usize = 4096;

/// `dir`'s entries in on-disk order, `.` and `..` included. A read failure,
/// or a walk past [`MAX_DIR_ENTRIES`], is `Io` and ends the walk.
fn dir_entries<'a, IO, TP, OCC>(
    dir: &fatfs::Dir<'a, IO, TP, OCC>,
) -> impl Iterator<Item = Result<fatfs::DirEntry<'a, IO, TP, OCC>, FsError>> + 'a
where
    IO: fatfs::ReadWriteSeek + 'a,
    TP: fatfs::TimeProvider + 'a,
    OCC: fatfs::OemCpConverter + 'a,
{
    dir.iter()
        .take(MAX_DIR_ENTRIES + 1)
        .enumerate()
        .map(|(index, entry)| match entry {
            Ok(_) if index == MAX_DIR_ENTRIES => Err(FsError::Io),
            Ok(entry) => Ok(entry),
            Err(_) => Err(FsError::Io),
        })
}

/// Page one FAT directory in on-disk order. An iteration error is the whole
/// page's error (`Io`), never an early end: fatfs ends its iterator after the
/// first error, which `list` used to report as a complete listing.
pub(crate) fn page_dir<IO, TP, OCC>(
    dir: &fatfs::Dir<'_, IO, TP, OCC>,
    start: u32,
    out: &mut [u8],
    skip: &dyn Fn(&[u8]) -> bool,
) -> Result<ListPage, FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
{
    let entries = dir_entries(dir).filter_map(|entry| {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        let name = entry.file_name();
        if name == "." || name == ".." || skip(name.as_bytes()) {
            return None;
        }
        let kind = if entry.is_dir() {
            EntryKind::Dir
        } else if entry.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        Some(Ok((kind, name)))
    });
    fill_page(entries, start, out)
}

/// Walk `parts` (a '/'-separated relative dir path) from `root`, creating any
/// missing component. Returns the leaf `Dir`, or the fatfs failure.
/// Empty `parts` returns `root` unchanged.
fn ensure_dir_chain<'a>(root: DataDir<'a>, parts: &str) -> Result<DataDir<'a>, FsError> {
    let mut cur = root;
    for part in parts.split('/').filter(|p| !p.is_empty()) {
        // create_dir is create-or-open → idempotent on existing dirs.
        cur = cur.create_dir(part).map_err(dir_error)?;
    }
    Ok(cur)
}

/// `name` in `dir` as a new empty file, replacing a regular file of that name.
/// A directory there is `IsADirectory` and is left in place, as on RamFS.
pub(crate) fn replace_file<'a, IO, TP, OCC>(
    dir: &fatfs::Dir<'a, IO, TP, OCC>,
    name: &str,
) -> Result<fatfs::File<'a, IO, TP, OCC>, FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
    IO::Error: DeviceError,
{
    if dir.open_dir(name).is_ok() {
        return Err(FsError::IsADirectory);
    }
    let _ = dir.remove(name);
    dir.create_file(name).map_err(file_error)
}

/// Delete `rel` (path relative to the FAT root) and everything below it.
///
/// The whole tree is walked first, with an explicit stack, and nothing is
/// removed unless the walk completes inside [`REMOVE_MAX_DEPTH`] and
/// [`REMOVE_MAX_ENTRIES`] (`Unsupported` otherwise) without a read error
/// (`Io`). Removal then runs children before parents.
pub(crate) fn remove_tree<IO, TP, OCC>(
    fs: &fatfs::FileSystem<IO, TP, OCC>,
    rel: &str,
) -> Result<(), FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
    IO::Error: DeviceError,
{
    let root = fs.root_dir();
    if root.open_dir(rel).is_err() {
        // `rel` is a file (or already gone) — remove it directly.
        return root.remove(rel).map_err(fat_error);
    }
    // Pre-order: a directory precedes everything below it, so the reverse is
    // a valid removal order.
    let mut order: Vec<String> = Vec::new();
    let mut pending: Vec<(String, usize)> = alloc::vec![(String::from(rel), 0)];
    let mut seen = 0usize;
    while let Some((path, depth)) = pending.pop() {
        let dir = root.open_dir(&path).map_err(dir_error)?;
        order.push(path.clone());
        for entry in dir_entries(&dir) {
            let entry = entry?;
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            seen += 1;
            if seen > REMOVE_MAX_ENTRIES {
                return Err(FsError::Unsupported);
            }
            let child = alloc::format!("{path}/{name}");
            if entry.is_dir() {
                if depth >= REMOVE_MAX_DEPTH {
                    return Err(FsError::Unsupported);
                }
                pending.push((child, depth + 1));
            } else {
                order.push(child);
            }
        }
    }
    for path in order.iter().rev() {
        root.remove(path).map_err(fat_error)?;
    }
    Ok(())
}

impl FsBackend for FatBackend {
    fn list(&self, path: &str, out: &mut [u8]) -> Result<usize, FsError> {
        let fs = self.volume_fs()?;
        let dir = open_dir_allow_root(fs, self.rel_allow_root(path))?;

        let mut pos = 0;
        for entry in dir_entries(&dir) {
            let e = entry?;
            let name = e.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let prefix: &[u8] = if e.is_dir() { b"d:" } else { b"f:" };
            let name_b = name.as_bytes();
            let entry_len = 2 + name_b.len() + 1;
            if pos + entry_len > out.len() {
                break;
            }
            out[pos..pos + 2].copy_from_slice(prefix);
            out[pos + 2..pos + 2 + name_b.len()].copy_from_slice(name_b);
            out[pos + 2 + name_b.len()] = b'\n';
            pos += entry_len;
        }
        Ok(pos)
    }

    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Result<ListPage, FsError> {
        self.list_page_filtered(path, start, out, &|_| false)
    }

    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        let fs = self.volume_fs()?;
        let rel = self.rel_allow_root(path);
        if rel.is_empty() {
            return Ok(Some((0, true)));
        } // mount root is a directory
        if rel.split('/').any(|c| c == "..") {
            return Ok(None);
        }
        // Seek(End) may clamp to a corrupt cluster chain's reachable bytes.
        // Stat must report the declared directory length so boot-config readers
        // can reject a valid TOML prefix from a truncated file.
        let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let root = fs.root_dir();
        let directory = if parent.is_empty() {
            root
        } else {
            match root.open_dir(parent) {
                Ok(directory) => directory,
                // A missing parent, or one that is a file, holds nothing.
                Err(fatfs::Error::NotFound | fatfs::Error::InvalidInput) => return Ok(None),
                Err(error) => return Err(fat_error(error)),
            }
        };
        for entry in dir_entries(&directory) {
            let entry = entry?;
            if entry.file_name().eq_ignore_ascii_case(name)
                || entry.short_file_name().eq_ignore_ascii_case(name)
            {
                return Ok(Some((entry.len(), entry.is_dir())));
            }
        }
        match fs.root_dir().open_dir(rel) {
            Ok(_) => Ok(Some((0, true))),
            Err(fatfs::Error::NotFound | fatfs::Error::InvalidInput) => Ok(None),
            Err(error) => Err(fat_error(error)),
        }
    }

    fn file_size(&self, path: &str) -> Result<u64, FsError> {
        match self.stat(path)? {
            Some((size, false)) => Ok(size),
            Some((_, true)) => Err(FsError::IsADirectory),
            None => Err(FsError::NotFound),
        }
    }

    fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, FsError> {
        let fs = self.volume_fs()?;
        let rel = self.rel_nonempty(path)?;
        let mut file = fs.root_dir().open_file(rel).map_err(file_error)?;
        read_to_end(&mut file)
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        use fatfs::{Read as _, Seek as _};
        let fs = self.volume_fs()?;
        let rel = self.rel_nonempty(path)?;
        if let Some(read) = self.read_mapped(fs, rel, offset, buf) {
            return read;
        }
        let mut file = fs.root_dir().open_file(rel).map_err(file_error)?;
        file.seek(fatfs::SeekFrom::Start(offset))
            .map_err(fat_error)?;
        let mut total = 0;
        while total < buf.len() {
            match file.read(&mut buf[total..]) {
                Ok(0) => {
                    // A short read is never normal for a file this size: the
                    // device model has to fail the request, so say what fatfs
                    // thought the file held.
                    let end = file.seek(fatfs::SeekFrom::End(0)).unwrap_or(u64::MAX);
                    println(&alloc::format!(
                        "[vfs] short read path={} offset={} got={} want={} fatfs_size={}",
                        rel,
                        offset,
                        total,
                        buf.len(),
                        end
                    ));
                    break;
                }
                Ok(n) => total += n,
                Err(_) => {
                    println(&alloc::format!(
                        "[vfs] read error path={} offset={} got={} want={}",
                        rel,
                        offset,
                        total,
                        buf.len()
                    ));
                    return Err(FsError::Io);
                }
            }
        }
        Ok(total)
    }

    /// Create-or-overwrite with mkdir -p on intermediate directories. Uses
    /// remove-then-create for truncate semantics without `seek(End)`.
    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        use fatfs::Write as _;
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let dir = ensure_dir_chain(fs.root_dir(), parent).inspect_err(|_| {
            println("[vfs-fat] write failed while preparing parent directories");
        })?;
        // Replacing the file frees its clusters for any other file to reuse.
        self.extents.get_mut().clear();
        let mut file = replace_file(&dir, name).inspect_err(|error| {
            println(&alloc::format!(
                "[vfs-fat] write failed while creating file: {error:?}"
            ));
        })?;
        file.write_all(content).map_err(|error| {
            println(&alloc::format!(
                "[vfs-fat] write failed while writing file data: {error:?}"
            ));
            fat_error(error)
        })
    }

    fn sync(&mut self, _path: &str) -> Result<(), FsError> {
        StreamError::of(crate::blk_router::blk_flush(self.dev))
            .map_err(|error| fat_error(fatfs::Error::Io(error)))
    }

    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> Result<(), FsError> {
        use fatfs::{Seek as _, Write as _};
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let dir = ensure_dir_chain(fs.root_dir(), parent)?;
        let mut file = dir.open_file(name).map_err(file_error)?;
        if let Some(key) = file_key(&mut file) {
            let end = offset.saturating_add(content.len() as u64);
            self.extents.get_mut().written(key, end);
        }
        file.seek(fatfs::SeekFrom::Start(offset))
            .map_err(fat_error)?;
        file.write_all(content).map_err(fat_error)
    }

    /// Append; creates the file (and parents) if absent — first append behaves
    /// like a write. `fatfs::File::seek(End(0))` translates to
    /// `disk.seek(Start(abs_end))` internally, so the `End` arm of
    /// `BlockStream::seek` (which errors) is never reached.
    fn append(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        use fatfs::{Seek as _, Write as _};
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        if rel.split('/').any(|c| c == "..") {
            return Err(FsError::InvalidPath);
        }
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let dir = ensure_dir_chain(fs.root_dir(), parent)?;
        let mut file = match dir.open_file(name) {
            Ok(f) => f,
            Err(_) => dir.create_file(name).map_err(file_error)?,
        };
        // An append always grows the file past its mapped size.
        if let Some(key) = file_key(&mut file) {
            self.extents.get_mut().written(key, u64::MAX);
        }
        file.seek(fatfs::SeekFrom::End(0)).map_err(fat_error)?;
        file.write_all(content).map_err(fat_error)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let fs = self.volume_fs()?;
        let rel = self.rel_nonempty(path)?;
        ensure_dir_chain(fs.root_dir(), rel).map(|_| ())
    }

    /// Remove an EMPTY directory. `open_dir` succeeds only for directories;
    /// `remove` errors on a non-empty dir — strict POSIX type checking.
    fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        // Reject any path component that is ".." — defense-in-depth even though
        // fatfs confines resolution to the volume root.
        if rel.split('/').any(|c| c == "..") {
            return Err(FsError::InvalidPath);
        }
        fs.root_dir().open_dir(rel).map_err(dir_error)?;
        fs.root_dir().remove(rel).map_err(fat_error)
    }

    /// Remove a regular FILE. `open_file` succeeds only for files in fatfs —
    /// acts as the type guard (use `rmdir` for directories).
    fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        fs.root_dir().open_file(rel).map_err(file_error)?;
        fs.root_dir().remove(rel).map_err(fat_error)
    }

    /// Recursive delete (`rm -r`). A path resolving to a regular file is
    /// removed directly.
    fn rmdir_recursive(&mut self, path: &str) -> Result<(), FsError> {
        let fs = self.fs.as_ref().ok_or(FsError::Unavailable)?;
        let rel = self.rel_nonempty(path)?;
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        // Defense-in-depth: reject ".." before the recursive delete amplifies any
        // path-confusion. fatfs also confines to the volume root, but explicit is safer.
        if rel.split('/').any(|c| c == "..") {
            return Err(FsError::InvalidPath);
        }
        remove_tree(fs, rel)
    }
}
