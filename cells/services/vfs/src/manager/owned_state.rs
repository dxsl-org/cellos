use crate::caller::Caller;
use crate::manager::{VfsManager, WatchedOwner};

impl VfsManager {
    /// Install a root-lifetime watch before dispatch can create durable state.
    /// A reused CellId retires only the predecessor principal, never a broad
    /// CellId row, and its token is cancelled after the VFS lock is released.
    pub fn install_owner_watch(&mut self, caller: Caller, root_tid: usize, token: u64) {
        let stale: alloc::vec::Vec<(u64, u64)> = self
            .watched_owners
            .keys()
            .filter(|(cell_id, generation)| {
                *cell_id == caller.cell.0 && *generation != caller.generation
            })
            .copied()
            .collect();
        for key in stale {
            if let Some(previous) = self.watched_owners.remove(&key) {
                self.purge_owned_state(previous.principal);
                self.cancelled_owner_watch_tokens.push(previous.token);
            }
        }
        let key = (caller.cell.0, caller.generation);
        if self.watched_owners.contains_key(&key) {
            self.cancelled_owner_watch_tokens.push(token);
            return;
        }
        self.watched_owners.insert(
            key,
            WatchedOwner {
                principal: caller,
                root_tid,
                token,
            },
        );
    }

    /// A death notification has no caller trailer. It is attributable only when
    /// its root TID matches a live, tokenized local owner record.
    pub fn handle_unattributed_owner_death(&mut self, root_tid: usize) -> bool {
        let matches: alloc::vec::Vec<(u64, u64)> = self
            .watched_owners
            .iter()
            .filter_map(|(&key, owner)| (owner.root_tid == root_tid).then_some(key))
            .collect();
        let mut purged = false;
        for key in matches {
            if let Some(owner) = self.watched_owners.remove(&key) {
                self.purge_owned_state(owner.principal);
                self.cancelled_owner_watch_tokens.push(owner.token);
                purged = true;
            }
        }
        purged
    }

    pub fn take_owner_watch_cancellations(&mut self) -> alloc::vec::Vec<u64> {
        core::mem::take(&mut self.cancelled_owner_watch_tokens)
    }

    fn purge_owned_state(&mut self, caller: Caller) -> usize {
        let dirs = self.dirs.purge_owner(caller);
        let revoked_files = self.files.revoke_by_parent_dirs(&dirs.revoked_ids);
        let files = self.files.purge_owner(caller);
        let handles = self.handles.purge_owner(caller);
        let pending = self.pending.purge_owner(caller);
        dirs.count + revoked_files + files + handles + pending
    }
}
