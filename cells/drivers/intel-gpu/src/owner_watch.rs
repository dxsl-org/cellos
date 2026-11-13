//! Client-death tracking (CP1 D-G7): which tids each client Cell incarnation sent from,
//! and the kernel owner watch (`WatchCellOwner`) that reports the Cell's death.
//!
//! Sessions are keyed by sending **thread** tid while the kernel reports the death of the
//! Cell's **root** tid, so a watched Cell carries every tid that opened a file for it.
//! Syscall-free so host tests drive it; `main.rs` supplies the kernel through [`WatchSys`].

use abi::caller_identity::CallerIdentity;
use alloc::vec::Vec;
use intel_gpu_core::server::MAX_CLIENTS;

/// The two kernel calls a watch needs.
pub trait WatchSys {
    /// Subscribe to the root death of `(cell_id, generation)`. The kernel grants it only to
    /// the registered provider while it serves that principal, i.e. during the receive turn
    /// of its message. Returns the root tid the death is reported under, and the token.
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)>;
    /// Cancel a watch token (harmless when already consumed).
    fn cancel(&mut self, token: u64);
}

struct Watched {
    cell_id: u64,
    generation: u64,
    root_tid: usize,
    token: u64,
    tids: Vec<usize>,
}

/// What [`OwnerWatches::admit`] asks the caller to do besides serving the request.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Admission {
    /// Tids of an older, dead generation of the same Cell: release their sessions.
    pub stale: Vec<usize>,
    /// No watch could be established; true only the first time (log once).
    pub unavailable: bool,
}

/// Cells whose death the driver has subscribed to.
#[derive(Default)]
pub struct OwnerWatches {
    cells: Vec<Watched>,
    unavailable_logged: bool,
}

impl OwnerWatches {
    /// Record that `tid` of `identity`'s Cell is opening a file, subscribing to the Cell's
    /// death on first sight. A refused watch never denies the request: the client is served
    /// and its files are released when it closes them or its tid is reused.
    pub fn admit(
        &mut self,
        identity: &CallerIdentity,
        tid: usize,
        sys: &mut impl WatchSys,
    ) -> Admission {
        let mut admission = Admission::default();
        if let Some(cell) = self
            .cells
            .iter_mut()
            .find(|w| (w.cell_id, w.generation) == (identity.cell_id, identity.generation))
        {
            // The server holds at most MAX_CLIENTS tids, so this cannot grow without bound.
            if !cell.tids.contains(&tid) && cell.tids.len() < MAX_CLIENTS {
                cell.tids.push(tid);
            }
            return admission;
        }
        let watched = (self.cells.len() < MAX_CLIENTS)
            .then(|| sys.watch(identity.cell_id, identity.generation))
            .flatten();
        let Some((root_tid, token)) = watched else {
            admission.unavailable = !core::mem::replace(&mut self.unavailable_logged, true);
            return admission;
        };
        // CellIds are reused: an older generation is a dead Cell whose death never reached us.
        while let Some(index) = self
            .cells
            .iter()
            .position(|w| w.cell_id == identity.cell_id)
        {
            let old = self.cells.swap_remove(index);
            sys.cancel(old.token);
            admission.stale.extend(old.tids);
        }
        self.cells.push(Watched {
            cell_id: identity.cell_id,
            generation: identity.generation,
            root_tid,
            token,
            tids: alloc::vec![tid],
        });
        admission
    }

    /// The tids of the watched Cell whose root tid is `root_tid`, removed: the kernel
    /// consumed its token with the death. A worker tid, 0 or an unwatched root never matches.
    pub fn take_death(&mut self, root_tid: usize) -> Option<Vec<usize>> {
        if root_tid == 0 {
            return None;
        }
        let index = self.cells.iter().position(|w| w.root_tid == root_tid)?;
        Some(self.cells.swap_remove(index).tids)
    }
}
