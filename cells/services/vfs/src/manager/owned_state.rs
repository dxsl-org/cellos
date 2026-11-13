use crate::caller::Caller;
use crate::manager::VfsManager;
use types::CellId;

impl VfsManager {
    /// `caller`'s owner death is watched (by `serve`, before dispatch can create
    /// durable state); remember it so reconcile knows the death will arrive.
    pub fn note_watched_owner(&mut self, caller: Caller) {
        self.watched_owners
            .insert((caller.cell.0, caller.generation));
    }

    /// `(cell_id, generation)` is gone: its root died or a newer generation of
    /// its CellId took over. Purge everything kept for that exact principal,
    /// never a broad CellId row. Returns whether it was watched.
    pub fn owner_died(&mut self, cell_id: u64, generation: u64) -> bool {
        let watched = self.watched_owners.remove(&(cell_id, generation));
        self.purge_owned_state(Caller::principal(CellId(cell_id), generation));
        watched
    }

    /// Whether `caller`'s root death will reach the receive loop as a notification.
    pub fn watches_owner(&self, caller: Caller) -> bool {
        self.watched_owners
            .contains(&(caller.cell.0, caller.generation))
    }

    fn purge_owned_state(&mut self, caller: Caller) -> usize {
        let dirs = self.dirs.purge_owner(caller);
        let revoked_files = self.files.revoke_by_parent_dirs(&dirs.revoked_ids);
        let files = self.files.purge_owner(caller);
        let pending = self.pending.purge_owner(caller);
        dirs.count + revoked_files + files + pending
    }
}
