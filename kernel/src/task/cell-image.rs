//! Kernel side of `ResolveCellImage` (syscall 441, `api::cell_image`).
//!
//! The identity lives on the Cell's root task (`Task::bind_image`), bound from
//! the admission measurement before the task is published and never rewritten.
//! It is reached only through the live owner slot, so the answer is always for
//! the exact `(cell_id, generation)` asked about: a respawn under the same
//! `CellId` is a different generation with its own root and its own image, and
//! a dead or stale generation has no answer at all.

use api::cell_image::CellImage;
use api::cell_owner::CellOwnerRequest;
use types::CellId;

impl super::scheduler::Scheduler {
    /// Image identity of the live `(cell_id, generation)`. `None` when that
    /// generation is not live (stale, retired, never existed) or its root was
    /// not admitted through a measured image.
    pub(crate) fn resolve_cell_image(&self, cell_id: CellId, generation: u64) -> Option<CellImage> {
        let owner = self.resolve_live_cell_owner(cell_id, generation)?;
        let root = self.tasks.get(&(owner.root_tid as usize))?;
        Some(CellImage {
            cell_id: owner.cell_id,
            generation: owner.generation,
            identity: root.image()?,
        })
    }
}

/// Dispatch rule for 441: answered only for a task of the registered VFS Cell.
///
/// Not tied to the principal VFS is currently receiving from (unlike 247):
/// VFS re-authorizes deferred work (handle reads, prepared directories) after
/// its receive context has moved on, and an image identity grants nothing by
/// itself. The registered-VFS check keeps every other Cell from learning which
/// program another Cell runs.
pub(super) fn resolve_for_caller(
    sched: &super::scheduler::Scheduler,
    caller_id: usize,
    request: CellOwnerRequest,
    is_vfs_cell: impl Fn(usize) -> bool,
) -> Option<CellImage> {
    let caller = sched.tasks.get(&caller_id)?;
    if !is_vfs_cell(caller.cell_id.0 as usize) {
        return None;
    }
    sched.resolve_cell_image(CellId(request.cell_id), request.generation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::scheduler::Scheduler;
    use crate::task::Task;
    use alloc::boxed::Box;
    use api::cell_image::{ImageIdentity, ANCHOR_RELEASE};
    use api::cell_owner::CellOwner;

    const ROOT: usize = 60_441;
    const CELL: u64 = 62;
    const VFS_TID: usize = 60_442;
    const VFS_CELL: u64 = 63;

    fn sshd() -> ImageIdentity {
        ImageIdentity::signed([7; 32], ANCHOR_RELEASE, b"sshd").expect("valid identity")
    }

    /// One published root with `image` bound, plus a VFS task. Returns the
    /// root's generation.
    fn fixture(sched: &mut Scheduler, image: Option<ImageIdentity>) -> u64 {
        let mut root = Box::new(Task::new(ROOT, CellId(CELL), "sshd", alloc::vec![]));
        if let Some(image) = image {
            assert!(root.bind_image(image));
        }
        let generation = root.cell_generation;
        sched.tasks.insert(ROOT, root);
        assert!(sched.publish_live_cell_owner(CellOwner::new(CELL, generation, ROOT as u64)));
        let vfs = Box::new(Task::new(VFS_TID, CellId(VFS_CELL), "vfs", alloc::vec![]));
        sched.tasks.insert(VFS_TID, vfs);
        generation
    }

    #[test]
    fn bound_identity_is_immutable() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut task = Task::new(ROOT, CellId(CELL), "sshd", alloc::vec![]);
        assert_eq!(task.image(), None);
        assert!(task.bind_image(sshd()));
        assert!(!task.bind_image(ImageIdentity::unsigned([9; 32])));
        assert_eq!(task.image(), Some(sshd()));
    }

    #[test]
    fn exact_live_generation_resolves_and_stale_ones_fail() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = Scheduler::new();
        let generation = fixture(&mut sched, Some(sshd()));
        let image = sched
            .resolve_cell_image(CellId(CELL), generation)
            .expect("live generation resolves");
        assert_eq!((image.cell_id, image.generation), (CELL, generation));
        assert_eq!(image.identity, sshd());
        for stale in [0, generation - 1, generation + 1] {
            assert!(sched.resolve_cell_image(CellId(CELL), stale).is_none());
        }
        assert!(sched
            .resolve_cell_image(CellId(CELL + 1), generation)
            .is_none());
        // Once the root is gone the incarnation is over: no answer survives it.
        sched.tasks.remove(&ROOT);
        assert!(sched.resolve_cell_image(CellId(CELL), generation).is_none());
    }

    #[test]
    fn unmeasured_root_has_no_identity() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = Scheduler::new();
        let generation = fixture(&mut sched, None);
        assert!(sched.resolve_cell_image(CellId(CELL), generation).is_none());
    }

    #[test]
    fn only_the_registered_vfs_cell_is_answered() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = Scheduler::new();
        let generation = fixture(&mut sched, Some(sshd()));
        let request = CellOwnerRequest::new(CELL, generation);
        let is_vfs = |cell: usize| cell == VFS_CELL as usize;
        assert!(resolve_for_caller(&sched, VFS_TID, request, is_vfs).is_some());
        // The subject itself, an unknown task, and VFS asking about a stale
        // generation all get nothing.
        assert!(resolve_for_caller(&sched, ROOT, request, is_vfs).is_none());
        assert!(resolve_for_caller(&sched, 1, request, is_vfs).is_none());
        let stale = CellOwnerRequest::new(CELL, generation + 1);
        assert!(resolve_for_caller(&sched, VFS_TID, stale, is_vfs).is_none());
    }
}
