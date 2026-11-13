//! The pin store: `/srv/.cellos-machine/pin` on CellosFS `/srv` (P5).
//!
//! The VFS admits only the live `MACHINE_PIN` provider (this Cell) to this
//! namespace, and its writes need the mutate flag like any other. No durability is claimed: a torn write reads back as an
//! unreadable record, which is safe mode, never an automatic re-pin.
//!
//! The VFS answers the same IO error for an absent path and for a failed
//! backend, so a failed `Stat` never proves absence. Absence is concluded only
//! from a successful, complete listing of the parent directory that lacks the
//! entry; every other failure is `Unavailable`, which never writes.

use alloc::vec::Vec;
use api::machine_facts::MACHINE_FACTS_CAPACITY;
use ostd::clients::VfsClient;
use ostd::{ViError, ViResult};

pub(crate) const PIN_DIR: &str = "/srv/.cellos-machine";
pub(crate) const PIN_PATH: &str = "/srv/.cellos-machine/pin";
const SRV: &str = "/srv";

/// A `ListDir` reply at least this long may have been cut at the reply
/// buffer, so it cannot prove that an entry is missing.
const LISTING_COMPLETE_BELOW: usize = 3_800;

/// What reading the pin found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PinRead {
    /// The file's bytes, at most `MACHINE_FACTS_CAPACITY`.
    Present(Vec<u8>),
    /// Definitive absence on a healthy store: a complete listing of the
    /// parent directory without the entry.
    Absent,
    /// A file larger than any record.
    Oversized,
    /// The store could not answer.
    Unavailable(&'static str),
}

/// Storage the gate needs. The VFS implements it; host tests use a fake.
pub(crate) trait PinStore {
    fn read(&mut self) -> PinRead;
    /// Write `bytes`, then read them back; `true` only on an exact match.
    fn write(&mut self, bytes: &[u8]) -> bool;
}

/// The VFS operations the store uses, so the absence rule is host-testable.
pub(crate) trait VfsOps {
    fn stat(&mut self, path: &str) -> ViResult<(u64, bool)>;
    fn list_dir(&mut self, path: &str) -> ViResult<Vec<u8>>;
    fn read_file(&mut self, path: &str, max: usize) -> ViResult<Vec<u8>>;
    fn mkdir(&mut self, path: &str) -> ViResult<()>;
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> ViResult<()>;
}

impl VfsOps for VfsClient {
    fn stat(&mut self, path: &str) -> ViResult<(u64, bool)> {
        VfsClient::stat(self, path)
    }
    fn list_dir(&mut self, path: &str) -> ViResult<Vec<u8>> {
        VfsClient::list_dir(self, path)
    }
    fn read_file(&mut self, path: &str, max: usize) -> ViResult<Vec<u8>> {
        self.read_file_bounded(path, max)
    }
    fn mkdir(&mut self, path: &str) -> ViResult<()> {
        VfsClient::mkdir(self, path)
    }
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> ViResult<()> {
        VfsClient::write_file(self, path, bytes)
    }
}

/// Whether a complete CellosFS `ListDir` reply (`f:`/`d:` kind prefix, then
/// the name, one per line) names `name` with either kind; `None` when the
/// reply may be truncated.
fn listing_names(listing: &[u8], name: &[u8]) -> Option<bool> {
    if listing.len() >= LISTING_COMPLETE_BELOW {
        return None;
    }
    Some(listing.split(|byte| *byte == b'\n').any(|entry| {
        entry.len() == name.len() + 2 && matches!(&entry[..2], b"f:" | b"d:") && &entry[2..] == name
    }))
}

/// The pin's state as seen through `ops`. Writes nothing.
pub(crate) fn read_pin(ops: &mut impl VfsOps) -> PinRead {
    // `/srv` answers as a directory only when its CellosFS volume opened.
    if !matches!(ops.stat(SRV), Ok((_, true))) {
        return PinRead::Unavailable("/srv not mounted");
    }
    match ops.stat(PIN_DIR) {
        Ok((_, true)) => {}
        Ok((_, false)) => return PinRead::Unavailable("pin directory is a file"),
        Err(_) => {
            return match ops
                .list_dir(SRV)
                .ok()
                .and_then(|names| listing_names(&names, b".cellos-machine"))
            {
                Some(false) => PinRead::Absent,
                Some(true) => PinRead::Unavailable("pin directory listed but not statable"),
                None => PinRead::Unavailable("/srv not listable"),
            };
        }
    }
    match ops
        .list_dir(PIN_DIR)
        .ok()
        .and_then(|names| listing_names(&names, b"pin"))
    {
        Some(false) => PinRead::Absent,
        Some(true) => match ops.read_file(PIN_PATH, MACHINE_FACTS_CAPACITY) {
            Ok(bytes) => PinRead::Present(bytes),
            Err(ViError::OutOfMemory) => PinRead::Oversized,
            Err(_) => PinRead::Unavailable("pin listed but unreadable"),
        },
        None => PinRead::Unavailable("pin directory unlistable"),
    }
}

/// Create the directory if needed, write `bytes`, read them back.
pub(crate) fn write_pin(ops: &mut impl VfsOps, bytes: &[u8]) -> bool {
    if !matches!(ops.stat(PIN_DIR), Ok((_, true))) && ops.mkdir(PIN_DIR).is_err() {
        return false;
    }
    ops.write_file(PIN_PATH, bytes).is_ok()
        && matches!(ops.read_file(PIN_PATH, MACHINE_FACTS_CAPACITY), Ok(back) if back == bytes)
}

/// [`PinStore`] over the VFS, as the live `MACHINE_PIN` provider.
pub(crate) struct VfsPinStore {
    vfs: VfsClient,
}

impl VfsPinStore {
    pub(crate) fn new() -> Self {
        Self {
            vfs: VfsClient::new(),
        }
    }
}

impl PinStore for VfsPinStore {
    fn read(&mut self) -> PinRead {
        read_pin(&mut self.vfs)
    }

    fn write(&mut self, bytes: &[u8]) -> bool {
        write_pin(&mut self.vfs, bytes)
    }
}
