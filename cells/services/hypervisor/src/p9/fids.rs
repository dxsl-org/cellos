//! The fids of one session and the qid paths of the files they name.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::backend::Entry;
use super::codec::{Qid, QT_DIR, QT_FILE};
use super::errno::{EBADF, EMFILE};

/// Most fids one session holds.
pub const MAX_FIDS: usize = 256;
/// Paths given a counted qid path; later ones get a hashed one.
const MAX_QIDS: usize = 4096;

pub struct Fid<F> {
    /// Components below the share root; empty is the root.
    pub path: Vec<String>,
    pub is_dir: bool,
    /// Tlopen or Tlcreate succeeded.
    pub opened: bool,
    /// The open regular file of an opened file fid.
    pub file: Option<F>,
    /// Written through `file` since its last sync.
    pub dirty: bool,
    /// Treaddir's snapshot of the directory, taken at offset 0: its
    /// indices are the cookies the guest passes back.
    pub listing: Option<Vec<Entry>>,
}

impl<F> Fid<F> {
    pub fn new(path: Vec<String>, is_dir: bool) -> Self {
        Self {
            path,
            is_dir,
            opened: false,
            file: None,
            dirty: false,
            listing: None,
        }
    }
}

pub struct FidTable<F> {
    fids: BTreeMap<u32, Fid<F>>,
}

impl<F> FidTable<F> {
    pub const fn new() -> Self {
        Self {
            fids: BTreeMap::new(),
        }
    }

    pub fn contains(&self, fid: u32) -> bool {
        self.fids.contains_key(&fid)
    }

    pub fn get(&self, fid: u32) -> Result<&Fid<F>, u32> {
        self.fids.get(&fid).ok_or(EBADF)
    }

    pub fn get_mut(&mut self, fid: u32) -> Result<&mut Fid<F>, u32> {
        self.fids.get_mut(&fid).ok_or(EBADF)
    }

    /// Add `fid`, which must not be in use.
    pub fn insert(&mut self, fid: u32, entry: Fid<F>) -> Result<(), u32> {
        if self.fids.contains_key(&fid) {
            return Err(EBADF);
        }
        if self.fids.len() >= MAX_FIDS {
            return Err(EMFILE);
        }
        self.fids.insert(fid, entry);
        Ok(())
    }

    pub fn remove(&mut self, fid: u32) -> Option<Fid<F>> {
        self.fids.remove(&fid)
    }

    pub fn drain(&mut self) -> Vec<Fid<F>> {
        core::mem::take(&mut self.fids).into_values().collect()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Fid<F>> {
        self.fids.values_mut()
    }
}

/// Qid paths per file path: FAT has no inode, so a path keeps the number it
/// was first seen with, and a rename carries it along.
pub struct QidTable {
    ids: BTreeMap<String, u64>,
    next: u64,
}

fn key(path: &[String]) -> String {
    path.join("/")
}

/// Whether key `k` is `prefix` or below it.
fn under(k: &str, prefix: &str) -> bool {
    k == prefix
        || (k.len() > prefix.len() && k.starts_with(prefix) && k.as_bytes()[prefix.len()] == b'/')
}

impl QidTable {
    pub const fn new() -> Self {
        Self {
            ids: BTreeMap::new(),
            next: 1,
        }
    }

    pub fn qid(&mut self, path: &[String], is_dir: bool) -> Qid {
        let k = key(path);
        let id = match self.ids.get(&k) {
            Some(&id) => id,
            None if self.ids.len() < MAX_QIDS => {
                let id = self.next;
                self.next += 1;
                self.ids.insert(k, id);
                id
            }
            // FNV-1a, top bit set so it never meets a counted number.
            None => {
                k.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
                }) | 1 << 63
            }
        };
        Qid {
            kind: if is_dir { QT_DIR } else { QT_FILE },
            version: 0,
            path: id,
        }
    }

    /// `path` and everything below it is gone.
    pub fn forget(&mut self, path: &[String]) {
        let k = key(path);
        self.ids.retain(|other, _| !under(other, &k));
    }

    /// `old` and everything below it is now at `new`; whatever was at `new`
    /// was replaced.
    pub fn moved(&mut self, old: &[String], new: &[String]) {
        self.forget(new);
        let (old, new) = (key(old), key(new));
        let moving: Vec<(String, u64)> = self
            .ids
            .iter()
            .filter(|(k, _)| under(k, &old))
            .map(|(k, &id)| (k.clone(), id))
            .collect();
        for (k, id) in moving {
            self.ids.remove(&k);
            let mut renamed = new.clone();
            renamed.push_str(&k[old.len()..]);
            self.ids.insert(renamed, id);
        }
    }
}
