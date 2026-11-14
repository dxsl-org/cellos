//! Static lock ranks (architecture remediation Phase 10).
//!
//! Every `static` [`Spinlock`](super::Spinlock) is built with
//! [`Spinlock::ranked`](super::Spinlock::ranked) and a rank from the table
//! below. A hart may only acquire a ranked lock whose rank is strictly greater
//! than every ranked lock it already holds; the order of the table *is* the
//! lock order. Instance locks (fields of per-object structs) stay unranked.
//!
//! The check runs only in the test posture (`test-hooks`) and in host tests:
//! there, acquiring out of order panics with both lock names before spinning,
//! so an inversion is reported instead of deadlocking. Dev and release kernels
//! store no rank and do no work (`docs/generated/lock-ranks.md` is generated
//! from this table).

/// A lock's position in the global acquisition order; 0 is unranked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rank(pub(crate) u16);

impl Rank {
    pub(crate) const UNRANKED: Rank = Rank(0);

    #[cfg_attr(not(any(feature = "test-hooks", test)), allow(dead_code))]
    pub(crate) const fn name(self) -> &'static str {
        NAMES[self.0 as usize]
    }
}

macro_rules! lock_ranks {
    ($($name:ident),+ $(,)?) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
        #[repr(u16)]
        enum Index { Unranked, $($name),+ }
        $(
            #[allow(dead_code)]
            pub const $name: Rank = Rank(Index::$name as u16);
        )+
        /// Lock names by rank, for violation reports and the generated table.
        pub const NAMES: &[&str] = &["(unranked)", $(stringify!($name)),+];
    };
}

// Acquisition order, outermost first. A lock may be taken while holding only
// locks listed above it.
lock_ranks! {
    VM_REGISTRY,
    SVM_VM_REGISTRY,
    CONSOLE_DRV,
    PENDING_ASCII,
    PCI_DEVICES,
    PS2_STATE,
    UART_SERIAL,
    VIFS1,
    TEST_STATE_LOCK,
    IOMMU_CLEANUP_SERIAL,
    DMA_EXPORTS,
    PAGE_GRANT_TABLE,
    REG_GRANT_TABLE,
    SCHEDULER,
    DEATH_SUBSCRIBERS,
    SWAP_CEILINGS,
    ROLE_PUBLICATION,
    OWNERS,
    SERVICE_REGISTRY,
    CELL_AUTHORITIES,
    MEASUREMENT_LOG,
    RT_HEAP,
    PAYLOAD_SCRATCH,
    NET_RX_RESERVATION,
    RISCV_DOMAINS,
    UNITS,
    ACPI_TOPOLOGY,
    FRAME_ALLOCATOR,
    UART_CONSOLE,
    KERNEL_ROOT,
    ADMISSION_REGISTRY,
    PENDING_PS2,
    KEYBOARD_ACK,
    PSCID_FREE_LIST,
    MMC_DEVICE,
    VIRTIO_GATE_REFUSED,
    DRIVER_REGISTRY,
    UART_RX_BUFFER,
    GOVERNED_PENDING,
    TRUSTED_PENDING,
    CELL_TABLE,
    ACPI_POWER_FACTS,
    X86_MACHINE_FACTS,
    ACCOUNTING,
    AUDIT_WRITE,
    NEXT_PIPE_ID,
    NEXT_FREEZE_NONCE,
    PIPES,
    NIC_DRIVER_STATE,
    RETIREMENT_VERDICTS,
    BLOCK_ROUTES,
    CSPACE_FILES,
    STATE_STASH,
    FUTEX_WAITERS,
    QUARANTINED_FRAMES,
    LIVE_ASIDS,
    DEFERRED_RELEASE,
    ABANDONED_TAGS,
    P01_LIFETIMES,
    P01_NONCPU_SNAPSHOTS,
    P01_NONCPU_TOPOLOGY_SNAPSHOTS,
    P01_NONCPU_TOPOLOGY_TEST_SNAPSHOT,
    P01_TOPOLOGY_SNAPSHOTS,
    P01_VTD_SNAPSHOTS,
    P01_VTD_TEST_SNAPSHOT,
    POLICY,
    RESOURCE_REGISTRY,
    DISPLAY_FRAMEBUFFER,
    PCIE_BARS,
    SNAPSHOT_SCRATCH,
    SHM_HANDLES,
    DMA_PUBLICATIONS,
    NEXT_CELL_GENERATION,
    RACE_SPACE,
    IOMMU_CLEANUP_RETRIES,
    CQ_TRANSACTION,
    PCIE_MSI_STATE,
    BDF_OWNERS,
    LOG_RING,
    PIN_REGISTRY,
    FRAME_RUN_PENDING,
    FB_STATE,
    AP02_MAPPINGS,
    QUOTA_LIMITS,
    SUPERVISOR_RANGES,
    TEST_FLUSHED_TAGS,
    TEST_FLUSHED_PAGES,
}

const _: () = assert!(NAMES.len() <= 128, "the held-rank mask is 128 bits");

#[cfg(any(feature = "test-hooks", test))]
pub(crate) use check::{acquired, before_acquire, released};

#[cfg(any(feature = "test-hooks", test))]
mod check {
    use super::{Rank, NAMES};

    /// Highest rank set in `held`, if any.
    fn highest(held: u128) -> Option<usize> {
        (held != 0).then(|| 127 - held.leading_zeros() as usize)
    }

    /// Panics if `rank` may not be acquired while holding `held`.
    pub(super) fn check(held: u128, rank: Rank) {
        if rank == Rank::UNRANKED {
            return;
        }
        if let Some(top) = highest(held) {
            if top >= rank.0 as usize {
                panic!(
                    "[lockrank] acquiring {} (rank {}) while holding {} (rank {})",
                    rank.name(),
                    rank.0,
                    NAMES[top],
                    top
                );
            }
        }
    }

    #[cfg(not(test))]
    mod held {
        use core::sync::atomic::{AtomicU64, Ordering};

        const HARTS: usize = 64;
        #[allow(clippy::declare_interior_mutable_const)]
        const ZERO: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
        /// Ranks held per hart. Only the owning hart touches its slot, with
        /// interrupts masked (every ranked lock masks them), so `Relaxed`.
        static HELD: [[AtomicU64; 2]; HARTS] = [ZERO; HARTS];

        /// This hart's slot; `None` before its hart-local state is installed
        /// (it would otherwise share hart 0's slot and race it).
        fn slot() -> Option<&'static [AtomicU64; 2]> {
            super::super::super::installed_hart_id().and_then(|hart| HELD.get(hart))
        }

        pub(in super::super) fn get() -> u128 {
            slot().map_or(0, |s| {
                u128::from(s[0].load(Ordering::Relaxed))
                    | (u128::from(s[1].load(Ordering::Relaxed)) << 64)
            })
        }

        pub(in super::super) fn set(mask: u128) {
            if let Some(s) = slot() {
                s[0].store(mask as u64, Ordering::Relaxed);
                s[1].store((mask >> 64) as u64, Ordering::Relaxed);
            }
        }
    }

    // Host tests run on many threads that share no hart; keep the mask per
    // thread so concurrent tests cannot see each other's locks.
    #[cfg(test)]
    mod held {
        extern crate std;
        use core::cell::Cell;

        std::thread_local!(static HELD: Cell<u128> = const { Cell::new(0) });

        pub(in super::super) fn get() -> u128 {
            HELD.with(Cell::get)
        }

        pub(in super::super) fn set(mask: u128) {
            HELD.with(|held| held.set(mask));
        }
    }

    /// Called with interrupts masked, before spinning on a `lock()`.
    pub(crate) fn before_acquire(rank: Rank) {
        check(held::get(), rank);
    }

    /// Records `rank` as held by this hart once the lock is owned.
    pub(crate) fn acquired(rank: Rank) {
        if rank != Rank::UNRANKED {
            held::set(held::get() | (1u128 << rank.0));
        }
    }

    /// Clears `rank` when its guard drops or the lock is force-unlocked; guards
    /// may drop in any order.
    pub(crate) fn released(rank: Rank) {
        if rank != Rank::UNRANKED {
            held::set(held::get() & !(1u128 << rank.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::Spinlock;

    #[test]
    fn ranks_follow_table_order() {
        assert_eq!(Rank::UNRANKED.0, 0);
        for (index, name) in NAMES.iter().enumerate().skip(1) {
            assert_eq!(Rank(index as u16).name(), *name);
        }
    }

    #[test]
    fn ascending_acquisition_is_allowed_and_any_release_order_works() {
        let outer = Spinlock::ranked((), Rank(1));
        let inner = Spinlock::ranked((), Rank(2));
        let a = outer.lock();
        let b = inner.lock();
        drop(a);
        drop(b);
        let _again = outer.lock();
    }

    #[test]
    #[should_panic(expected = "[lockrank] acquiring")]
    fn descending_acquisition_panics() {
        let outer = Spinlock::ranked((), Rank(1));
        let inner = Spinlock::ranked((), Rank(2));
        let _b = inner.lock();
        let _a = outer.lock();
    }

    #[test]
    #[should_panic(expected = "[lockrank] acquiring")]
    fn same_rank_twice_panics() {
        let first = Spinlock::ranked((), Rank(3));
        let second = Spinlock::ranked((), Rank(3));
        let _a = first.lock();
        let _b = second.lock();
    }

    #[test]
    fn unranked_and_try_lock_are_not_checked() {
        let ranked = Spinlock::ranked((), Rank(2));
        let unranked = Spinlock::new(());
        let lower = Spinlock::ranked((), Rank(1));
        let _r = ranked.lock();
        let _u = unranked.lock();
        assert!(lower.try_lock().is_some());
    }
}
