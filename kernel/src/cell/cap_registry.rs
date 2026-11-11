//! Backing storage for CSpace-managed BootFS files.
//!
//! Authority lives in the per-incarnation CSpace (`cell::cspace`); this module
//! only parks the open `ViFile` behind a CSpace ticket, keyed by the exact
//! `(cell_id, generation, ticket)` so a successor incarnation never reaches it.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;

/// A parked BootFS file behind its CSpace ticket.
type TicketFile = alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>;

static CSPACE_FILES: Spinlock<BTreeMap<(u64, u64, u64), Option<TicketFile>>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::CSPACE_FILES);

pub fn attach_file_to_ticket(key: (u64, u64, u64), file: TicketFile) {
    CSPACE_FILES.lock().insert(key, Some(file));
}

pub fn park_ticket_file(key: (u64, u64, u64)) -> Option<TicketFile> {
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

/// Whether a file, parked or not, is registered under `key`; retirement tests.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn has_ticket_file(key: (u64, u64, u64)) -> bool {
    CSPACE_FILES.lock().contains_key(&key)
}

/// Backing file with no content, for retirement tests.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) struct InertFile;

#[cfg(any(test, feature = "test-hooks"))]
impl api::fs::ViFile for InertFile {
    fn read(&mut self, _buf: &mut [u8]) -> types::ViResult<usize> {
        Err(types::ViError::NotSupported)
    }

    fn write(&mut self, _buf: &[u8]) -> types::ViResult<usize> {
        Err(types::ViError::NotSupported)
    }

    fn seek(&mut self, _pos: api::fs::SeekFrom) -> types::ViResult<u64> {
        Err(types::ViError::NotSupported)
    }

    fn read_async(
        self: alloc::boxed::Box<Self>,
        _buf_ptr: usize,
        _buf_len: usize,
    ) -> api::fs::BoxFuture<'static, api::fs::FileResult<usize>> {
        alloc::boxed::Box::pin(async move {
            let file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync> = self;
            (file, Err(types::ViError::NotSupported))
        })
    }
}
