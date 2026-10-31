//! Backing storage for CSpace-managed BootFS files.
//!
//! Authority lives in the per-incarnation CSpace (`cell::cspace`); this module
//! only parks the open `ViFile` behind a CSpace ticket, keyed by the exact
//! `(cell_id, generation, ticket)` so a successor incarnation never reaches it.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;

static CSPACE_FILES: Spinlock<BTreeMap<(u64, u64, u64), Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>>>> =
    Spinlock::new(BTreeMap::new());

pub fn attach_file_to_ticket(
    key: (u64, u64, u64),
    file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
) {
    CSPACE_FILES.lock().insert(key, Some(file));
}

pub fn park_ticket_file(
    key: (u64, u64, u64),
) -> Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>> {
    CSPACE_FILES.lock().get_mut(&key).and_then(|opt| opt.take())
}

pub fn unpark_ticket_file(
    key: (u64, u64, u64),
    file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
) {
    if let Some(opt) = CSPACE_FILES.lock().get_mut(&key) {
        *opt = Some(file);
    }
}

pub fn remove_ticket_file(key: (u64, u64, u64)) {
    CSPACE_FILES.lock().remove(&key);
}
