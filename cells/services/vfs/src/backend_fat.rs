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
use crate::block_stream::{BlockStream, CachedBlockStream};
use crate::fat_extent_map::{ExtentCache, ExtentMap, FileKey};
use alloc::sync::Arc;
use core::cell::RefCell;
use api::ipc::vfs_dir_page::EntryKind;
use ostd::io::println;

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
    /// fatfs touches is bounded to `sectors` from `base_lba`.
    pub fn mount(prefix: &str, dev: DeviceRef, base_lba: u64, sectors: u64) -> Self {
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
        let fs = match fatfs::FileSystem::new(CachedBlockStream::new(dev, base_lba, sectors), opts)
        {
            Ok(fs) => {
                // Include the mount prefix so every FAT volume is distinguishable
                // in the boot log.
                ostd::io::print("[vfs] FAT32 ");
                ostd::io::print(prefix);
                println(" volume mounted");
                for entry in fs.root_dir().iter().flatten() {
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
    /// serve it.
    fn read_mapped(&self, fs: &DataFs, rel: &str, offset: u64, buf: &mut [u8]) -> Option<usize> {
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
            if !self.volume.read_bytes_at(disk, &mut buf[total..total + len]) {
                break;
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
        Some(total)
    }

    /// Whether a FAT32 volume was opened.
    pub fn is_mounted(&self) -> bool {
        self.fs.is_some()
    }

    /// Strip the mount prefix; reject the mount root itself.
    /// `/data/x` → `x`; `/data` and `/data/` → None.
    fn rel_nonempty<'a>(&self, path: &'a str) -> Option<&'a str> {
        let r = path.strip_prefix(&*self.prefix)?.strip_prefix('/')?;
        if r.is_empty() {
            None
        } else {
            Some(r)
        }
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
    ) -> Option<ListPage> {
        let fs = self.fs.as_ref()?;
        let rel = self.rel_allow_root(path);
        let root = fs.root_dir();
        let dir = if rel.is_empty() {
            root
        } else {
            root.open_dir(rel).ok()?
        };
        page_dir(&dir, start, out, skip)
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

/// Page one FAT directory in on-disk order. An iteration error is the whole
/// page's error, never an early end: fatfs ends its iterator after the first
/// error, which `list` used to report as a complete listing.
pub(crate) fn page_dir<IO, TP, OCC>(
    dir: &fatfs::Dir<'_, IO, TP, OCC>,
    start: u32,
    out: &mut [u8],
    skip: &dyn Fn(&[u8]) -> bool,
) -> Option<ListPage>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
{
    let entries = dir.iter().filter_map(|entry| {
        let Ok(entry) = entry else {
            return Some(Err(()));
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
/// missing component. Returns the leaf `Dir`, or `Err(())` on fatfs failure.
/// Empty `parts` returns `root` unchanged.
fn ensure_dir_chain<'a>(root: DataDir<'a>, parts: &str) -> Result<DataDir<'a>, ()> {
    let mut cur = root;
    for part in parts.split('/').filter(|p| !p.is_empty()) {
        // create_dir is create-or-open → idempotent on existing dirs.
        cur = cur.create_dir(part).map_err(|_| ())?;
    }
    Ok(cur)
}

/// Depth-first delete of `rel` (path relative to the FAT root).
///
/// Rebuilds `root_dir()` per level and addresses children by full relative path
/// so no `Dir` handle is held across a recursive call (borrow-checker safe).
/// Collects `iter()` entries before mutating — avoids iterator-vs-mutation aliasing.
fn remove_tree(fs: &DataFs, rel: &str) -> bool {
    let dir = match fs.root_dir().open_dir(rel) {
        Ok(d) => d,
        // `rel` is a file (or already gone) — remove it directly.
        Err(_) => return fs.root_dir().remove(rel).is_ok(),
    };
    // Collect (name, is_dir) so the iterator borrow is released before we mutate.
    let entries: Vec<(String, bool)> = dir
        .iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            if name == "." || name == ".." {
                None
            } else {
                Some((name, e.is_dir()))
            }
        })
        .collect();
    drop(dir);

    for (name, is_dir) in &entries {
        let child = alloc::format!("{}/{}", rel, name);
        let ok = if *is_dir {
            remove_tree(fs, &child)
        } else {
            fs.root_dir().remove(&child).is_ok()
        };
        if !ok {
            return false;
        }
    }
    fs.root_dir().remove(rel).is_ok()
}

impl FsBackend for FatBackend {
    fn get_file_ptr(&self, _path: &str) -> Option<(usize, usize)> {
        None // disk-backed: no stable in-memory pointer — callers use the copy path
    }

    fn list(&self, path: &str, out: &mut [u8]) -> usize {
        let fs = match &self.fs {
            Some(f) => f,
            None => return 0,
        };
        let rel = self.rel_allow_root(path);
        let dir = if rel.is_empty() {
            fs.root_dir()
        } else {
            match fs.root_dir().open_dir(rel) {
                Ok(d) => d,
                Err(_) => return 0,
            }
        };

        let mut pos = 0;
        for entry in dir.iter() {
            let e = match entry {
                Ok(e) => e,
                Err(_) => break,
            };
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
        pos
    }

    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Option<ListPage> {
        self.list_page_filtered(path, start, out, &|_| false)
    }

    fn stat(&self, path: &str) -> Option<(u64, bool)> {
        let fs = self.fs.as_ref()?;
        let rel = self.rel_allow_root(path);
        if rel.is_empty() {
            return Some((0, true));
        } // mount root is a directory
        if rel.split('/').any(|c| c == "..") {
            return None;
        }
        // Seek(End) may clamp to a corrupt cluster chain's reachable bytes.
        // Stat must report the declared directory length so boot-config readers
        // can reject a valid TOML prefix from a truncated file.
        let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let root = fs.root_dir();
        let directory = if parent.is_empty() {
            root
        } else {
            root.open_dir(parent).ok()?
        };
        for entry in directory.iter() {
            let entry = entry.ok()?;
            if entry.file_name().eq_ignore_ascii_case(name)
                || entry.short_file_name().eq_ignore_ascii_case(name)
            {
                return Some((entry.len(), entry.is_dir()));
            }
        }
        if fs.root_dir().open_dir(rel).is_ok() {
            return Some((0, true));
        }
        None
    }

    fn file_size(&self, path: &str) -> u64 {
        self.stat(path)
            .filter(|(_, is_dir)| !is_dir)
            .map_or(0, |(size, _)| size)
    }

    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        use fatfs::Read as _;
        let fs = match &self.fs {
            Some(f) => f,
            None => return Vec::new(),
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return Vec::new(),
        };
        let mut file = match fs.root_dir().open_file(rel) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut buf = alloc::vec![0u8; 4096];
        let mut result = Vec::new();
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => result.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        result
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        use fatfs::{Read as _, Seek as _};
        let fs = match &self.fs {
            Some(f) => f,
            None => return 0,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return 0,
        };
        if let Some(read) = self.read_mapped(fs, rel, offset, buf) {
            return read;
        }
        let mut file = match fs.root_dir().open_file(rel) {
            Ok(f) => f,
            Err(_) => return 0,
        };
        if file.seek(fatfs::SeekFrom::Start(offset)).is_err() {
            return 0;
        }
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
                    break;
                }
            }
        }
        total
    }

    /// Create-or-overwrite with mkdir -p on intermediate directories. Uses
    /// remove-then-create for truncate semantics without `seek(End)`.
    fn write(&mut self, path: &str, content: &[u8]) -> bool {
        use fatfs::Write as _;
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return false;
        }
        let dir = match ensure_dir_chain(fs.root_dir(), parent) {
            Ok(d) => d,
            Err(()) => {
                println("[vfs-fat] write failed while preparing parent directories");
                return false;
            }
        };
        // Replacing the file frees its clusters for any other file to reuse.
        self.extents.get_mut().clear();
        let _ = dir.remove(name);
        let mut file = match dir.create_file(name) {
            Ok(f) => f,
            Err(error) => {
                println(&alloc::format!(
                    "[vfs-fat] write failed while creating file: {error:?}"
                ));
                return false;
            }
        };
        match file.write_all(content) {
            Ok(()) => true,
            Err(error) => {
                println(&alloc::format!(
                    "[vfs-fat] write failed while writing file data: {error:?}"
                ));
                false
            }
        }
    }

    fn sync(&mut self, _path: &str) -> bool {
        crate::blk_router::blk_flush(self.dev).is_completed()
    }

    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        use fatfs::{Seek as _, Write as _};
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return false;
        }
        let dir = match ensure_dir_chain(fs.root_dir(), parent) {
            Ok(d) => d,
            Err(()) => return false,
        };
        let mut file = match dir.open_file(name) {
            Ok(f) => f,
            Err(_) => return false,
        };
        if let Some(key) = file_key(&mut file) {
            let end = offset.saturating_add(content.len() as u64);
            self.extents.get_mut().written(key, end);
        }
        if file.seek(fatfs::SeekFrom::Start(offset)).is_err() {
            return false;
        }
        file.write_all(content).is_ok()
    }

    /// Append; creates the file (and parents) if absent — first append behaves
    /// like a write. `fatfs::File::seek(End(0))` translates to
    /// `disk.seek(Start(abs_end))` internally, so the `End` arm of
    /// `BlockStream::seek` (which errors) is never reached.
    fn append(&mut self, path: &str, content: &[u8]) -> bool {
        use fatfs::{Seek as _, Write as _};
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        if rel.split('/').any(|c| c == "..") {
            return false;
        }
        let (parent, name) = split_last(rel);
        if name.is_empty() {
            return false;
        }
        let dir = match ensure_dir_chain(fs.root_dir(), parent) {
            Ok(d) => d,
            Err(()) => return false,
        };
        let mut file = match dir.open_file(name) {
            Ok(f) => f,
            Err(_) => match dir.create_file(name) {
                Ok(f) => f,
                Err(_) => return false,
            },
        };
        // An append always grows the file past its mapped size.
        if let Some(key) = file_key(&mut file) {
            self.extents.get_mut().written(key, u64::MAX);
        }
        if file.seek(fatfs::SeekFrom::End(0)).is_err() {
            return false;
        }
        file.write_all(content).is_ok()
    }

    fn mkdir(&mut self, path: &str) -> bool {
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        ensure_dir_chain(fs.root_dir(), rel).is_ok()
    }

    /// Remove an EMPTY directory. `open_dir` succeeds only for directories;
    /// `remove` errors on a non-empty dir — strict POSIX type checking.
    fn rmdir(&mut self, path: &str) -> bool {
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        // Reject any path component that is ".." — defense-in-depth even though
        // fatfs confines resolution to the volume root.
        if rel.split('/').any(|c| c == "..") {
            return false;
        }
        if fs.root_dir().open_dir(rel).is_err() {
            return false;
        }
        fs.root_dir().remove(rel).is_ok()
    }

    /// Remove a regular FILE. `open_file` succeeds only for files in fatfs —
    /// acts as the type guard (use `rmdir` for directories).
    fn unlink(&mut self, path: &str) -> bool {
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        if fs.root_dir().open_file(rel).is_err() {
            return false;
        }
        fs.root_dir().remove(rel).is_ok()
    }

    /// Recursive delete (`rm -r`). A path resolving to a regular file is
    /// removed directly.
    fn rmdir_recursive(&mut self, path: &str) -> bool {
        let fs = match &self.fs {
            Some(f) => f,
            None => return false,
        };
        let rel = match self.rel_nonempty(path) {
            Some(r) => r,
            None => return false,
        };
        // Freed clusters may be reused by any file's next write.
        self.extents.get_mut().clear();
        // Defense-in-depth: reject ".." before the recursive delete amplifies any
        // path-confusion. fatfs also confines to the volume root, but explicit is safer.
        if rel.split('/').any(|c| c == "..") {
            return false;
        }
        remove_tree(fs, rel)
    }
}
