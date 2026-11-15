//! Kernel-owned registry of stack VA runs, guard pages included.
//!
//! A stack's bottom guard pages are deliberately *unmapped*: a kernel stack's
//! guard frames at their identity VA, a user stack's guard VA in its arena
//! slot. "Not currently mapped" is therefore no proof that a VA is free: a cell
//! image placed over a guard page would turn the next overflow of that stack
//! into a silent write to an attacker-readable frame instead of a fault. Every
//! live stack's whole range `[base, top)` is recorded here from allocation
//! until its frames are back on the allocator, and the ELF loader refuses any
//! page that overlaps one. (The loader never places an image in the stack
//! arena, which lies above the cell slots; the record keeps the one collision
//! view for both placements.)
//!
//! The whole range is recorded, not only the guards, so the teardown window —
//! frames unpublished but not yet freed (or restored to the identity map) — is
//! covered too.
//!
//! Lock order: the registry lock is a leaf. It is taken under
//! `FRAME_ALLOCATOR` (stack allocation/release, ELF admission) and takes
//! nothing itself; heap growth never takes `FRAME_ALLOCATOR`.

use crate::sync::Spinlock;
use alloc::vec::Vec;
use types::ViError;

pub(crate) struct StackReservations {
    runs: Spinlock<Vec<(usize, usize)>>,
}

impl StackReservations {
    pub(crate) const fn new() -> Self {
        Self {
            runs: Spinlock::new(Vec::new()),
        }
    }

    /// Record `[start, end)`. Fails (allocation refused) rather than halting
    /// on heap exhaustion: an unrecorded stack must not be handed out.
    pub(crate) fn insert(&self, start: usize, end: usize) -> Result<(), ViError> {
        if start >= end {
            return Err(ViError::InvalidInput);
        }
        let mut runs = self.runs.lock();
        runs.try_reserve(1).map_err(|_| ViError::OutOfMemory)?;
        runs.push((start, end));
        Ok(())
    }

    /// Forget exactly `[start, end)`; `false` when it was never recorded.
    pub(crate) fn remove(&self, start: usize, end: usize) -> bool {
        let mut runs = self.runs.lock();
        let Some(index) = runs.iter().position(|&run| run == (start, end)) else {
            return false;
        };
        runs.swap_remove(index);
        true
    }

    /// Does any recorded run intersect `[start, end)`?
    pub(crate) fn overlaps(&self, start: usize, end: usize) -> bool {
        self.runs
            .lock()
            .iter()
            .any(|&(run_start, run_end)| start < run_end && run_start < end)
    }
}

pub(crate) static STACK_RESERVATIONS: StackReservations = StackReservations::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_exact_at_the_edges() {
        let runs = StackReservations::new();
        runs.insert(0x8000, 0xA000).unwrap();
        assert!(runs.overlaps(0x8000, 0x9000), "guard page itself");
        assert!(runs.overlaps(0x7000, 0x8001));
        assert!(!runs.overlaps(0x7000, 0x8000), "page just below");
        assert!(!runs.overlaps(0xA000, 0xB000), "page just above");
        assert!(runs.remove(0x8000, 0xA000));
        assert!(!runs.overlaps(0x8000, 0x9000));
        assert!(!runs.remove(0x8000, 0xA000));
        assert!(runs.insert(0x9000, 0x9000).is_err());
    }
}
