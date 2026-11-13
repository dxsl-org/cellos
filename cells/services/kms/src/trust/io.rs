//! The store through the VFS client: `Observe`, a bounded read, and the D03
//! conditional save (`save_stage` / `save_commit`) under the KMS namespace.

extern crate alloc;

use alloc::vec::Vec;

use api::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome};
use ostd::clients::vfs::VfsClient;

use super::TrustIo;

const SRV_CELLOS: &str = "/srv/cellos";
/// Directory of the store: the KMS namespace.
const STORE_DIR: &str = "/srv/cellos/kms";
/// The store (seeded by `scripts/phase09_trust_disk.py`).
const STORE_PATH: &str = "/srv/cellos/kms/ssh_known_hosts";

pub(crate) struct VfsTrustIo {
    client: VfsClient,
}

impl VfsTrustIo {
    /// Resolution is lazy: nothing is sent before the first trust request.
    pub(crate) fn new() -> Self {
        Self {
            client: VfsClient::new(),
        }
    }
}

impl TrustIo for VfsTrustIo {
    fn observe(&mut self) -> Result<Observation, ()> {
        self.client.observe(STORE_PATH).map_err(drop)
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, ()> {
        self.client.read_file_bounded(STORE_PATH, max).map_err(drop)
    }

    fn ensure_dir(&mut self) -> Result<(), ()> {
        for path in [SRV_CELLOS, STORE_DIR] {
            match self.client.stat(path) {
                Ok((_, true)) => {}
                // A file in the way.
                Ok((_, false)) => return Err(()),
                Err(_) => self.client.mkdir(path).map_err(drop)?,
            }
        }
        Ok(())
    }

    fn publish(&mut self, bytes: &[u8], expect: SaveExpect) -> SaveOutcome {
        let sha256 = ostd::session::sha256(bytes);
        let Ok(id) = self.client.save_stage(STORE_PATH, bytes, sha256) else {
            return SaveOutcome::NotPublished(save_refusal::REFUSED);
        };
        // A lost commit reply proves nothing either way: ask about this same transaction
        // once, never commit again.
        let outcome = self
            .client
            .save_commit(id, expect)
            .or_else(|_| self.client.save_query(id))
            .unwrap_or(SaveOutcome::Indeterminate);
        let _ = self.client.save_abort(id);
        outcome
    }
}
