//! Directory handles, and the service's authority over them.
//!
//! A handle is an entry in this table naming one resolved directory and the cell
//! allowed to use it. Nothing outside this table gives a handle meaning: the
//! kernel carries handle values between cells and states where they came from,
//! but it cannot tell a live handle from a revoked one, so every question about
//! what a handle permits is answered here.
//!
//! ## Why a cell cannot name what it does not hold
//! A handle-addressed request carries a directory and one component. The
//! component is checked as raw bytes before anything is joined to it
//! (`api::dir_name`), so it cannot contain a separator or be `..`. The resolved
//! path is therefore always exactly one level below a directory the caller was
//! given. There is no path outside the handle the caller can express, which is
//! the difference between this and an access check.
//!
//! Lifetime, sealing and revocation live in [`lifecycle`]; binding an inherited
//! set to a child lives in [`bind`].

pub mod bind;
pub mod lifecycle;

use alloc::collections::BTreeMap;
use alloc::string::String;
use api::dir_handles::ViDirHandle;
use api::dir_name::{join_component, validate_dir_component, DirNameError};

use crate::caller::Caller;

/// Most handles one cell may hold at once.
///
/// Bounds the table against a cell that opens directories in a loop. Well above
/// `api::dir_handles::MAX_SPAWN_DIR_HANDLES`, so a cell can always derive from
/// everything it inherited.
pub const MAX_HANDLES_PER_CELL: usize = 32;

/// One issued handle.
pub(crate) struct DirEntry {
    pub(crate) owner: Caller,
    /// Absolute directory this handle refers to, already validated.
    pub(crate) path: String,
    /// Handle this one was derived from; `None` for a directly acquired root.
    /// Revoking the parent revokes this entry with it.
    pub(crate) parent: Option<u64>,
}

/// Per-cell state that outlives any individual handle.
pub(crate) struct CellState {
    pub(crate) generation: u64,
    /// Set once and never cleared: path-addressed requests are refused from
    /// here on.
    pub(crate) sealed: bool,
    /// Whether the kernel's provenance record for this cell has been consumed.
    /// One attempt per cell generation, successful or not — a record that could
    /// be re-read is a record that could be bound twice.
    pub(crate) attested: bool,
    pub(crate) handles: usize,
}

/// Why a handle-addressed request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirError {
    /// No such handle, or one belonging to another cell. The two are
    /// deliberately the same answer: sweeping handle values must reveal nothing
    /// about what other cells hold.
    UnknownHandle,
    /// The name may not be resolved inside a directory.
    BadName(DirNameError),
    /// The caller holds [`MAX_HANDLES_PER_CELL`] already.
    TooManyHandles,
}

/// The service's handle table.
pub struct DirTable {
    pub(crate) entries: BTreeMap<u64, DirEntry>,
    pub(crate) cells: BTreeMap<u64, CellState>,
    next: u64,
}

impl DirTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            cells: BTreeMap::new(),
            // 0 is never a handle: it is what a zeroed spawn carrier holds, and
            // issuing it would make "no handle" and "this handle" the same value.
            next: 1,
        }
    }

    /// Issue a handle for an already-validated absolute directory path.
    ///
    /// # Errors
    /// [`DirError::TooManyHandles`] when the caller is at its limit, and
    /// [`DirError::UnknownHandle`] for a caller with no registered state, which
    /// cannot happen after `on_contact`.
    pub fn open_root(&mut self, caller: Caller, path: &str) -> Result<ViDirHandle, DirError> {
        self.insert(caller, String::from(path), None)
    }

    /// The directory `dir` refers to, if `caller` holds it.
    pub fn dir_path(&self, caller: Caller, dir: ViDirHandle) -> Option<&str> {
        self.owned(caller, dir).map(|e| e.path.as_str())
    }

    /// The absolute path of `name` inside `dir`.
    ///
    /// # Errors
    /// [`DirError::UnknownHandle`] or [`DirError::BadName`]; the name is checked
    /// as raw bytes before anything is joined, so a refusal happens before the
    /// backend sees a string at all.
    pub fn resolve(
        &self,
        caller: Caller,
        dir: ViDirHandle,
        name: &str,
    ) -> Result<String, DirError> {
        let entry = self.owned(caller, dir).ok_or(DirError::UnknownHandle)?;
        let checked = validate_dir_component(name.as_bytes()).map_err(DirError::BadName)?;
        Ok(join_component(&entry.path, checked))
    }

    /// How many handles `caller` holds.
    pub fn held_by(&self, caller: Caller) -> usize {
        self.state(caller).map_or(0, |s| s.handles)
    }

    pub(crate) fn owned(&self, caller: Caller, dir: ViDirHandle) -> Option<&DirEntry> {
        self.entries.get(&dir.0).filter(|e| e.owner == caller)
    }

    pub(crate) fn insert(
        &mut self,
        owner: Caller,
        path: String,
        parent: Option<u64>,
    ) -> Result<ViDirHandle, DirError> {
        let state = self.state_mut(owner).ok_or(DirError::UnknownHandle)?;
        if state.handles >= MAX_HANDLES_PER_CELL {
            return Err(DirError::TooManyHandles);
        }
        state.handles += 1;
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.entries.insert(
            id,
            DirEntry {
                owner,
                path,
                parent,
            },
        );
        Ok(ViDirHandle(id))
    }
}

impl Default for DirTable {
    fn default() -> Self {
        Self::new()
    }
}
