use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use api::vfs_file_handles::ViVfsFileHandle;

use super::owner_counts;
use crate::caller::Caller;

/// Most file handles one caller generation may hold at once.
pub const MAX_FILE_HANDLES_PER_CALLER: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Open,
    InFlightSyncRead,
    Tombstoned,
    Closed,
}

pub struct FileEntry {
    pub owner: Caller,
    pub path: String,
    pub parent_dir: u64,
    pub state: FileState,
    pub _lease: Option<crate::namespace::ServiceHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHandleError {
    UnknownHandle,
    TooManyHandles,
    Exhausted,
}

pub struct FileHandleTable {
    entries: BTreeMap<u64, FileEntry>,
    counts: BTreeMap<(u64, u64), usize>,
    next: u64,
    exhausted: bool,
}

impl FileHandleTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            counts: BTreeMap::new(),
            next: 1,
            exhausted: false,
        }
    }

    #[allow(dead_code)]
    pub fn insert(
        &mut self,
        owner: Caller,
        path: &str,
        parent_dir: u64,
    ) -> Result<ViVfsFileHandle, FileHandleError> {
        self.insert_leased(owner, path, parent_dir, None)
    }

    pub fn insert_leased(
        &mut self,
        owner: Caller,
        path: &str,
        parent_dir: u64,
        lease: Option<crate::namespace::ServiceHandle>,
    ) -> Result<ViVfsFileHandle, FileHandleError> {
        let key = owner_counts::key(owner);
        if self.counts.get(&key).copied().unwrap_or(0) >= MAX_FILE_HANDLES_PER_CALLER {
            return Err(FileHandleError::TooManyHandles);
        }
        if self.exhausted || self.next == 0 {
            return Err(FileHandleError::Exhausted);
        }

        let id = self.next;
        self.next = self.next.checked_add(1).unwrap_or_else(|| {
            self.exhausted = true;
            self.next
        });
        self.entries.insert(
            id,
            FileEntry {
                owner,
                path: path.to_string(),
                parent_dir,
                state: FileState::Open,
                _lease: lease,
            },
        );
        *self.counts.entry(key).or_insert(0) += 1;
        Ok(ViVfsFileHandle(id))
    }

    /// Resolve an open file handle for its owner without exposing entries held
    /// by another caller. Grant writes use this to recover the authorized path
    /// before resolving caller-controlled grant memory.
    pub fn path_of(&self, caller: Caller, file: ViVfsFileHandle) -> Option<&str> {
        self.entries
            .get(&file.0)
            .filter(|entry| entry.owner == caller && entry.state == FileState::Open)
            .map(|entry| entry.path.as_str())
    }

    pub fn begin_sync_read(
        &mut self,
        caller: Caller,
        file: ViVfsFileHandle,
    ) -> Result<String, FileHandleError> {
        let entry = self
            .entries
            .get_mut(&file.0)
            .filter(|entry| entry.owner == caller)
            .ok_or(FileHandleError::UnknownHandle)?;
        match entry.state {
            FileState::Open => {
                entry.state = FileState::InFlightSyncRead;
                Ok(entry.path.clone())
            }
            FileState::InFlightSyncRead | FileState::Tombstoned | FileState::Closed => {
                Err(FileHandleError::UnknownHandle)
            }
        }
    }

    pub fn finish_sync_read(&mut self, caller: Caller, file: ViVfsFileHandle) -> bool {
        let Some(entry) = self
            .entries
            .get_mut(&file.0)
            .filter(|entry| entry.owner == caller)
        else {
            return false;
        };
        if entry.state != FileState::InFlightSyncRead {
            return false;
        }
        entry.state = FileState::Open;
        true
    }

    pub fn close(&mut self, caller: Caller, file: ViVfsFileHandle) -> bool {
        self.remove_if_owned(caller, file.0, FileState::Closed)
    }

    pub fn revoke_by_parent_dirs(&mut self, dir_ids: &[u64]) -> usize {
        let doomed: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| dir_ids.contains(&entry.parent_dir))
            .map(|(id, _)| *id)
            .collect();
        self.remove_ids(&doomed, FileState::Tombstoned)
    }

    pub fn purge_owner(&mut self, caller: Caller) -> usize {
        let doomed: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.owner == caller)
            .map(|(id, _)| *id)
            .collect();
        self.remove_ids(&doomed, FileState::Closed)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn contains(&self, file: ViVfsFileHandle) -> bool {
        self.entries.contains_key(&file.0)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn held_by(&self, caller: Caller) -> usize {
        self.counts
            .get(&owner_counts::key(caller))
            .copied()
            .unwrap_or(0)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn set_next_for_test(&mut self, next: u64, exhausted: bool) {
        self.next = next;
        self.exhausted = exhausted;
    }

    fn remove_if_owned(&mut self, caller: Caller, id: u64, state: FileState) -> bool {
        let Some(entry) = self.entries.get(&id) else {
            return false;
        };
        if entry.owner != caller {
            return false;
        }
        self.remove_id(id, caller, state).is_some()
    }

    fn remove_ids(&mut self, ids: &[u64], state: FileState) -> usize {
        ids.iter()
            .filter_map(|id| {
                let owner = self.entries.get(id).map(|entry| entry.owner)?;
                self.remove_id(*id, owner, state)
            })
            .count()
    }

    fn remove_id(&mut self, id: u64, owner: Caller, state: FileState) -> Option<()> {
        let mut removed = self.entries.remove(&id)?;
        removed.state = state;
        owner_counts::decrement(&mut self.counts, owner);
        Some(())
    }
}

impl Default for FileHandleTable {
    fn default() -> Self {
        Self::new()
    }
}
