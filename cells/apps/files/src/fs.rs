// SPDX-License-Identifier: MIT
//! Files' data path: the VFS service through [`ostd::clients::VfsClient`], i.e. the same
//! typed `VfsRequest` family cellbox's file commands send (`cells/tools/cellbox/src/cmd_fs.rs`).
//! Nothing here parses CLI output or shares state with cellbox; parity is "same service,
//! same request types, same bounds".
//!
//! Operation semantics mirror cellbox:
//! - list = `ListDir` (`d:`/`f:` lines), sizes from `Stat`; preview = bounded read;
//! - copy = `Stat` + one bounded whole-file read (`cp`'s 1 MiB cap) + `Write`/`Append` in
//!   `cp`'s 400-byte chunks; folders are copied entry by entry like `cp -r`;
//! - move = `Rename` when both paths are under `/srv/` (the only renameable mount), otherwise,
//!   or when that rename is refused, copy and then remove the source (`mv`);
//! - remove = `Unlink` for files, `RmdirRecursive` for folders under `/data/` (`rm -r`) and
//!   `Rmdir` (empty folders only) elsewhere (`rmdir`);
//! - new folder = `Mkdir`; new file = empty `Write` when `Stat` finds nothing (`touch`).
//!
//! [`FileSystem`] is the seam between Files and the service: [`Vfs`] is the only production
//! implementation; host tests drive the same operations against an in-memory tree.

extern crate alloc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ostd::clients::VfsClient;
use ostd::{ViError, ViResult};

/// Largest file a copy reads: cellbox `copy_single_file` (`read_file_vfs_owned(src, 1 MiB)`).
pub const COPY_READ_MAX: usize = 1024 * 1024;
/// Bytes per `Write`/`Append` request: cellbox `vfs_write_chunked`.
pub const WRITE_CHUNK: usize = 400;
/// Largest file Quick Look reads: the `VfsClient::read_file` default cap.
pub const PREVIEW_READ_MAX: usize = 64 * 1024;

/// One `ListDir` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// The VFS requests Files issues, one method per typed `VfsRequest`.
pub trait FileSystem {
    fn list_dir(&mut self, path: &str) -> ViResult<Vec<DirEntry>>;
    fn stat(&mut self, path: &str) -> ViResult<(u64, bool)>;
    /// Whole-file read refusing files larger than `max` bytes.
    fn read_bounded(&mut self, path: &str, max: usize) -> ViResult<Vec<u8>>;
    fn mkdir(&mut self, path: &str) -> ViResult<()>;
    fn write(&mut self, path: &str, content: &[u8]) -> ViResult<()>;
    fn append(&mut self, path: &str, content: &[u8]) -> ViResult<()>;
    fn rename(&mut self, old: &str, new: &str) -> ViResult<()>;
    fn unlink(&mut self, path: &str) -> ViResult<()>;
    fn rmdir(&mut self, path: &str) -> ViResult<()>;
    fn rmdir_recursive(&mut self, path: &str) -> ViResult<()>;
}

/// The VFS service, reached through `VfsClient`.
pub struct Vfs(VfsClient);

impl Vfs {
    pub fn new() -> Self {
        Self(VfsClient::new())
    }
}

impl FileSystem for Vfs {
    fn list_dir(&mut self, path: &str) -> ViResult<Vec<DirEntry>> {
        parse_list(&self.0.list_dir(path)?)
    }
    fn stat(&mut self, path: &str) -> ViResult<(u64, bool)> {
        self.0.stat(path)
    }
    fn read_bounded(&mut self, path: &str, max: usize) -> ViResult<Vec<u8>> {
        self.0.read_file_bounded(path, max)
    }
    fn mkdir(&mut self, path: &str) -> ViResult<()> {
        self.0.mkdir(path)
    }
    fn write(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        self.0.write_file(path, content)
    }
    fn append(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        self.0.append_file(path, content)
    }
    fn rename(&mut self, old: &str, new: &str) -> ViResult<()> {
        self.0.rename(old, new)
    }
    fn unlink(&mut self, path: &str) -> ViResult<()> {
        self.0.unlink(path)
    }
    fn rmdir(&mut self, path: &str) -> ViResult<()> {
        self.0.rmdir(path)
    }
    fn rmdir_recursive(&mut self, path: &str) -> ViResult<()> {
        self.0.rmdir_recursive(path)
    }
}

/// Decode a `ListDir` reply: one `d:<name>` or `f:<name>` per line (the VFS wire format
/// cellbox's `vfs_list_dir_details` reads). Other lines are skipped.
pub fn parse_list(bytes: &[u8]) -> ViResult<Vec<DirEntry>> {
    let text = core::str::from_utf8(bytes).map_err(|_| ViError::IO)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let (is_dir, name) = if let Some(name) = line.strip_prefix("d:") {
                (true, name)
            } else {
                (false, line.strip_prefix("f:")?)
            };
            (!name.is_empty()).then(|| DirEntry { name: String::from(name), is_dir })
        })
        .collect())
}

/// Why a Files operation failed. VFS refusals are reported as returned, never masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    Vfs(ViError),
    /// The source exceeds the bound the matching cellbox command applies.
    TooLarge,
    /// A folder copy or move whose destination lies inside the source.
    IntoItself,
}

impl From<ViError> for FsError {
    fn from(e: ViError) -> Self {
        Self::Vfs(e)
    }
}

impl FsError {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Vfs(ViError::PermissionDenied) => "permission denied (VFS ERR_DENIED)",
            Self::Vfs(ViError::IO) => "I/O error (VFS ERR_IO)",
            Self::Vfs(ViError::OutOfMemory) => "quota or size limit exceeded (VFS ERR_QUOTA)",
            Self::Vfs(ViError::NotFound) => "not found (VFS ERR_HANDLE or no VFS service)",
            Self::Vfs(ViError::InvalidArgument) => "request does not fit the VFS limits",
            Self::Vfs(ViError::AlreadyExists) => "already exists",
            Self::Vfs(ViError::IsADirectory) => "is a folder",
            Self::Vfs(_) => "VFS error",
            Self::TooLarge => "larger than the 1 MiB copy limit",
            Self::IntoItself => "cannot copy a folder into itself",
        }
    }
}

pub type FsResult<T> = Result<T, FsError>;

/// `dir/name` without doubling the root slash.
pub fn join(dir: &str, name: &str) -> String {
    let mut out = String::with_capacity(dir.len() + 1 + name.len());
    out.push_str(dir.trim_end_matches('/'));
    out.push('/');
    out.push_str(name);
    out
}

/// Parent of an absolute path (`/` for top-level entries and for `/` itself).
pub fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => "/",
        Some(idx) => &path[..idx],
    }
}

/// Resolve user input against `current` into a canonical absolute path (`.`/`..` folded,
/// duplicate and trailing slashes dropped), as cellbox's `resolve_shell_path` does.
pub fn resolve(current: &str, input: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let base = if input.starts_with('/') { "" } else { current };
    for comp in base.split('/').chain(input.split('/')) {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = String::new();
    for p in &parts {
        out.push('/');
        out.push_str(p);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// A single new entry name: non-empty, no `/`, not `.` or `..`.
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        Err("a name is required")
    } else if name.contains('/') {
        Err("a name cannot contain '/'")
    } else if name == "." || name == ".." {
        Err("'.' and '..' are reserved")
    } else {
        Ok(())
    }
}

fn inside(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || (path.starts_with(ancestor) && path.as_bytes().get(ancestor.trim_end_matches('/').len()) == Some(&b'/'))
}

/// Read a whole regular file within `max` bytes: `Stat` decides kind and size, then one
/// bounded read whose length must equal that size (cellbox `read_file_vfs_owned`).
pub fn read_file<F: FileSystem>(fs: &mut F, path: &str, max: usize) -> FsResult<Vec<u8>> {
    let (size, is_dir) = fs.stat(path)?;
    if is_dir {
        return Err(ViError::IsADirectory.into());
    }
    let size = usize::try_from(size).map_err(|_| FsError::TooLarge)?;
    if size > max {
        return Err(FsError::TooLarge);
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    let data = fs.read_bounded(path, size)?;
    if data.len() != size {
        return Err(ViError::IO.into());
    }
    Ok(data)
}

/// Create or replace `path` with `data`: `Write` for the first chunk, `Append` for the rest.
pub fn write_chunked<F: FileSystem>(fs: &mut F, path: &str, data: &[u8]) -> FsResult<()> {
    let mut chunks = data.chunks(WRITE_CHUNK);
    fs.write(path, chunks.next().unwrap_or(&[]))?;
    for chunk in chunks {
        fs.append(path, chunk)?;
    }
    Ok(())
}

pub fn copy_file<F: FileSystem>(fs: &mut F, src: &str, dst: &str) -> FsResult<()> {
    let data = read_file(fs, src, COPY_READ_MAX)?;
    write_chunked(fs, dst, &data)
}

/// Copy a folder tree like `cp -r`: `Mkdir` each folder, copy each file. Iterative, so the
/// depth is bounded by what the VFS accepts as a path rather than by this cell's stack.
pub fn copy_tree<F: FileSystem>(fs: &mut F, src: &str, dst: &str) -> FsResult<()> {
    if inside(dst, src) {
        return Err(FsError::IntoItself);
    }
    let mut pending = vec![(String::from(src), String::from(dst))];
    while let Some((from, to)) = pending.pop() {
        fs.mkdir(&to)?;
        for entry in fs.list_dir(&from)? {
            let (child_from, child_to) = (join(&from, &entry.name), join(&to, &entry.name));
            if entry.is_dir {
                pending.push((child_from, child_to));
            } else {
                copy_file(fs, &child_from, &child_to)?;
            }
        }
    }
    Ok(())
}

pub fn copy_entry<F: FileSystem>(fs: &mut F, src: &str, dst: &str, is_dir: bool) -> FsResult<()> {
    if is_dir {
        copy_tree(fs, src, dst)
    } else {
        copy_file(fs, src, dst)
    }
}

pub fn remove_entry<F: FileSystem>(fs: &mut F, path: &str, is_dir: bool) -> FsResult<()> {
    if !is_dir {
        fs.unlink(path)?;
    } else if path.starts_with("/data/") {
        fs.rmdir_recursive(path)?;
    } else {
        fs.rmdir(path)?;
    }
    Ok(())
}

pub fn move_entry<F: FileSystem>(fs: &mut F, src: &str, dst: &str, is_dir: bool) -> FsResult<()> {
    if is_dir && inside(dst, src) {
        return Err(FsError::IntoItself);
    }
    if src.starts_with("/srv/") && dst.starts_with("/srv/") && fs.rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_entry(fs, src, dst, is_dir)?;
    remove_entry(fs, src, is_dir)
}

/// `touch` for a new name: refuse an existing entry, otherwise create it empty.
pub fn create_file<F: FileSystem>(fs: &mut F, path: &str) -> FsResult<()> {
    if fs.stat(path).is_ok() {
        return Err(ViError::AlreadyExists.into());
    }
    fs.write(path, &[])?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod fake {
    //! In-memory tree with the VFS service's observable contract: `ERR_IO` for missing
    //! entries, `ERR_DENIED` for refused prefixes, empty-only `Rmdir`, `/srv`-only rename,
    //! and the `RmdirRecursive`/`Write`/`Append` semantics of the RamFS backend.
    use super::*;
    use alloc::collections::BTreeMap;

    pub struct FakeFs {
        pub nodes: BTreeMap<String, Option<Vec<u8>>>, // None = folder
        pub denied: Vec<&'static str>,
        pub read_only: Vec<&'static str>,
    }

    impl FakeFs {
        pub fn new() -> Self {
            let mut nodes = BTreeMap::new();
            nodes.insert(String::from("/"), None);
            Self { nodes, denied: Vec::new(), read_only: Vec::new() }
        }
        pub fn dir(mut self, path: &str) -> Self {
            self.nodes.insert(String::from(path), None);
            self
        }
        pub fn file(mut self, path: &str, data: &[u8]) -> Self {
            self.nodes.insert(String::from(path), Some(data.to_vec()));
            self
        }
        fn readable(&self, path: &str) -> ViResult<()> {
            if self.denied.iter().any(|p| inside(path, p)) {
                return Err(ViError::PermissionDenied);
            }
            Ok(())
        }
        fn writable(&self, path: &str) -> ViResult<()> {
            self.readable(path)?;
            if self.read_only.iter().any(|p| inside(path, p)) {
                return Err(ViError::PermissionDenied);
            }
            Ok(())
        }
        fn parent_is_dir(&self, path: &str) -> bool {
            matches!(self.nodes.get(parent(path)), Some(None))
        }
        fn children(&self, path: &str) -> Vec<String> {
            self.nodes.keys().filter(|k| k.as_str() != path && parent(k) == path).cloned().collect()
        }
    }

    impl FileSystem for FakeFs {
        fn list_dir(&mut self, path: &str) -> ViResult<Vec<DirEntry>> {
            self.readable(path)?;
            if !matches!(self.nodes.get(path), Some(None)) {
                return Err(ViError::IO);
            }
            let mut wire = Vec::new();
            for child in self.children(path) {
                wire.extend_from_slice(if self.nodes[&child].is_none() { b"d:" } else { b"f:" });
                wire.extend_from_slice(child.rsplit('/').next().unwrap_or("").as_bytes());
                wire.push(b'\n');
            }
            parse_list(&wire)
        }
        fn stat(&mut self, path: &str) -> ViResult<(u64, bool)> {
            self.readable(path)?;
            match self.nodes.get(path) {
                Some(None) => Ok((0, true)),
                Some(Some(data)) => Ok((data.len() as u64, false)),
                None => Err(ViError::IO),
            }
        }
        fn read_bounded(&mut self, path: &str, max: usize) -> ViResult<Vec<u8>> {
            self.readable(path)?;
            match self.nodes.get(path) {
                Some(Some(data)) if data.len() > max => Err(ViError::OutOfMemory),
                Some(Some(data)) => Ok(data.clone()),
                _ => Err(ViError::IO),
            }
        }
        fn mkdir(&mut self, path: &str) -> ViResult<()> {
            self.writable(path)?;
            if self.nodes.contains_key(path) || !self.parent_is_dir(path) {
                return Err(ViError::IO);
            }
            self.nodes.insert(String::from(path), None);
            Ok(())
        }
        fn write(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
            self.writable(path)?;
            if matches!(self.nodes.get(path), Some(None)) || !self.parent_is_dir(path) {
                return Err(ViError::IO);
            }
            self.nodes.insert(String::from(path), Some(content.to_vec()));
            Ok(())
        }
        fn append(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
            self.writable(path)?;
            match self.nodes.get_mut(path) {
                Some(Some(data)) => {
                    data.extend_from_slice(content);
                    Ok(())
                }
                _ => Err(ViError::IO),
            }
        }
        fn rename(&mut self, old: &str, new: &str) -> ViResult<()> {
            self.writable(old)?;
            self.writable(new)?;
            if !old.starts_with("/srv/") || !new.starts_with("/srv/") {
                return Err(ViError::PermissionDenied);
            }
            match self.nodes.get(old) {
                Some(Some(_)) if !self.nodes.contains_key(new) && self.parent_is_dir(new) => {
                    let data = self.nodes.remove(old).unwrap();
                    self.nodes.insert(String::from(new), data);
                    Ok(())
                }
                _ => Err(ViError::IO),
            }
        }
        fn unlink(&mut self, path: &str) -> ViResult<()> {
            self.writable(path)?;
            match self.nodes.get(path) {
                Some(Some(_)) => {
                    self.nodes.remove(path);
                    Ok(())
                }
                _ => Err(ViError::IO),
            }
        }
        fn rmdir(&mut self, path: &str) -> ViResult<()> {
            self.writable(path)?;
            if !matches!(self.nodes.get(path), Some(None)) || !self.children(path).is_empty() {
                return Err(ViError::IO);
            }
            self.nodes.remove(path);
            Ok(())
        }
        fn rmdir_recursive(&mut self, path: &str) -> ViResult<()> {
            self.writable(path)?;
            if !matches!(self.nodes.get(path), Some(None)) {
                return Err(ViError::IO);
            }
            self.nodes.retain(|k, _| !inside(k, path));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeFs;
    use super::*;

    #[test]
    fn list_reply_decodes_kinds_and_skips_noise() {
        let entries = parse_list(b"d:docs\nf:a.txt\nx:junk\nf:\n").unwrap();
        assert_eq!(
            entries,
            vec![
                DirEntry { name: String::from("docs"), is_dir: true },
                DirEntry { name: String::from("a.txt"), is_dir: false },
            ]
        );
        assert_eq!(parse_list(&[0xff, 0xfe]), Err(ViError::IO));
    }

    #[test]
    fn paths_resolve_like_the_shell() {
        assert_eq!(join("/", "tmp"), "/tmp");
        assert_eq!(join("/tmp/", "a"), "/tmp/a");
        assert_eq!(parent("/tmp/a"), "/tmp");
        assert_eq!(parent("/tmp"), "/");
        assert_eq!(resolve("/data", "/tmp//c5dir/"), "/tmp/c5dir");
        assert_eq!(resolve("/tmp/c5dir", "../x/./y"), "/tmp/x/y");
        assert_eq!(resolve("/tmp", "../../.."), "/");
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("c5new").is_ok());
        assert!(inside("/tmp/a/b", "/tmp/a"));
        assert!(!inside("/tmp/ab", "/tmp/a"));
    }

    #[test]
    fn copy_round_trips_multi_chunk_content_and_respects_the_cp_bound() {
        let big: Vec<u8> = (0..(WRITE_CHUNK * 2 + 17)).map(|i| (i % 251) as u8).collect();
        let mut fs = FakeFs::new().dir("/tmp").file("/tmp/big.bin", &big);
        copy_file(&mut fs, "/tmp/big.bin", "/tmp/copy.bin").unwrap();
        assert_eq!(fs.nodes["/tmp/copy.bin"].as_deref(), Some(&big[..]));

        let mut fs = FakeFs::new().dir("/tmp").file("/tmp/huge", &vec![0u8; COPY_READ_MAX + 1]);
        assert_eq!(copy_file(&mut fs, "/tmp/huge", "/tmp/out"), Err(FsError::TooLarge));
        assert!(!fs.nodes.contains_key("/tmp/out"));
    }

    #[test]
    fn folder_copy_is_recursive_and_refuses_itself() {
        let mut fs = FakeFs::new()
            .dir("/tmp")
            .dir("/tmp/src")
            .dir("/tmp/src/sub")
            .file("/tmp/src/sub/x", b"x1")
            .file("/tmp/src/y", b"");
        copy_tree(&mut fs, "/tmp/src", "/tmp/dst").unwrap();
        assert_eq!(fs.nodes["/tmp/dst/sub/x"].as_deref(), Some(&b"x1"[..]));
        assert_eq!(fs.nodes["/tmp/dst/y"].as_deref(), Some(&b""[..]));
        assert_eq!(copy_tree(&mut fs, "/tmp/src", "/tmp/src/sub/inner"), Err(FsError::IntoItself));
        assert!(!fs.nodes.contains_key("/tmp/src/sub/inner"));
    }

    #[test]
    fn removal_follows_rm_and_rmdir_scope() {
        let mut fs = FakeFs::new()
            .dir("/tmp")
            .dir("/tmp/full")
            .file("/tmp/full/f", b"1")
            .dir("/data")
            .dir("/data/tree")
            .file("/data/tree/f", b"1");
        // Outside /data a folder is removed only when empty, exactly like `rmdir`.
        assert_eq!(remove_entry(&mut fs, "/tmp/full", true), Err(FsError::Vfs(ViError::IO)));
        assert!(fs.nodes.contains_key("/tmp/full/f"));
        remove_entry(&mut fs, "/data/tree", true).unwrap();
        assert!(!fs.nodes.keys().any(|k| k.starts_with("/data/tree")));
    }

    #[test]
    fn move_renames_only_on_srv_and_otherwise_copies_then_removes() {
        let mut fs = FakeFs::new()
            .dir("/srv")
            .dir("/srv/d")
            .file("/srv/a", b"A")
            .dir("/tmp")
            .file("/tmp/b", b"B");
        move_entry(&mut fs, "/srv/a", "/srv/d/a", false).unwrap();
        assert_eq!(fs.nodes["/srv/d/a"].as_deref(), Some(&b"A"[..]));
        assert!(!fs.nodes.contains_key("/srv/a"));
        move_entry(&mut fs, "/tmp/b", "/srv/d/b", false).unwrap();
        assert_eq!(fs.nodes["/srv/d/b"].as_deref(), Some(&b"B"[..]));
        assert!(!fs.nodes.contains_key("/tmp/b"));
    }

    #[test]
    fn refusals_surface_unchanged() {
        let mut fs = FakeFs::new().dir("/srv").dir("/srv/cellos").dir("/bin").file("/tmp", b"");
        fs.denied.push("/srv/cellos");
        fs.read_only.push("/bin");
        assert_eq!(fs.list_dir("/srv/cellos"), Err(ViError::PermissionDenied));
        assert_eq!(
            create_file(&mut fs, "/bin/new").map_err(FsError::describe),
            Err("permission denied (VFS ERR_DENIED)")
        );
        assert!(!fs.nodes.contains_key("/bin/new"));
        assert_eq!(create_file(&mut fs, "/tmp"), Err(FsError::Vfs(ViError::AlreadyExists)));
    }
}
