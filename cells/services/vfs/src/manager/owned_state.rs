use crate::caller::Caller;
use crate::manager::VfsManager;

impl VfsManager {
    pub fn should_watch_after_response(
        &mut self,
        caller: Caller,
        response: &api::ipc::VfsResponse<'_>,
    ) -> Option<usize> {
        if !caller.may_own_state() || !Self::response_creates_owned_state(response) {
            return None;
        }
        match self.watched_owners.insert(caller.cell.0, caller) {
            Some(existing) if existing == caller => None,
            Some(existing) => {
                self.purge_owned_state(existing);
                Some(caller.cell.0 as usize)
            }
            None => Some(caller.cell.0 as usize),
        }
    }

    pub fn handle_unattributed_owner_death(&mut self, owner_tid: usize) -> bool {
        let Some(caller) = self.watched_owners.remove(&(owner_tid as u64)) else {
            return false;
        };
        self.purge_owned_state(caller);
        true
    }

    pub fn rollback_owner_watch(&mut self, caller: Caller) {
        self.watched_owners.remove(&caller.cell.0);
        self.purge_owned_state(caller);
    }

    fn purge_owned_state(&mut self, caller: Caller) -> usize {
        let dirs = self.dirs.purge_owner(caller);
        let revoked_files = self.files.revoke_by_parent_dirs(&dirs.revoked_ids);
        let files = self.files.purge_owner(caller);
        let handles = self.handles.purge_owner(caller);
        let pending = self.pending.purge_owner(caller);
        dirs.count + revoked_files + files + handles + pending
    }

    fn response_creates_owned_state(response: &api::ipc::VfsResponse<'_>) -> bool {
        matches!(
            response,
            api::ipc::VfsResponse::DirHandle(_)
                | api::ipc::VfsResponse::PendingHandle(_)
                | api::ipc::VfsResponse::FileHandle(_)
        )
    }
}
