// SPDX-License-Identifier: MPL-2.0

//! Owner-death watches for services that keep state on behalf of their callers.
//!
//! A service subscribes to the root death of each caller Cell incarnation
//! (`WatchCellOwner`) while serving that caller's message, the only moment the
//! kernel grants it. The death later arrives as a message without an identity
//! trailer whose sender is the dead Cell's **root** tid; [`OwnerWatches::take_death`]
//! maps it back to the `(cell_id, generation)` the service keys its state by.
//! CellIds are reused, so a new generation of a watched CellId also retires the
//! old one: its death record may never have arrived.
//!
//! Syscall-free behind [`WatchSys`], so host tests drive it with a scripted kernel.

use alloc::vec::Vec;

use abi::caller_identity::CallerIdentity;

/// The two kernel calls a watch needs.
pub trait WatchSys {
    /// Subscribe to the root death of `(cell_id, generation)`: the root tid the
    /// death will be reported under, and the cancel token. `None` when refused.
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)>;
    /// Cancel a token (harmless when the kernel already consumed it).
    fn cancel(&mut self, token: u64);
}

/// The live kernel.
pub struct KernelWatch;

impl WatchSys for KernelWatch {
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)> {
        crate::syscall::sys_watch_cell_owner(cell_id, generation)
            .map(|(owner, token)| (owner.root_tid as usize, token))
    }

    fn cancel(&mut self, token: u64) {
        crate::syscall::sys_cancel_cell_owner_watch(token);
    }
}

/// One Cell incarnation whose state a service holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Owner {
    pub cell_id: u64,
    pub generation: u64,
}

impl Owner {
    pub const fn of(caller: &CallerIdentity) -> Self {
        Self {
            cell_id: caller.cell_id,
            generation: caller.generation,
        }
    }
}

/// What [`OwnerWatches::admit`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Admission {
    /// The caller's owner is watched (newly or already).
    pub watched: bool,
    /// The watch was just installed.
    pub new: bool,
    /// Older generations of the caller's CellId: gone, reclaim them.
    pub stale: Vec<Owner>,
}

struct Watched {
    owner: Owner,
    root_tid: usize,
    token: u64,
}

/// Owners a service has subscribed to, at most `capacity` at once.
pub struct OwnerWatches {
    cells: Vec<Watched>,
    capacity: usize,
}

impl OwnerWatches {
    pub const fn new(capacity: usize) -> Self {
        Self {
            cells: Vec::new(),
            capacity,
        }
    }

    /// Whether `owner` is watched.
    pub fn watches(&self, owner: Owner) -> bool {
        self.cells.iter().any(|w| w.owner == owner)
    }

    /// Watch `caller`'s Cell incarnation if it is not watched yet. Must run in
    /// the receive turn that delivered the caller's message.
    pub fn admit(&mut self, caller: &CallerIdentity, sys: &mut impl WatchSys) -> Admission {
        let owner = Owner::of(caller);
        let mut admission = Admission::default();
        // Cell 0 / generation 0 is the kernel's own identity, never a client.
        if owner.cell_id == 0 || owner.generation == 0 {
            return admission;
        }
        if self.watches(owner) {
            admission.watched = true;
            return admission;
        }
        // Older generations of this CellId leave the table when it is admitted.
        let retiring = self
            .cells
            .iter()
            .filter(|w| w.owner.cell_id == owner.cell_id)
            .count();
        if self.cells.len() - retiring >= self.capacity {
            return admission;
        }
        let Some((root_tid, token)) = sys.watch(owner.cell_id, owner.generation) else {
            return admission;
        };
        while let Some(index) = self
            .cells
            .iter()
            .position(|w| w.owner.cell_id == owner.cell_id)
        {
            let old = self.cells.swap_remove(index);
            sys.cancel(old.token);
            admission.stale.push(old.owner);
        }
        self.cells.push(Watched {
            owner,
            root_tid,
            token,
        });
        admission.watched = true;
        admission.new = true;
        admission
    }

    /// The watched owner whose root tid is `root_tid`, removed: the kernel
    /// consumed its token with the death. A worker tid, 0 or an unwatched root
    /// never matches.
    pub fn take_death(&mut self, root_tid: usize) -> Option<Owner> {
        if root_tid == 0 {
            return None;
        }
        let index = self.cells.iter().position(|w| w.root_tid == root_tid)?;
        Some(self.cells.swap_remove(index).owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted kernel: the root tid of a cell is `cell * 1000`.
    #[derive(Default)]
    struct Scripted {
        refuse: bool,
        watches: Vec<(u64, u64)>,
        cancels: Vec<u64>,
        next_token: u64,
    }

    impl WatchSys for Scripted {
        fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)> {
            self.watches.push((cell_id, generation));
            if self.refuse {
                return None;
            }
            self.next_token += 1;
            Some((cell_id as usize * 1000, self.next_token))
        }

        fn cancel(&mut self, token: u64) {
            self.cancels.push(token);
        }
    }

    fn caller(cell_id: u64, generation: u64) -> CallerIdentity {
        CallerIdentity {
            flags: 0,
            cell_id,
            generation,
            sender_tid: cell_id * 1000 + 1,
        }
    }

    #[test]
    fn a_caller_is_watched_once_and_its_root_death_names_it() {
        let (mut watches, mut sys) = (OwnerWatches::new(4), Scripted::default());
        assert!(watches.admit(&caller(5, 1), &mut sys).new);
        let again = watches.admit(&caller(5, 1), &mut sys);
        assert!(again.watched && !again.new);
        assert_eq!(sys.watches, [(5, 1)], "no second subscription");
        // A worker tid is not the root.
        assert_eq!(watches.take_death(5001), None);
        assert_eq!(
            watches.take_death(5000),
            Some(Owner {
                cell_id: 5,
                generation: 1
            })
        );
        assert_eq!(watches.take_death(5000), None, "consumed with the death");
    }

    #[test]
    fn a_new_generation_retires_the_old_one() {
        let (mut watches, mut sys) = (OwnerWatches::new(4), Scripted::default());
        watches.admit(&caller(5, 1), &mut sys);
        let admission = watches.admit(&caller(5, 2), &mut sys);
        assert_eq!(
            admission.stale,
            [Owner {
                cell_id: 5,
                generation: 1
            }]
        );
        assert_eq!(sys.cancels, [1]);
        assert!(!watches.watches(Owner {
            cell_id: 5,
            generation: 1
        }));
    }

    #[test]
    fn a_refused_or_full_table_leaves_the_caller_unwatched() {
        let (mut watches, mut sys) = (OwnerWatches::new(1), Scripted::default());
        sys.refuse = true;
        assert!(!watches.admit(&caller(5, 1), &mut sys).watched);
        sys.refuse = false;
        assert!(watches.admit(&caller(5, 1), &mut sys).watched);
        assert!(
            !watches.admit(&caller(6, 1), &mut sys).watched,
            "capacity 1"
        );
        // A full table still admits a new generation of a watched cell: it retires the old one.
        let admission = watches.admit(&caller(5, 2), &mut sys);
        assert!(admission.watched && admission.stale.len() == 1);
        // The kernel's own identity is never a client.
        assert!(!watches.admit(&caller(0, 1), &mut sys).watched);
    }
}
