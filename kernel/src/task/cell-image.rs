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

impl crate::sched::scheduler::Scheduler {
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

/// Dispatch rule for 441: answered for a task of the registered VFS Cell, or
/// for a `SupervisorCap` holder (architecture remediation Phase 06, CP1 option
/// A: `/bin/supervisor` authorises `OP_POWER`/`OP_HOTSWAP` by the sender's signed
/// program).
///
/// Not tied to the principal VFS is currently receiving from (unlike 247):
/// VFS re-authorizes deferred work (handle reads, prepared directories) after
/// its receive context has moved on, and an image identity grants nothing by
/// itself. Every other Cell is refused, so it cannot learn which program
/// another Cell runs.
///
/// The supervisor gets an answer only for a Cell whose name is its launch
/// identity (`Task::launch_named`): a signed `shutdown` copied under `/home`, or
/// bytes another caller supplied, is the same program but not the reviewed
/// install, and gets `None` (refused) — the rule the kernel's own 502 gate uses.
/// VFS keeps the unfiltered record (`/srv/sshd` is not a `/bin` install).
pub(super) fn resolve_for_caller(
    sched: &crate::sched::scheduler::Scheduler,
    caller_id: usize,
    request: CellOwnerRequest,
    retains_outer_caller: impl Fn(u64, u64) -> bool,
) -> Option<CellImage> {
    let caller = sched.tasks.get(&caller_id)?;
    if retains_outer_caller(caller.cell_id.0, caller.cell_generation) {
        return sched.resolve_cell_image(CellId(request.cell_id), request.generation);
    }
    if !sched.cell_rights(caller_id).supervisor {
        return None;
    }
    let owner = sched.resolve_live_cell_owner(CellId(request.cell_id), request.generation)?;
    let root = sched.tasks.get(&(owner.root_tid as usize))?;
    if !root.launch_named {
        return None;
    }
    sched.resolve_cell_image(CellId(request.cell_id), request.generation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sched::scheduler::Scheduler;
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
        let is_vfs = |cell: u64, _generation: u64| cell == VFS_CELL;
        assert!(resolve_for_caller(&sched, VFS_TID, request, is_vfs).is_some());
        // The subject itself, an unknown task, and VFS asking about a stale
        // generation all get nothing.
        assert!(resolve_for_caller(&sched, ROOT, request, is_vfs).is_none());
        assert!(resolve_for_caller(&sched, 1, request, is_vfs).is_none());
        let stale = CellOwnerRequest::new(CELL, generation + 1);
        assert!(resolve_for_caller(&sched, VFS_TID, stale, is_vfs).is_none());
    }

    /// CP1 option A: a `SupervisorCap` holder is answered too, but only for a
    /// Cell whose signed name is its launch identity (`Task::launch_named`).
    #[test]
    fn supervisor_is_answered_only_for_named_launches() {
        const SUPERVISOR_TID: usize = 60_443;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let mut sched = Scheduler::new();
        let generation = fixture(&mut sched, Some(sshd()));
        let mut supervisor = Box::new(Task::new(
            SUPERVISOR_TID,
            CellId(64),
            "supervisor",
            alloc::vec![],
        ));
        crate::task::cap::CapSet {
            supervisor: true,
            ..crate::task::cap::CapSet::EMPTY
        }
        .apply_to(&mut supervisor);
        sched.tasks.insert(SUPERVISOR_TID, supervisor);
        let request = CellOwnerRequest::new(CELL, generation);
        let no_vfs = |_: u64, _: u64| false;

        // Same bytes, but not a reviewed install (sandbox copy, caller ELF).
        assert!(resolve_for_caller(&sched, SUPERVISOR_TID, request, no_vfs).is_none());
        sched.tasks.get_mut(&ROOT).expect("root").launch_named = true;
        let image = resolve_for_caller(&sched, SUPERVISOR_TID, request, no_vfs)
            .expect("supervisor answered for a named launch");
        assert_eq!(image.identity, sshd());
        // A task without SupervisorCap still learns nothing.
        assert!(resolve_for_caller(&sched, VFS_TID, request, no_vfs).is_none());
        let stale = CellOwnerRequest::new(CELL, generation + 1);
        assert!(resolve_for_caller(&sched, SUPERVISOR_TID, stale, no_vfs).is_none());
    }
}
