//! Logical ↔ physical CPU map (plan 261103-2218 phase 07a).
//!
//! A *logical* hart is the dense index every per-hart array is sized by
//! (`0..MAX_HARTS`, 0 = boot hart, [`super::HART_RT`] = the RealTime hart). A
//! *physical* id is what the hardware or firmware names the CPU by: the LAPIC ID
//! on x86_64, the SBI hart id on RISC-V, the CPU index on AArch64 (its `/cpus`
//! position, which `smp::mpidr` maps to and from `MPIDR_EL1`).
//!
//! Contract:
//! - Each pair is published by the boot hart **before** the CPU it names is
//!   started (on AArch64 the boot hart's own pair is published by
//!   `hart_local::install`). A started CPU may re-publish its own pair, which is
//!   a no-op; nothing else writes.
//! - Every slot is written at most once (compare-exchange from [`UNMAPPED`]) and
//!   is never cleared, so readers take no lock: a reader sees either the final
//!   value or "unmapped".
//! - The map is a bijection: no logical hart names two physical ids and no
//!   physical id is claimed by two logical harts, even if publishers race.
//! - Out-of-range ids are refused, never truncated.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Slot value meaning "no mapping published".
pub const UNMAPPED: usize = usize::MAX;

/// Exclusive bound on the physical ids [`CPU_TOPOLOGY`] can map
/// (`crate::arch::hart_local::PHYSICAL_ID_LIMIT`).
pub const PHYSICAL_ID_LIMIT: usize = crate::arch::hart_local::PHYSICAL_ID_LIMIT;

/// The kernel's one logical ↔ physical map, filled at boot (see module docs).
pub static CPU_TOPOLOGY: CpuTopology<{ super::MAX_HARTS }, PHYSICAL_ID_LIMIT> = CpuTopology::new();

/// A write-once bijection between `N` logical harts and physical ids below `P`.
pub struct CpuTopology<const N: usize, const P: usize> {
    /// Logical hart → physical id, or [`UNMAPPED`].
    physical: [AtomicUsize; N],
    /// Physical id → logical hart, or [`UNMAPPED`]. Kept so the reverse lookup
    /// (AArch64 resolves the calling hart through it on every
    /// `current_hart_id`) stays O(1).
    logical: [AtomicUsize; P],
    /// Logical slots published so far.
    count: AtomicUsize,
}

impl<const N: usize, const P: usize> CpuTopology<N, P> {
    pub const fn new() -> Self {
        Self {
            physical: [const { AtomicUsize::new(UNMAPPED) }; N],
            logical: [const { AtomicUsize::new(UNMAPPED) }; P],
            count: AtomicUsize::new(0),
        }
    }

    /// Publish `logical ↔ physical`.
    ///
    /// Returns `true` when the pair is now mapped (also when it already was).
    /// Returns `false`, leaving the map unchanged, when either id is out of
    /// range, `logical` already names another physical id, or `physical` is
    /// already claimed by another logical hart.
    pub fn publish(&self, logical: usize, physical: usize) -> bool {
        let (Some(forward), Some(reverse)) =
            (self.physical.get(logical), self.logical.get(physical))
        else {
            return false;
        };
        // Claim the physical id first: whoever wins it is the only publisher
        // that may then take a logical slot for it.
        let claimed = match reverse.compare_exchange(
            UNMAPPED,
            logical,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => true,
            Err(owner) if owner == logical => false,
            Err(_) => return false,
        };
        match forward.compare_exchange(UNMAPPED, physical, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                self.count.fetch_add(1, Ordering::AcqRel);
                true
            }
            Err(current) if current == physical => true,
            Err(_) => {
                // `logical` already names another CPU: give the claim back.
                if claimed {
                    reverse.store(UNMAPPED, Ordering::Release);
                }
                false
            }
        }
    }

    /// The physical id of `logical`, if published.
    pub fn physical(&self, logical: usize) -> Option<usize> {
        let id = self.physical.get(logical)?.load(Ordering::Acquire);
        (id != UNMAPPED).then_some(id)
    }

    /// The logical hart that owns `physical`, if published.
    ///
    /// Only the forward slot completes a mapping, so a claim whose publisher
    /// has not finished (or is about to give it back) is not reported.
    pub fn logical(&self, physical: usize) -> Option<usize> {
        let logical = self.logical.get(physical)?.load(Ordering::Acquire);
        (self.physical(logical) == Some(physical)).then_some(logical)
    }

    /// Logical slots published so far.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }
}

impl<const N: usize, const P: usize> Default for CpuTopology<N, P> {
    fn default() -> Self {
        Self::new()
    }
}

/// RISC-V numbering: the boot hart is logical 0 and the hart it displaced takes
/// its number; every other hart keeps its own (`0 ↔ boot`, identity elsewhere).
/// With two harts this is the former `boot ^ 1` swap.
#[cfg(any(target_arch = "riscv64", test))]
pub const fn boot_swapped_physical(boot: usize, logical: usize) -> usize {
    if logical == 0 {
        boot
    } else if logical == boot {
        0
    } else {
        logical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_physical_round_trip() {
        let map = CpuTopology::<4, 256>::new();
        assert!(map.publish(0, 17));
        assert!(map.publish(1, 3));
        assert!(map.publish(3, 255));
        assert_eq!(map.count(), 3);
        for (logical, physical) in [(0, 17), (1, 3), (3, 255)] {
            assert_eq!(map.physical(logical), Some(physical));
            assert_eq!(map.logical(physical), Some(logical));
        }
        assert_eq!(map.physical(2), None);
        assert_eq!(map.logical(0), None);
    }

    #[test]
    fn out_of_range_ids_are_refused() {
        let map = CpuTopology::<2, 4>::new();
        assert!(!map.publish(2, 0), "logical id at the bound");
        assert!(!map.publish(usize::MAX, 0));
        assert!(!map.publish(0, 4), "physical id at the bound");
        assert!(!map.publish(0, UNMAPPED));
        assert_eq!(map.count(), 0);
        assert_eq!(map.physical(2), None);
        assert_eq!(map.logical(4), None);
        assert_eq!(map.logical(usize::MAX), None);
    }

    #[test]
    fn slots_are_write_once_and_the_map_stays_a_bijection() {
        let map = CpuTopology::<4, 8>::new();
        assert!(map.publish(1, 5));
        assert!(map.publish(1, 5), "re-publishing the same pair is accepted");
        assert_eq!(map.count(), 1, "an idempotent re-publish is not a new slot");
        assert!(!map.publish(1, 6), "a logical hart cannot move");
        assert_eq!(map.logical(6), None, "the refused claim was given back");
        assert!(!map.publish(2, 5), "a physical id cannot have two owners");
        assert_eq!(map.physical(2), None);
        assert!(map.publish(2, 6), "the given-back id is still publishable");
        assert_eq!(map.physical(1), Some(5));
        assert_eq!(map.logical(5), Some(1));
        assert_eq!(map.count(), 2);
    }

    #[test]
    fn rv64_boot_hart_swap_matches_the_former_xor_mapping() {
        for boot in 0..2 {
            let map = CpuTopology::<2, 2>::new();
            for logical in 0..2 {
                assert!(map.publish(logical, boot_swapped_physical(boot, logical)));
            }
            assert_eq!(map.physical(0), Some(boot));
            assert_eq!(map.physical(1), Some(boot ^ 1));
            assert_eq!(map.logical(boot), Some(0));
            assert_eq!(map.logical(boot ^ 1), Some(1));
        }
        // Beyond two harts the swap is a transposition of 0 and the boot hart.
        let map = CpuTopology::<4, 4>::new();
        for logical in 0..4 {
            assert!(map.publish(logical, boot_swapped_physical(2, logical)));
        }
        assert_eq!(
            [0, 1, 2, 3].map(|logical| map.physical(logical)),
            [Some(2), Some(1), Some(0), Some(3)]
        );
    }

    #[test]
    fn the_kernel_map_is_sized_by_max_harts() {
        assert_eq!(CPU_TOPOLOGY.physical.len(), super::super::MAX_HARTS);
        assert_eq!(CPU_TOPOLOGY.logical.len(), PHYSICAL_ID_LIMIT);
    }
}
