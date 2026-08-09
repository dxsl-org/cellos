//! How long a directory handle lasts, and what takes it away.
//!
//! ## A handle dies with the cell that holds it
//! An entry belongs to a [`Caller`], which carries the cell's generation, so a
//! successor cell under a recycled id is a different holder and reaches nothing
//! its predecessor had. Seeing a higher generation proves the predecessor is
//! gone, and its entries are purged then rather than left to be filtered out on
//! every lookup — a dead cell's authority should not sit in the table waiting
//! for a lookup that never comes.
//!
//! That is also why a replacement instance inherits nothing across a hot-swap.
//! The generation exists to tell a respawned cell from the one it replaced, and
//! honouring a predecessor's handles would discard that distinction at the one
//! moment it matters. A cell that transfers state re-acquires its handles on the
//! way back up; what it cannot do is silently continue with a dead instance's
//! authority.
//!
//! ## Revocation is transitive
//! A derived handle is a narrower share of the handle it came from, so it cannot
//! outlive it: authority surviving the withdrawal of the authority it came from
//! would make revocation advisory, and a cell could keep access indefinitely by
//! deriving a subdirectory and dropping the original. Every entry records the
//! handle it was derived from and revocation walks that graph — across cells,
//! because a set inherited at spawn records the spawner's handle as its parent.
//!
//! The cost is real and accepted: a child can lose access because of a decision
//! about its parent, the same shape as a spawner's capability ceiling bounding a
//! child it will never see again.

mod revoke;

use api::dir_handles::ViDirHandle;

use super::{CellState, DirError, DirTable, RevokeOutcome};
use crate::caller::Caller;

/// What the service still owes `caller` before serving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contact {
    /// Nothing outstanding.
    Ready,
    /// The kernel has not yet been asked what this cell inherited.
    NeedsAttestation {
        replaced_owner: Option<Caller>,
        revoked_dir_ids: alloc::vec::Vec<u64>,
    },
}

impl DirTable {
    /// Register contact with `caller` and report what is still owed.
    ///
    /// A message arriving under a *lower* generation than the one on record
    /// cannot come from a live cell; it is left unregistered, which means it
    /// holds nothing and can bind nothing.
    pub fn on_contact(&mut self, caller: Caller) -> Contact {
        let cell = caller.cell.0;
        match self.cells.get(&cell) {
            Some(state) if state.generation == caller.generation => {
                if state.attested {
                    Contact::Ready
                } else {
                    Contact::NeedsAttestation {
                        replaced_owner: None,
                        revoked_dir_ids: alloc::vec![],
                    }
                }
            }
            Some(state) if state.generation < caller.generation => {
                let replaced_owner = Caller {
                    cell: caller.cell,
                    generation: state.generation,
                };
                let revoked = self.purge_cell(cell);
                self.insert_cell_state(caller);
                Contact::NeedsAttestation {
                    replaced_owner: Some(replaced_owner),
                    revoked_dir_ids: revoked.revoked_ids,
                }
            }
            Some(_) => Contact::Ready,
            None => {
                self.insert_cell_state(caller);
                Contact::NeedsAttestation {
                    replaced_owner: None,
                    revoked_dir_ids: alloc::vec![],
                }
            }
        }
    }

    /// Record that the kernel has been asked about `caller`, whatever it said.
    pub fn mark_attested(&mut self, caller: Caller) {
        if let Some(state) = self.state_mut(caller) {
            state.attested = true;
        }
    }

    /// Whether path-addressed requests from `caller` must be refused.
    pub fn is_sealed(&self, caller: Caller) -> bool {
        self.state(caller).is_some_and(|s| s.sealed)
    }

    /// Whether this cell generation has been seen before.
    ///
    /// The fast-IPC path uses this: it cannot make the attestation syscall, so a
    /// cell it has never seen might be one that should already be sealed. It
    /// declines those and lets the ecall path decide.
    pub fn has_met(&self, caller: Caller) -> bool {
        self.state(caller).is_some()
    }

    /// Refuse every path-addressed request from `caller` from now on.
    ///
    /// One-way for the life of the cell generation. Returns `false` only for a
    /// caller with no registered state, which cannot happen after
    /// [`Self::on_contact`].
    pub fn seal(&mut self, caller: Caller) -> bool {
        match self.state_mut(caller) {
            Some(state) => {
                state.sealed = true;
                true
            }
            None => false,
        }
    }

    /// Revoke `dir` and everything derived from it. Returns how many entries went.
    ///
    /// # Errors
    /// [`DirError::UnknownHandle`] when `dir` is not this caller's.
    pub fn revoke(&mut self, caller: Caller, dir: ViDirHandle) -> Result<RevokeOutcome, DirError> {
        if self.owned(caller, dir).is_none() {
            return Err(DirError::UnknownHandle);
        }
        Ok(self.revoke_ids(&[dir.0]))
    }

    pub(crate) fn state(&self, caller: Caller) -> Option<&CellState> {
        self.cells
            .get(&caller.cell.0)
            .filter(|s| s.generation == caller.generation)
    }

    pub(crate) fn state_mut(&mut self, caller: Caller) -> Option<&mut CellState> {
        self.cells
            .get_mut(&caller.cell.0)
            .filter(|s| s.generation == caller.generation)
    }

    fn insert_cell_state(&mut self, caller: Caller) {
        self.cells.insert(
            caller.cell.0,
            CellState {
                generation: caller.generation,
                sealed: false,
                attested: false,
                handles: 0,
            },
        );
    }
}
