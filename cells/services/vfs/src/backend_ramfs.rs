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
}

pub struct RamFsBackend {
    root: Box<RamFile>,
}

impl RamFsBackend {
    pub fn new() -> Self {
        let mut root = Box::new(RamFile::new_dir());
        root.children.insert(
            String::from("readme.txt"),
            Box::new(RamFile::new_file(b"Welcome to Cellos!\n")),
        );
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

        Self { root }
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
        let (parent, name) = self.parent_mut(path)?;
        match parent.children.get_mut(&name) {
            Some(existing) if existing.is_dir => Err(FsError::IsADirectory),
            Some(existing) => {
                existing.data = Vec::from(content);
                Ok(())
            }
            None => {
                parent
                    .children
                    .insert(name, Box::new(RamFile::new_file(content)));
                Ok(())
            }
        }
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
        let mut data = match self.get_file_data(path) {
            Ok(d) => d.to_vec(),
            Err(FsError::NotFound) => Vec::new(),
            Err(error) => return Err(error),
        };
        data.extend_from_slice(content);
        self.write_node(path, &data)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let (parent, name) = self.parent_mut(path)?;
        if parent.children.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }
        parent.children.insert(name, Box::new(RamFile::new_dir()));
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        let (parent, name) = self.parent_mut(path)?;
        match parent.children.get(&name) {
            None => Err(FsError::NotFound),
            Some(c) if !c.is_dir => Err(FsError::NotADirectory),
            Some(c) if !c.children.is_empty() => Err(FsError::DirectoryNotEmpty),
            Some(_) => {
                parent.children.remove(&name);
                Ok(())
            }
        }
    }

    fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        if Self::is_oracle_fixture_path(path) {
            return Err(FsError::Unsupported);
        }
        let (parent, name) = self.parent_mut(path)?;
        match parent.children.get(&name) {
            None => Err(FsError::NotFound),
            Some(c) if c.is_dir => Err(FsError::IsADirectory),
            Some(_) => {
                parent.children.remove(&name);
                Ok(())
            }
        }
    }

    fn rmdir_recursive(&mut self, path: &str) -> Result<(), FsError> {
        if Self::is_oracle_fixture_path(path) {
            return Err(FsError::Unsupported);
        }
        let (parent, name) = self.parent_mut(path)?;
        match parent.children.get(&name) {
            None => Err(FsError::NotFound),
            Some(c) if !c.is_dir => Err(FsError::NotADirectory),
            Some(_) => {
                parent.children.remove(&name);
                Ok(())
            }
        }
    }
}
