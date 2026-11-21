//! In-memory RamFS backend: read-only root catalog plus the volatile
//! read-write `/tmp` scratch space.
//!
//! Write/append are structurally restricted to `/tmp/` — the root catalog is
//! immutable by design. mkdir/rmdir/unlink operate anywhere in the tree;
//! the dispatcher's AccessTable is the authorization layer above this.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::backend::{fill_page, FsBackend, ListPage};
use crate::fs_error::FsError;
use wire::ipc::vfs_dir_page::EntryKind;
#[cfg(feature = "c2c-oracle-k1-fixture")]
const ORACLE_CLUSTER_KEY: &[u8; 32] =
    include_bytes!(concat!(env!("OUT_DIR"), "/c2c-oracle-cluster.key"));

#[derive(Clone)]
struct RamFile {
    data: Vec<u8>,
    is_dir: bool,
    children: BTreeMap<String, Box<RamFile>>,
}

impl RamFile {
    fn new_file(data: &[u8]) -> Self {
        Self {
            data: Vec::from(data),
            is_dir: false,
            children: BTreeMap::new(),
        }
    }
    fn new_dir() -> Self {
        Self {
            data: Vec::new(),
            is_dir: true,
            children: BTreeMap::new(),
        }
    }

    /// Nodes and bytes below this node, walked without recursion.
    fn usage_below(&self) -> Usage {
        let mut usage = Usage::default();
        let mut pending: Vec<&RamFile> = alloc::vec![self];
        while let Some(node) = pending.pop() {
            for (name, child) in &node.children {
                usage.nodes += 1;
                usage.bytes += name.len() + child.data.len();
                pending.push(child);
            }
        }
        usage
    }
}

/// Most nodes (files and directories) RamFS holds: each costs heap every
/// client shares, whatever its size, and the quota charges only a minimum.
const MAX_NODES: usize = 4096;

/// Most bytes of names and file content RamFS holds. Kept well under the VFS
/// heap: the per-cell quota is larger than the heap, so it cannot be the bound.
const MAX_BYTES: usize = crate::VFS_HEAP_BYTES / 4;

/// What a tree holds, counted against [`MAX_NODES`] and [`MAX_BYTES`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Usage {
    nodes: usize,
    bytes: usize,
}

impl Usage {
    /// `self` after adding `nodes` nodes and `added` bytes and dropping `freed`
    /// bytes; `NoSpace` past either bound.
    fn admit(self, nodes: usize, added: usize, freed: usize) -> Result<Self, FsError> {
        let next = Self {
            nodes: self.nodes.saturating_add(nodes),
            bytes: self.bytes.saturating_sub(freed).saturating_add(added),
        };
        if next.nodes > MAX_NODES || next.bytes > MAX_BYTES {
            return Err(FsError::NoSpace);
        }
        Ok(next)
    }

    /// `self` after removing the node `name` and everything below it.
    fn release(self, name: &str, node: &RamFile) -> Self {
        let below = node.usage_below();
        Self {
            nodes: self.nodes.saturating_sub(1 + below.nodes),
            bytes: self
                .bytes
                .saturating_sub(name.len() + node.data.len() + below.bytes),
        }
    }
}

pub struct RamFsBackend {
    root: Box<RamFile>,
    usage: Usage,
}

impl RamFsBackend {
    pub fn new() -> Self {
        let mut root = Box::new(RamFile::new_dir());
        root.children.insert(
            String::from("readme.txt"),
            Box::new(RamFile::new_file(b"Welcome to Cellos!\n")),
        );
        // httpd's web root: network clients reach files only below /www.
        let mut www = Box::new(RamFile::new_dir());
        www.children.insert(
            String::from("readme.txt"),
            Box::new(RamFile::new_file(b"Welcome to Cellos!\n")),
        );
        root.children.insert(String::from("www"), www);
        // /bin is served by BootFsProxy (kernel initramfs) since Phase 02 —
        // the embedded ELF copies that used to live here doubled every /bin
        // binary in RAM (once in kernel_fs.img, once in the VFS cell image).
        root.children
            .insert(String::from("tmp"), Box::new(RamFile::new_dir()));

        // The default build has no /etc/cellos tree and therefore no K1.
        // Only the isolated benchmark-oracle feature can compile this seed.
        #[cfg(feature = "c2c-oracle-k1-fixture")]
        {
            let mut etc = Box::new(RamFile::new_dir());
            let mut cellos = Box::new(RamFile::new_dir());
            cellos.children.insert(
                String::from("cluster.key"),
                Box::new(RamFile::new_file(ORACLE_CLUSTER_KEY)),
            );
            etc.children.insert(String::from("cellos"), cellos);
            root.children.insert(String::from("etc"), etc);
        }

        let usage = root.usage_below();
        Self { root, usage }
    }

    #[cfg(test)]
    pub(crate) fn seed_test_file(&mut self, path: &str, content: &[u8]) -> bool {
        self.write_node(path, content).is_ok()
    }

    fn find_node(&self, path: &str) -> Option<&RamFile> {
        if path == "/" {
            return Some(&self.root);
        }
        let mut cur = &self.root;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            cur = cur.children.get(part)?;
        }
        Some(cur)
    }

    fn find_node_mut(&mut self, path: &str) -> Option<&mut RamFile> {
        if path == "/" {
            return Some(&mut self.root);
        }
        let mut cur: &mut RamFile = &mut self.root;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            cur = cur.children.get_mut(part)?.as_mut();
        }
        Some(cur)
    }

    /// Split `path` into (parent_path, child_name). Returns `None` for root "/".
    fn split_parent_name(path: &str) -> Option<(String, String)> {
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return None;
        }
        let slash = path.rfind('/')?;
        let parent = if slash == 0 {
            String::from("/")
        } else {
            String::from(&path[..slash])
        };
        let name = String::from(&path[slash + 1..]);
        if name.is_empty() {
            return None;
        }
        Some((parent, name))
    }

    fn get_file_data(&self, path: &str) -> Result<&[u8], FsError> {
        let n = self.find_node(path).ok_or(FsError::NotFound)?;
        if n.is_dir {
            return Err(FsError::IsADirectory);
        }
        Ok(&n.data)
    }

    /// The directory that holds `path`'s last component, and that component.
    fn parent_mut(&mut self, path: &str) -> Result<(&mut RamFile, String), FsError> {
        let (parent_path, name) = Self::split_parent_name(path).ok_or(FsError::InvalidPath)?;
        let parent = self.find_node_mut(&parent_path).ok_or(FsError::NotFound)?;
        if !parent.is_dir {
            return Err(FsError::NotADirectory);
        }
        Ok((parent, name))
    }

    /// Create or overwrite a regular file. Fails if the parent does not exist
    /// or `path` names an existing directory.
    fn write_node(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        self.put_file(
            path,
            true,
            |_| Some(content.len()),
            |data| {
                *data = Vec::from(content);
            },
        )
    }

    /// Resize the regular file at `path` to `new_len(old_len)` bytes and let
    /// `fill` write them, creating the file first when `create` allows. The
    /// change is admitted against the RamFS bounds before anything moves;
    /// `None` from `new_len` is an unrepresentable length.
    fn put_file(
        &mut self,
        path: &str,
        create: bool,
        new_len: impl FnOnce(usize) -> Option<usize>,
        fill: impl FnOnce(&mut Vec<u8>),
    ) -> Result<(), FsError> {
        let usage = self.usage;
        let (parent, name) = self.parent_mut(path)?;
        let (nodes, old_len) = match parent.children.get(&name) {
            Some(existing) if existing.is_dir => return Err(FsError::IsADirectory),
            Some(existing) => (0, existing.data.len()),
            None if create => (1, 0),
            None => return Err(FsError::NotFound),
        };
        let new_len = new_len(old_len).ok_or(FsError::InvalidPath)?;
        let name_len = if nodes == 1 { name.len() } else { 0 };
        let next = usage.admit(nodes, name_len.saturating_add(new_len), old_len)?;
        let file = parent
            .children
            .entry(name)
            .or_insert_with(|| Box::new(RamFile::new_file(&[])));
        fill(&mut file.data);
        self.usage = next;
        Ok(())
    }

    /// Remove `path`'s last component once `check` accepts the node, and
    /// release what it held.
    fn detach(
        &mut self,
        path: &str,
        check: impl FnOnce(&RamFile) -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        let usage = self.usage;
        let (parent, name) = self.parent_mut(path)?;
        check(parent.children.get(&name).ok_or(FsError::NotFound)?)?;
        let Some(removed) = parent.children.remove(&name) else {
            return Err(FsError::NotFound);
        };
        self.usage = usage.release(&name, &removed);
        Ok(())
    }

    fn dir_node(&self, path: &str) -> Result<&RamFile, FsError> {
        match self.find_node(path) {
            Some(n) if n.is_dir => Ok(n),
            Some(_) => Err(FsError::NotADirectory),
            None => Err(FsError::NotFound),
        }
    }

    #[cfg(feature = "c2c-oracle-k1-fixture")]
    fn is_oracle_fixture_path(path: &str) -> bool {
        let mut parts = path.split('/').filter(|part| !part.is_empty());
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some("etc"), None, None, None)
            | (Some("etc"), Some("cellos"), None, None)
            | (Some("etc"), Some("cellos"), Some("cluster.key"), None) => true,
            _ => false,
        }
    }

    #[cfg(not(feature = "c2c-oracle-k1-fixture"))]
    fn is_oracle_fixture_path(_path: &str) -> bool {
        false
    }
}

impl FsBackend for RamFsBackend {
    fn list(&self, path: &str, out: &mut [u8]) -> Result<usize, FsError> {
        let node = self.dir_node(path)?;
        // Historical 480-byte cap (pre-MountTable dispatch staged through a
        // 480-byte temp buffer) — kept so reply sizes stay bit-identical.
        let cap = out.len().min(480);
        let mut pos = 0;
        for (name, child) in node.children.iter() {
            let prefix: &[u8] = if child.is_dir { b"d:" } else { b"f:" };
            let name_b = name.as_bytes();
            let entry_len = prefix.len() + name_b.len() + 1; // +1 for '\n'
            if pos + entry_len > cap {
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
        let node = self.dir_node(path)?;
        let entries = node.children.iter().map(|(name, child)| {
            let kind = if child.is_dir {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            Ok((kind, name.as_bytes()))
        });
        fill_page(entries, start, out)
    }

    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        Ok(self
            .find_node(path)
            .map(|n| (n.data.len() as u64, n.is_dir)))
    }

    fn file_size(&self, path: &str) -> Result<u64, FsError> {
        self.get_file_data(path).map(|d| d.len() as u64)
    }

    fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, FsError> {
        self.get_file_data(path).map(|d| d.to_vec())
    }

    /// Positional read out of the in-memory node.
    ///
    /// Required: the handle-read path (`ReadFileHandle`) is ranged, and a backend that leaves this
    /// at the trait default hands every caller an error — failing every file the shell writes
    /// under `/tmp` and reads back.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let data = self.get_file_data(path)?;
        let Ok(start) = usize::try_from(offset) else {
            return Ok(0);
        };
        if start >= data.len() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        // Embedded catalog is immutable; only the volatile scratch space accepts writes.
        if !path.starts_with("/tmp/") {
            return Err(FsError::Unsupported);
        }
        self.write_node(path, content)
    }

    fn append(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        if !path.starts_with("/tmp/") {
            return Err(FsError::Unsupported);
        }
        self.put_file(
            path,
            true,
            |old| old.checked_add(content.len()),
            |data| data.extend_from_slice(content),
        )
    }

    /// Positional write into an existing `/tmp` file; a gap past the end reads
    /// back as zeros.
    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> Result<(), FsError> {
        if !path.starts_with("/tmp/") {
            return Err(FsError::Unsupported);
        }
        let start = usize::try_from(offset).map_err(|_| FsError::InvalidPath)?;
        let end = start
            .checked_add(content.len())
            .ok_or(FsError::InvalidPath)?;
        self.put_file(
            path,
            false,
            |old| Some(old.max(end)),
            |data| {
                if data.len() < end {
                    data.reserve_exact(end - data.len());
                    data.resize(end, 0);
                }
                data[start..end].copy_from_slice(content);
            },
        )
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let usage = self.usage;
        let (parent, name) = self.parent_mut(path)?;
        if parent.children.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }
        let next = usage.admit(1, name.len(), 0)?;
        parent.children.insert(name, Box::new(RamFile::new_dir()));
        self.usage = next;
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        self.detach(path, |node| match node {
            c if !c.is_dir => Err(FsError::NotADirectory),
            c if !c.children.is_empty() => Err(FsError::DirectoryNotEmpty),
            _ => Ok(()),
        })
    }

    fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        if Self::is_oracle_fixture_path(path) {
            return Err(FsError::Unsupported);
        }
        self.detach(path, |node| {
            if node.is_dir {
                Err(FsError::IsADirectory)
            } else {
                Ok(())
            }
        })
    }

    fn rmdir_recursive(&mut self, path: &str) -> Result<(), FsError> {
        if Self::is_oracle_fixture_path(path) {
            return Err(FsError::Unsupported);
        }
        self.detach(path, |node| {
            if node.is_dir {
                Ok(())
            } else {
                Err(FsError::NotADirectory)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_count_is_bounded_and_removal_returns_room() {
        let mut ram = RamFsBackend::new();
        let room = MAX_NODES - ram.usage.nodes;
        for i in 0..room {
            assert_eq!(ram.write(&alloc::format!("/tmp/{i}"), b""), Ok(()), "{i}");
        }
        assert_eq!(ram.write("/tmp/one-more", b""), Err(FsError::NoSpace));
        assert_eq!(ram.mkdir("/tmp/one-more"), Err(FsError::NoSpace));
        assert_eq!(ram.unlink("/tmp/0"), Ok(()));
        assert_eq!(ram.mkdir("/tmp/d"), Ok(()));
        assert_eq!(ram.mkdir("/tmp/d/e"), Err(FsError::NoSpace));
    }

    #[test]
    fn byte_total_is_bounded_across_writes_appends_and_positional_writes() {
        let mut ram = RamFsBackend::new();
        let chunk = alloc::vec![7u8; MAX_BYTES / 4];
        for name in ["a", "b", "c"] {
            assert_eq!(ram.write(&alloc::format!("/tmp/{name}"), &chunk), Ok(()));
        }
        // The fourth quarter no longer fits beside the seeded catalog.
        assert_eq!(ram.write("/tmp/d", &chunk), Err(FsError::NoSpace));
        assert_eq!(ram.append("/tmp/a", &chunk), Err(FsError::NoSpace));
        assert_eq!(
            ram.write_at("/tmp/a", (MAX_BYTES / 2) as u64, b"x"),
            Err(FsError::NoSpace)
        );
        assert_eq!(
            ram.stat("/tmp/a"),
            Ok(Some(((MAX_BYTES / 4) as u64, false)))
        );
        // Overwriting in place is charged the difference only.
        assert_eq!(ram.write("/tmp/a", &chunk), Ok(()));
        // A recursive delete returns everything below it.
        assert_eq!(ram.mkdir("/tmp/tree"), Ok(()));
        assert_eq!(ram.write("/tmp/tree/big", &chunk[..1024]), Ok(()));
        let before = ram.usage;
        assert_eq!(ram.rmdir_recursive("/tmp/tree"), Ok(()));
        assert_eq!(ram.usage.nodes, before.nodes - 2);
        assert_eq!(ram.usage, ram.root.usage_below());
        assert_eq!(ram.unlink("/tmp/b"), Ok(()));
        assert_eq!(ram.write("/tmp/d", &chunk), Ok(()));
        assert_eq!(ram.usage, ram.root.usage_below());
    }

    #[test]
    fn positional_writes_patch_extend_and_need_an_existing_file() {
        let mut ram = RamFsBackend::new();
        assert_eq!(ram.write_at("/tmp/f", 0, b"x"), Err(FsError::NotFound));
        assert_eq!(ram.write("/tmp/f", b"hello"), Ok(()));
        assert_eq!(ram.write_at("/tmp/f", 1, b"EL"), Ok(()));
        assert_eq!(ram.write_at("/tmp/f", 7, b"!"), Ok(()));
        assert_eq!(ram.read_to_vec("/tmp/f").as_deref(), Ok(&b"hELlo\0\0!"[..]));
        assert_eq!(
            ram.write_at("/readme.txt", 0, b"x"),
            Err(FsError::Unsupported)
        );
        assert_eq!(ram.mkdir("/tmp/dir"), Ok(()));
        assert_eq!(
            ram.write_at("/tmp/dir", 0, b"x"),
            Err(FsError::IsADirectory)
        );
        assert_eq!(ram.usage, ram.root.usage_below());
    }
}
