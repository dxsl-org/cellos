//! Carrying a directory-handle set from a spawner to the cell it spawns.
//!
//! The kernel's whole role is transport and provenance. It copies a set of
//! opaque values from one task to another and can later state which task the
//! values came from. It never resolves a handle, never checks one, and holds no
//! table of its own — the filesystem service issues, validates and revokes, and
//! remains the only place a handle means anything.
//!
//! ## Why the record cannot be a second source of truth
//! A cell acquires handles from the filesystem service after it starts, so the
//! set recorded here is not "what the cell holds" and must never be read as
//! that. It is only "what its spawner named at spawn". Treating it as authority
//! would create a copy that drifts from the service's table, and drift in this
//! direction widens what a cell may reach without anything failing to compile.
//!
//! ## Ordering contract
//! [`take_for_launch`] runs while the scheduler lock is held and produces the
//! complete record installed in the unpublished child before insertion.

use api::dir_handles::{DirHandleSet, InheritedDirHandles};

/// Record `set` as the handles `tid`'s next spawn should pass on.
///
/// Replaces any previously staged set: the most recent statement wins, and a
/// staged set that is never spawned against costs one bounded inline array.
pub fn stage(tid: usize, set: DirHandleSet) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.staged_dirs = set;
        }
    }
}

/// Discard whatever `tid` has staged.
///
/// Called after every spawn `tid` attempts, successful or not. A failed spawn
/// that left its set staged would hand it to whichever child `tid` created
/// next — an over-broad grant nobody asked for and nobody would see.
pub fn clear_staged(tid: usize) {
    stage(tid, DirHandleSet::EMPTY);
}

/// Consume `spawner`'s staged set and build the child's immutable provenance.
///
/// Call while holding the scheduler lock immediately before atomic publication.
pub(crate) fn take_for_launch(
    sched: &mut super::scheduler::Scheduler,
    spawner: usize,
) -> InheritedDirHandles {
    if spawner == 0 {
        return InheritedDirHandles::NONE;
    }
    let Some(spawner_task) = sched.tasks.get_mut(&spawner) else {
        return InheritedDirHandles::NONE;
    };
    let set = core::mem::replace(&mut spawner_task.staged_dirs, DirHandleSet::EMPTY);
    if set.is_empty() || spawner_task.cell_id.0 == 0 {
        return InheritedDirHandles::NONE;
    }
    let inherited = InheritedDirHandles {
        spawner_cell_id: spawner_task.cell_id.0,
        spawner_generation: spawner_task.cell_generation,
        set,
    };
    log::info!(
        "[dirs] next cell inherits {} dir handle(s) from cell {}",
        set.len(),
        inherited.spawner_cell_id
    );
    inherited
}

/// Returns `None` when `cell_id` names no live root-owned Cell generation.
/// Cell IDs are bounded reusable quota slots, never task-table indexes.
pub fn attestation_for(cell_id: u64) -> Option<api::dir_attestation::ViDirHandleAttestation> {
    if cell_id == 0 {
        return None;
    }
    let guard = super::SCHEDULER.lock();
    let sched = guard.as_ref()?;
    let owner = sched.live_cell_owner_for_id(types::CellId(cell_id))?;
    let task = sched.tasks.get(&(owner.root_tid as usize))?;
    Some(api::dir_attestation::ViDirHandleAttestation {
        cell_id,
        generation: owner.generation,
        spawner_cell_id: task.inherited_dirs.spawner_cell_id,
        spawner_generation: task.inherited_dirs.spawner_generation,
        set: task.inherited_dirs.set,
    })
}
