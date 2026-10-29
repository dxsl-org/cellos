//! [`Store`] over the VFS D03 protocol.

use api::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome};
use ostd::clients::VfsClient;
use ostd::ViError;

use super::editor::{SaveResult, Store};

pub struct VfsStore {
    vfs: VfsClient,
}

impl VfsStore {
    pub fn new() -> Self {
        Self { vfs: VfsClient::new() }
    }
}

fn reason(code: u8) -> &'static str {
    match code {
        save_refusal::STAGE_CHANGED => "staged copy changed",
        save_refusal::NOT_SEALED => "staging incomplete",
        save_refusal::QUOTA => "quota exceeded",
        _ => "refused by storage",
    }
}

impl Store for VfsStore {
    fn observe(&mut self, path: &str) -> Result<Observation, ViError> {
        self.vfs.observe(path)
    }

    fn save(&mut self, path: &str, bytes: &[u8], expect: SaveExpect) -> SaveResult {
        let id = match self.vfs.save_stage(path, bytes, ostd::session::sha256(bytes)) {
            Ok(id) => id,
            Err(error) => return SaveResult::Failed(error),
        };
        // A lost commit reply proves nothing either way: ask about this same
        // transaction once, never commit again.
        let outcome = self.vfs.save_commit(id, expect).or_else(|_| self.vfs.save_query(id));
        let result = match outcome {
            Ok(SaveOutcome::Committed { inode, generation } | SaveOutcome::CommittedDurable { inode, generation }) => {
                SaveResult::Committed(SaveExpect::Present { inode, generation })
            }
            Ok(SaveOutcome::Conflict) => SaveResult::Conflict,
            Ok(SaveOutcome::NotPublished(code)) => SaveResult::NotPublished(reason(code)),
            Ok(SaveOutcome::Indeterminate) | Err(_) => SaveResult::Indeterminate,
        };
        let _ = self.vfs.save_abort(id);
        result
    }
}
