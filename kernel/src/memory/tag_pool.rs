//! One allocator for the architectural tags that name a translation context:
//! private-root ASIDs/PCIDs, AArch64 Stage-2 VMIDs and x86 SVM ASIDs (census
//! §8.5).
//!
//! A [`TagPool`] hands out each value in `first..first + count` to at most one
//! [`TagLease`] at a time, and at most `per_owner` leases to one incarnation
//! ([`Charge`]). [`Charge::KERNEL`] is exempt from the owner limit: kernel
//! fixtures and kernel-owned roots are bounded by the pool alone.
//!
//! Reuse is the pool's contract, not the caller's:
//!
//! - [`Reuse::AfterInvalidation`]: a value goes back only through
//!   [`TagLease::release`], which takes an [`Invalidated`] proof for exactly
//!   that value. The proof exists only once the invalidation that drops every
//!   TLB entry tagged with it, on every hart, has completed.
//! - [`Reuse::Never`]: a value is issued once per boot.
//!
//! A lease dropped without `release` or `cancel` keeps its value reserved and
//! its owner counted for the rest of the boot. That is the fail-closed answer
//! for a tag whose invalidation was never confirmed: it is never silently
//! reissued.
//!
//! Lock-free: every operation is a CAS or a fetch on one word, so a release
//! from the hart-0 timer reaper or under any spinlock takes no lock rank.

use super::cell_quota::{Charge, MAX_CELLS};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// When a value may be handed out again after its lease ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reuse {
    /// Only after an invalidation proved no TLB entry tagged with it remains.
    AfterInvalidation,
    /// Never within a boot.
    Never,
}

/// Why [`TagPool::acquire`] refused. Nothing was taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TagRefusal {
    /// Every value is held (or, for [`Reuse::Never`], was issued).
    Exhausted,
    /// The owner already holds `per_owner` leases, or names a CellId the pool
    /// cannot count.
    OwnerLimit,
}

/// Proof that every TLB entry tagged with one value is gone on every hart that
/// could hold one.
///
/// Minted only by an invalidation that completed for that value:
/// `tlb_shootdown::confirm_tag_invalidation` (local flush plus every online
/// hart's acknowledgement), the frame-run reaper once a tag wait's epochs are
/// all published (`frame_run::Pending::complete`), and AArch64
/// `s2_tlb_flush_all` (`TLBI ALLE1IS`, which covers every VMID).
#[derive(Debug)]
pub(crate) struct Invalidated {
    tag: u32,
}

impl Invalidated {
    /// The proof for `tag`. Call only right after the invalidation that covers
    /// `tag` completed (see [`Invalidated`]).
    pub(crate) const fn of(tag: u32) -> Self {
        Self { tag }
    }
}

/// Most values an [`Reuse::AfterInvalidation`] pool tracks (its bitmap).
const MAX_REUSED: u32 = 256;
const BITMAP_WORDS: usize = (MAX_REUSED / 64) as usize;

/// How many values a pool holds.
enum Span {
    Fixed(u32),
    /// Read at each acquire (a CPUID-sized range); constant for the boot.
    #[allow(dead_code)] // reason: only the x86 SVM ASID pool is sized at run time
    Runtime(fn() -> u32),
}

/// A bounded set of tags with a per-owner limit. See the module docs.
pub(crate) struct TagPool {
    first: u32,
    span: Span,
    reuse: Reuse,
    per_owner: u32,
    /// [`Reuse::AfterInvalidation`]: bit `i` set while `first + i` is held.
    used: [AtomicU64; BITMAP_WORDS],
    /// [`Reuse::Never`]: how many values were issued.
    issued: AtomicU32,
    /// Per CellId: the holding incarnation's generation (low 32 bits) in bits
    /// `32..64`, its lease count in bits `0..32`. A row naming another
    /// generation counts 0 for this one, and a release by a retired
    /// incarnation leaves the next one's count alone.
    held: [AtomicU64; MAX_CELLS],
}

impl TagPool {
    /// `count` values from `first`, reissued only with an [`Invalidated`]
    /// proof; at most `per_owner` held by one incarnation.
    pub(crate) const fn after_invalidation(first: u32, count: u32, per_owner: u32) -> Self {
        assert!(count <= MAX_REUSED);
        Self::new(
            first,
            Span::Fixed(count),
            Reuse::AfterInvalidation,
            per_owner,
        )
    }

    /// `count()` values from `first`, each issued once per boot; at most
    /// `per_owner` held by one incarnation.
    #[allow(dead_code)] // reason: the x86 SVM ASID pool is the only never-reused pool
    pub(crate) const fn never_reused(first: u32, count: fn() -> u32, per_owner: u32) -> Self {
        Self::new(first, Span::Runtime(count), Reuse::Never, per_owner)
    }

    const fn new(first: u32, span: Span, reuse: Reuse, per_owner: u32) -> Self {
        Self {
            first,
            span,
            reuse,
            per_owner,
            used: [const { AtomicU64::new(0) }; BITMAP_WORDS],
            issued: AtomicU32::new(0),
            held: [const { AtomicU64::new(0) }; MAX_CELLS],
        }
    }

    /// Values in the pool.
    pub(crate) fn capacity(&self) -> u32 {
        match self.span {
            Span::Fixed(count) => count,
            Span::Runtime(count) => count(),
        }
    }

    /// Lease the lowest free value (the next unissued one for
    /// [`Reuse::Never`]) to `owner`.
    ///
    /// The owner limit is checked first, so an owner at its limit is refused
    /// with [`TagRefusal::OwnerLimit`] even when the pool is also exhausted. A
    /// refusal takes nothing. While an acquire of the same owner is in flight
    /// and about to fail on exhaustion, another may transiently read that
    /// owner one lease higher.
    pub(crate) fn acquire(&'static self, owner: Charge) -> Result<TagLease, TagRefusal> {
        self.count_owner(owner)?;
        match self.claim() {
            Some(value) => Ok(TagLease {
                pool: self,
                value,
                owner,
            }),
            None => {
                self.uncount_owner(owner);
                Err(TagRefusal::Exhausted)
            }
        }
    }

    /// Values free now; a snapshot, not a claim. For [`Reuse::Never`], the
    /// values not yet issued.
    #[allow(dead_code)] // reason: read by the VM boot fixtures and host tests only
    pub(crate) fn free_count(&self) -> usize {
        let capacity = self.capacity();
        let taken = match self.reuse {
            Reuse::AfterInvalidation => self
                .used
                .iter()
                .map(|word| word.load(Ordering::Relaxed).count_ones())
                .sum::<u32>(),
            Reuse::Never => self.issued.load(Ordering::Relaxed),
        };
        capacity.saturating_sub(taken) as usize
    }

    fn row(&self, owner: Charge) -> Option<&AtomicU64> {
        usize::try_from(owner.cell().0)
            .ok()
            .and_then(|cell| self.held.get(cell))
    }

    fn count_owner(&self, owner: Charge) -> Result<(), TagRefusal> {
        if is_kernel(owner) {
            return Ok(());
        }
        let row = self.row(owner).ok_or(TagRefusal::OwnerLimit)?;
        let generation = generation_bits(owner);
        row.fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
            let held = if word >> 32 == generation {
                word as u32
            } else {
                0
            };
            (held < self.per_owner).then(|| (generation << 32) | u64::from(held + 1))
        })
        .map(drop)
        .map_err(|_| TagRefusal::OwnerLimit)
    }

    fn uncount_owner(&self, owner: Charge) {
        if is_kernel(owner) {
            return;
        }
        let Some(row) = self.row(owner) else {
            return;
        };
        let generation = generation_bits(owner);
        // A row now naming another incarnation is not this owner's to change.
        let _ = row.fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
            (word >> 32 == generation && word as u32 > 0).then(|| word - 1)
        });
    }

    fn claim(&self) -> Option<u32> {
        let count = self.capacity();
        match self.reuse {
            Reuse::Never => self
                .issued
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |issued| {
                    (issued < count && issued.checked_add(self.first).is_some()).then(|| issued + 1)
                })
                .ok()
                .map(|issued| self.first + issued),
            Reuse::AfterInvalidation => {
                for (index, word) in self.used.iter().enumerate() {
                    let base = index as u32 * 64;
                    if base >= count {
                        break;
                    }
                    let valid = match count - base {
                        64.. => u64::MAX,
                        bits => (1u64 << bits) - 1,
                    };
                    let mut current = word.load(Ordering::Relaxed);
                    loop {
                        let free = !current & valid;
                        if free == 0 {
                            break;
                        }
                        let bit = free.trailing_zeros();
                        // Acquire pairs with the Release in `give_back`: the
                        // previous holder's invalidation happens before this
                        // holder's first use.
                        match word.compare_exchange_weak(
                            current,
                            current | (1 << bit),
                            Ordering::Acquire,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => return Some(self.first + base + bit),
                            Err(seen) => current = seen,
                        }
                    }
                }
                None
            }
        }
    }

    /// Return `value` to the bitmap. A [`Reuse::Never`] value stays issued.
    fn give_back(&self, value: u32) {
        if self.reuse == Reuse::Never {
            return;
        }
        let index = value - self.first;
        let mask = 1u64 << (index % 64);
        let previous = self.used[(index / 64) as usize].fetch_and(!mask, Ordering::Release);
        debug_assert!(previous & mask != 0, "tag {value} released while free");
    }
}

fn is_kernel(owner: Charge) -> bool {
    owner.cell() == Charge::KERNEL.cell()
}

fn generation_bits(owner: Charge) -> u64 {
    u64::from(owner.generation() as u32)
}

/// One value of a [`TagPool`], held by one owner.
///
/// Ends through [`Self::release`] (with the proof that the value is clean) or
/// [`Self::cancel`] (the value never reached hardware). Dropping it otherwise
/// keeps the value reserved and the owner counted for the rest of the boot.
#[must_use = "a dropped lease keeps its tag reserved and counted for the rest of the boot"]
pub(crate) struct TagLease {
    pool: &'static TagPool,
    value: u32,
    owner: Charge,
}

impl TagLease {
    /// The tag.
    pub(crate) fn value(&self) -> u32 {
        self.value
    }

    /// The incarnation the lease is counted against.
    #[cfg_attr(not(test), allow(dead_code))] // reason: host tests and diagnostics read it
    pub(crate) fn owner(&self) -> Charge {
        self.owner
    }

    /// End the lease after the invalidation `proof` stands for. The value
    /// becomes reusable (for [`Reuse::AfterInvalidation`]) and the owner's
    /// count drops. A proof for another value proves nothing: the value then
    /// stays reserved and counted.
    pub(crate) fn release(self, proof: Invalidated) {
        if proof.tag != self.value {
            log::error!(
                "[tag-pool] tag {} released with a proof for tag {}: retained",
                self.value,
                proof.tag
            );
            return;
        }
        self.pool.give_back(self.value);
        self.pool.uncount_owner(self.owner);
    }

    /// End a lease whose value was never programmed into any register or
    /// table a hart walks: no TLB entry can carry it under this lease, and the
    /// previous holder's release already proved it clean. The owner's count
    /// drops; a [`Reuse::Never`] value stays issued.
    pub(crate) fn cancel(self) {
        self.pool.give_back(self.value);
        self.pool.uncount_owner(self.owner);
    }
}

#[cfg(test)]
mod tests {
    use super::{Invalidated, TagLease, TagPool, TagRefusal};
    use crate::memory::cell_quota::Charge;
    use types::CellId;

    fn cell(id: u64, generation: u64) -> Charge {
        Charge::new(CellId(id), generation)
    }

    #[test]
    fn exhaustion_refuses_and_values_stay_in_range() {
        static POOL: TagPool = TagPool::after_invalidation(1, 255, 1);
        let pool = &POOL;
        let mut held = Vec::new();
        for expected in 1..=255 {
            let lease = pool.acquire(Charge::KERNEL).expect("free");
            assert_eq!(lease.value(), expected);
            held.push(lease);
        }
        assert_eq!(pool.free_count(), 0);
        assert!(matches!(
            pool.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
        // A refused cell takes nothing, not even a count: with a limit of 1
        // it still gets the next free value.
        assert!(matches!(
            pool.acquire(cell(3, 1)),
            Err(TagRefusal::Exhausted)
        ));
        let lease = held.pop().unwrap();
        let value = lease.value();
        lease.release(Invalidated::of(value));
        let again = pool.acquire(cell(3, 1)).expect("count was not leaked");
        assert_eq!(again.value(), value);
        assert_eq!(again.owner(), cell(3, 1));
    }

    #[test]
    fn an_owner_is_refused_past_its_limit_while_others_still_get_tags() {
        static POOL: TagPool = TagPool::after_invalidation(1, 255, 4);
        let pool = &POOL;
        let a = cell(5, 7);
        let held: Vec<TagLease> = (0..4).map(|_| pool.acquire(a).expect("free")).collect();
        assert!(matches!(pool.acquire(a), Err(TagRefusal::OwnerLimit)));
        assert_eq!(pool.free_count(), 251, "the refusal took no tag");
        // Another cell and the kernel are unaffected.
        assert!(pool.acquire(cell(6, 1)).is_ok());
        assert!(pool.acquire(Charge::KERNEL).is_ok());
        // The next incarnation of A's CellId starts at 0.
        assert!(pool.acquire(cell(5, 8)).is_ok());
        // A release returns one lease to A.
        for lease in held {
            let value = lease.value();
            lease.release(Invalidated::of(value));
        }
        assert!(pool.acquire(a).is_ok());
    }

    #[test]
    fn a_retired_incarnation_releasing_late_leaves_the_next_one_counted() {
        static POOL: TagPool = TagPool::after_invalidation(1, 8, 1);
        let pool = &POOL;
        let old = pool.acquire(cell(2, 1)).expect("free");
        let new = pool
            .acquire(cell(2, 2))
            .expect("a new incarnation counts from 0");
        let value = old.value();
        old.release(Invalidated::of(value));
        assert!(matches!(
            pool.acquire(cell(2, 2)),
            Err(TagRefusal::OwnerLimit)
        ));
        let value = new.value();
        new.release(Invalidated::of(value));
        assert!(pool.acquire(cell(2, 2)).is_ok());
    }

    #[test]
    fn the_kernel_is_exempt_from_the_owner_limit() {
        static POOL: TagPool = TagPool::after_invalidation(1, 255, 1);
        let pool = &POOL;
        let held: Vec<TagLease> = (0..255)
            .map(|_| {
                pool.acquire(Charge::KERNEL)
                    .expect("kernel is never limited")
            })
            .collect();
        assert_eq!(held.len(), 255);
        assert!(matches!(
            pool.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
    }

    #[test]
    fn an_owner_beyond_the_table_is_refused() {
        static POOL: TagPool = TagPool::after_invalidation(1, 8, 4);
        let pool = &POOL;
        let beyond = cell(crate::memory::cell_quota::MAX_CELLS as u64, 1);
        assert!(matches!(pool.acquire(beyond), Err(TagRefusal::OwnerLimit)));
        assert_eq!(pool.free_count(), 8);
    }

    #[test]
    fn a_released_tag_is_reused_only_with_its_proof() {
        static POOL: TagPool = TagPool::after_invalidation(1, 2, 4);
        let pool = &POOL;
        let a = pool.acquire(cell(1, 1)).unwrap();
        let b = pool.acquire(cell(1, 1)).unwrap();
        assert_eq!((a.value(), b.value()), (1, 2));
        // A proof for another value proves nothing: 1 stays reserved.
        a.release(Invalidated::of(2));
        assert!(matches!(
            pool.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
        b.release(Invalidated::of(2));
        let again = pool.acquire(Charge::KERNEL).expect("2 was proven clean");
        assert_eq!(again.value(), 2);
    }

    #[test]
    fn a_dropped_lease_stays_reserved_and_counted() {
        static POOL: TagPool = TagPool::after_invalidation(1, 2, 1);
        let pool = &POOL;
        let owner = cell(4, 1);
        drop(pool.acquire(owner).unwrap());
        assert_eq!(pool.free_count(), 1, "the dropped value is never reissued");
        assert!(matches!(pool.acquire(owner), Err(TagRefusal::OwnerLimit)));
        let other = pool.acquire(Charge::KERNEL).unwrap();
        assert_eq!(other.value(), 2);
        assert!(matches!(
            pool.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
    }

    #[test]
    fn a_cancelled_lease_returns_value_and_count() {
        static POOL: TagPool = TagPool::after_invalidation(1, 1, 1);
        let pool = &POOL;
        let owner = cell(4, 1);
        pool.acquire(owner).unwrap().cancel();
        let again = pool.acquire(owner).expect("value and count came back");
        assert_eq!(again.value(), 1);
    }

    #[test]
    fn concurrent_acquirers_never_share_a_tag() {
        static POOL: TagPool = TagPool::after_invalidation(1, 255, 64);
        let pool: &'static TagPool = &POOL;
        let threads: Vec<_> = (0..6u64)
            .map(|thread| {
                std::thread::spawn(move || {
                    // Two threads per owner race on the same row too.
                    let owner = if thread < 2 {
                        Charge::KERNEL
                    } else {
                        cell(10 + thread / 2, 1)
                    };
                    let mut held = Vec::new();
                    while let Ok(lease) = pool.acquire(owner) {
                        held.push(lease);
                    }
                    held
                })
            })
            .collect();
        let mut seen = [false; 256];
        let mut per_owner = std::collections::BTreeMap::new();
        for thread in threads {
            for lease in thread.join().unwrap() {
                let value = lease.value() as usize;
                assert!(!seen[value], "tag {value} handed out twice");
                seen[value] = true;
                *per_owner.entry(lease.owner().cell().0).or_insert(0) += 1;
            }
        }
        assert!(!seen[0] && seen[1..].iter().all(|&held| held));
        for (owner, held) in per_owner {
            assert!(owner == 0 || held <= 64, "cell {owner} held {held}");
        }
    }

    #[test]
    fn a_never_reused_pool_issues_each_value_once_per_boot() {
        fn four() -> u32 {
            4
        }
        static POOL: TagPool = TagPool::never_reused(1, four, 4);
        let pool = &POOL;
        let owner = cell(9, 1);
        let held: Vec<TagLease> = (1..=4)
            .map(|expected| {
                let lease = pool.acquire(owner).unwrap();
                assert_eq!(lease.value(), expected);
                lease
            })
            .collect();
        assert!(matches!(pool.acquire(owner), Err(TagRefusal::OwnerLimit)));
        assert!(matches!(
            pool.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
        // Ending a lease returns the owner's count but never the value.
        for lease in held {
            let value = lease.value();
            lease.release(Invalidated::of(value));
        }
        assert_eq!(pool.free_count(), 0);
        assert!(matches!(pool.acquire(owner), Err(TagRefusal::Exhausted)));
    }

    #[test]
    fn a_never_reused_pool_with_no_capacity_or_at_the_top_issues_nothing() {
        fn none() -> u32 {
            0
        }
        fn all() -> u32 {
            u32::MAX
        }
        static EMPTY: TagPool = TagPool::never_reused(1, none, 4);
        let empty = &EMPTY;
        assert!(matches!(
            empty.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
        // Value 0 is never issued from a pool starting at 1, and the value
        // never wraps past u32::MAX.
        static TOP: TagPool = TagPool::never_reused(1, all, 4);
        let top = &TOP;
        top.issued
            .store(u32::MAX - 1, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(top.acquire(Charge::KERNEL).unwrap().value(), u32::MAX);
        assert!(matches!(
            top.acquire(Charge::KERNEL),
            Err(TagRefusal::Exhausted)
        ));
    }
}
