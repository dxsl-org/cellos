//! Backend routing and change marks for D03 conditional save. Authorization,
//! quota and the transaction table live in `crate::save`.

use alloc::string::String;

use api::ipc::{Observation, SaveExpect, SaveOutcome};

use crate::manager::VfsManager;

impl VfsManager {
    /// Staging path for transaction `id` on the backend that owns `dst`;
    /// `None` when that backend has no conditional publication.
    pub fn stage_path(&self, dst: &str, id: u32) -> Option<String> {
        self.mounts.backend(dst)?.stage_path(id)
    }

    pub fn observe(&self, path: &str) -> Option<Observation> {
        self.mounts.backend(path)?.observe(path)
    }

    pub fn stage_create(&mut self, stage: &str) -> bool {
        self.mounts.backend_mut(stage).is_some_and(|b| b.stage_create(stage))
    }

    pub fn stage_write(&mut self, stage: &str, offset: u64, bytes: &[u8]) -> bool {
        self.mounts.backend_mut(stage).is_some_and(|b| b.stage_write(stage, offset, bytes))
    }

    /// Remove an unpublished staging object (synced like any unlink).
    pub fn stage_remove(&mut self, stage: &str) -> bool {
        self.mounts.backend_mut(stage).is_some_and(|b| b.unlink(stage))
    }

    pub fn publish_if(&mut self, stage: &str, sealed: (u64, u64), dst: &str, expect: SaveExpect) -> SaveOutcome {
        let created = self.would_create(dst);
        let outcome = match self.mounts.backend_mut(dst) {
            Some(backend) => backend.publish_if(stage, sealed, dst, expect),
            None => SaveOutcome::NotPublished(api::ipc::save_refusal::REFUSED),
        };
        // An indeterminate publication may have reached storage, so it marks too.
        if outcome.published().is_some() || outcome == SaveOutcome::Indeterminate {
            self.mark_content(dst, created);
            self.mark_binding(stage);
        }
        outcome
    }
}
