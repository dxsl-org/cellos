//! In-memory [`Backend`] for host tests. Like FAT, a write past the end of a
//! file lands at its end, and a file handle names its path.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::backend::{Backend, Entry, Stat};
use super::errno::{EBADF, EEXIST, EIO, ENOENT, ENOTDIR};

pub enum Node {
    Dir,
    File(Vec<u8>),
}

pub struct Fake {
    /// Path (components joined by `/`) to node; the root is `""`.
    pub nodes: BTreeMap<String, Node>,
    pub fat: bool,
    pub attach_fails: bool,
    /// Files open now.
    pub open: usize,
    pub syncs: usize,
}

fn key(path: &[String]) -> String {
    path.join("/")
}

fn parent(k: &str) -> &str {
    k.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Fake {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(String::new(), Node::Dir);
        Self {
            nodes,
            fat: false,
            attach_fails: false,
            open: 0,
            syncs: 0,
        }
    }

    pub fn put(&mut self, path: &str, content: Option<&[u8]>) {
        let node = content.map_or(Node::Dir, |c| Node::File(c.to_vec()));
        self.nodes.insert(String::from(path), node);
    }

    pub fn content(&self, path: &str) -> Option<&[u8]> {
        match self.nodes.get(path) {
            Some(Node::File(data)) => Some(data),
            _ => None,
        }
    }

    fn file_mut(&mut self, k: &str) -> Result<&mut Vec<u8>, u32> {
        match self.nodes.get_mut(k) {
            Some(Node::File(data)) => Ok(data),
            _ => Err(EBADF),
        }
    }

    fn need_parent(&self, k: &str) -> Result<(), u32> {
        match self.nodes.get(parent(k)) {
            Some(Node::Dir) => Ok(()),
            Some(Node::File(_)) => Err(ENOTDIR),
            None => Err(ENOENT),
        }
    }
}

impl Backend for Fake {
    type File = String;

    fn attach(&mut self) -> Result<(), u32> {
        if self.attach_fails {
            Err(EIO)
        } else {
            Ok(())
        }
    }

    fn fat_names(&self) -> bool {
        self.fat
    }

    fn stat(&mut self, path: &[String]) -> Result<Stat, u32> {
        match self.nodes.get(&key(path)) {
            Some(Node::Dir) => Ok(Stat {
                is_dir: true,
                size: 0,
            }),
            Some(Node::File(data)) => Ok(Stat {
                is_dir: false,
                size: data.len() as u64,
            }),
            None => Err(ENOENT),
        }
    }

    fn list(&mut self, dir: &[String]) -> Result<Vec<Entry>, u32> {
        let k = key(dir);
        Ok(self
            .nodes
            .iter()
            .filter(|(other, _)| !other.is_empty() && parent(other) == k)
            .map(|(other, node)| Entry {
                name: String::from(other.rsplit('/').next().unwrap_or(other)),
                is_dir: matches!(node, Node::Dir),
            })
            .collect())
    }

    fn truncate(&mut self, path: &[String]) -> Result<(), u32> {
        let k = key(path);
        self.need_parent(&k)?;
        self.nodes.insert(k, Node::File(Vec::new()));
        Ok(())
    }

    fn open(&mut self, path: &[String]) -> Result<String, u32> {
        let k = key(path);
        self.file_mut(&k).map_err(|_| ENOENT)?;
        self.open += 1;
        Ok(k)
    }

    fn close(&mut self, _file: String) {
        self.open -= 1;
    }

    fn size(&mut self, file: &String) -> Result<u64, u32> {
        Ok(self.file_mut(file)?.len() as u64)
    }

    fn read(&mut self, file: &String, offset: u64, out: &mut [u8]) -> Result<usize, u32> {
        let data = self.file_mut(file)?;
        let start = (offset as usize).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&mut self, file: &String, offset: u64, bytes: &[u8]) -> Result<(), u32> {
        let data = self.file_mut(file)?;
        let start = (offset as usize).min(data.len());
        let end = start + bytes.len();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    fn sync(&mut self, file: &String) -> Result<(), u32> {
        self.file_mut(file)?;
        self.syncs += 1;
        Ok(())
    }

    fn mkdir(&mut self, path: &[String]) -> Result<(), u32> {
        let k = key(path);
        self.need_parent(&k)?;
        if self.nodes.contains_key(&k) {
            return Err(EEXIST);
        }
        self.nodes.insert(k, Node::Dir);
        Ok(())
    }

    fn remove(&mut self, path: &[String], _is_dir: bool) -> Result<(), u32> {
        self.nodes.remove(&key(path)).map(drop).ok_or(ENOENT)
    }

    fn rename(&mut self, old: &[String], new: &[String]) -> Result<(), u32> {
        let (old, new) = (key(old), key(new));
        self.need_parent(&new)?;
        let below = |k: &str| k == old || k.starts_with(&alloc::format!("{old}/"));
        let moving: Vec<String> = self.nodes.keys().filter(|k| below(k)).cloned().collect();
        for k in moving {
            let node = self.nodes.remove(&k).ok_or(EIO)?;
            let moved = alloc::format!("{new}{}", &k[old.len()..]);
            self.nodes.insert(moved, node);
        }
        Ok(())
    }
}
