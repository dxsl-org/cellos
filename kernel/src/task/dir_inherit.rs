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
//! [`install_on_child`] runs inside the scheduler-lock critical section that
//! creates the child, before the child is reachable by any hart. A child is
//! therefore never observable with an unset record, so the service can never
//! read "no set" for a cell that was in fact given one.

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

/// Move `spawner`'s staged set onto the freshly created `child`.
///
/// Call while holding the scheduler lock that created `child` and before
/// releasing it; see the module ordering contract. `spawner` of `0` (kernel
/// boot, no calling task) installs nothing.
///
/// The spawner's identity is captured here rather than trusted later: by the
/// time the filesystem service asks, the spawner may be gone or replaced.
pub fn install_on_child(sched: &mut super::scheduler::Scheduler, child: usize, spawner: usize) {
    if spawner == 0 || spawner == child {
        return;
    }
    let Some(spawner_task) = sched.tasks.get_mut(&spawner) else {
        return;
    };
    let set = core::mem::replace(&mut spawner_task.staged_dirs, DirHandleSet::EMPTY);
    if set.is_empty() {
        return;
    }
    let spawner_cell_id = spawner_task.cell_id.0;
    let spawner_generation = spawner_task.cell_generation;
    // A set whose source the kernel cannot name is worthless to the service that
    // must check it against that source, so drop it rather than pass an
    // unattributable grant.
    if spawner_cell_id == 0 {
        log::warn!("[dirs] dropping staged set from unattributable spawner tid {spawner}");
        return;
    }
    if let Some(child_task) = sched.tasks.get_mut(&child) {
        child_task.inherited_dirs = InheritedDirHandles {
            spawner_cell_id,
            spawner_generation,
            set,
        };
        log::info!(
            "[dirs] cell {} inherits {} dir handle(s) from cell {}",
            child,
            set.len(),
            spawner_cell_id
        );
    }
}

/// What the kernel is prepared to state about `cell_id`'s inherited handles.
///
/// Returns `None` when `cell_id` names no live cell. The record lives on the
/// cell's own task — the one whose tid the `CellId` is derived from — so a
/// thread of that cell resolves to the same answer rather than to an empty set
/// of its own.
pub fn attestation_for(cell_id: u64) -> Option<api::dir_attestation::ViDirHandleAttestation> {
    if cell_id == 0 {
        return None;
    }
    let guard = super::SCHEDULER.lock();
    let task = guard.as_ref()?.tasks.get(&(cell_id as usize))?;
    if task.cell_id.0 != cell_id {
        return None;
    }
    Some(api::dir_attestation::ViDirHandleAttestation {
        cell_id,
        generation: task.cell_generation,
        spawner_cell_id: task.inherited_dirs.spawner_cell_id,
        spawner_generation: task.inherited_dirs.spawner_generation,
        set: task.inherited_dirs.set,
    })
}
