//! Private allocation-lifetime root inventory, not DMA/VM authority or a tree walk.
//! A registry-empty cutoff is insufficient: unpublished nested roots and the
//! intentionally non-freeing IOMMU tables remain in this birth-witness ledger.
use crate::sync::Spinlock;
use core::sync::atomic::{AtomicU64, Ordering};

pub(crate) const ROOT_CAPACITY: usize = 128;
const REFERENCE_CAPACITY: usize = 256;
const UNSUPPORTED: u64 = 1;
const CAPACITY: u64 = 2;
const LIFETIME: u64 = 4;
const REFERENCE: u64 = 8;
const HISTORY: u64 = 16;
const INFLIGHT: u64 = 32;

struct Lifetimes {
    epoch: u64,
    births: u64,
    invalid: u64,
    // token, PA, kind, physical incarnation (filled at cutoff), backing,
    // actual heap pointer, actual heap incarnation, reference count at cutoff.
    roots: [[u64; 8]; ROOT_CAPACITY],
}
impl Lifetimes {
    const fn new() -> Self {
        Self { epoch: 0, births: 0, invalid: 0, roots: [[0; 8]; ROOT_CAPACITY] }
    }
    fn advance(&mut self) -> Option<u64> {
        match self.epoch.checked_add(1) {
            Some(epoch) => { self.epoch = epoch; Some(epoch) }
            None => { self.invalid |= LIFETIME; None }
        }
    }
    fn create(&mut self, pa: u64, kind: u64, ptr: u64, incarnation: u64) {
        if pa == 0 || pa & 4095 != 0 || pa >= 1 << 52 || !(1..=6).contains(&kind)
            || (kind <= 2 && (ptr != 0 || incarnation != 0))
            || (kind >= 3 && (ptr == 0 || ptr & 4095 != 0 || incarnation == 0))
            || self.roots.iter().any(|row| row[0] != 0 && row[1] == pa) {
            self.invalid |= LIFETIME;
            return;
        }
        let Some(epoch) = self.advance() else { return; };
        let Some(births) = self.births.checked_add(1) else {
            self.invalid |= LIFETIME; return;
        };
        self.births = births;
        let Some(slot) = self.roots.iter_mut().find(|row| row[0] == 0) else {
            self.invalid |= CAPACITY; return;
        };
        *slot = [epoch, pa, kind, 0, if kind <= 2 { 1 } else { 2 }, ptr, incarnation, 0];
    }
    fn release(&mut self, pa: u64) {
        let Some(index) = self.roots.iter().position(|row| row[0] != 0 && row[1] == pa) else {
            self.invalid |= LIFETIME; return;
        };
        // Only NestedPageTable::Drop frees storage. IOMMU types have no freeing
        // destructor, so their allocations stay visible after Rust handles die.
        if self.roots[index][2] > 2 { self.invalid |= LIFETIME; return; }
        if self.advance().is_some() { self.roots[index] = [0; 8]; }
    }
}
static LIFETIMES: Spinlock<Lifetimes> = Spinlock::new(Lifetimes::new());
static ACTIVITIES: AtomicU64 = AtomicU64::new(0);
pub(crate) struct Activity { _not_send: core::marker::PhantomData<*mut ()> }
pub(crate) fn activity() -> Activity {
    if ACTIVITIES.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| count.checked_add(1)).is_err() {
        LIFETIMES.lock().invalid |= LIFETIME;
    }
    Activity { _not_send: core::marker::PhantomData }
}
impl Drop for Activity {
    fn drop(&mut self) {
        if ACTIVITIES.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| count.checked_sub(1)).is_err() {
            LIFETIMES.lock().invalid |= LIFETIME;
        }
    }
}
pub(crate) fn created_frame(pa: u64, kind: u64) { LIFETIMES.lock().create(pa, kind, 0, 0); }
pub(crate) fn created_heap(pa: u64, kind: u64, ptr: usize) {
    // Heap lock is released before the lifetime lock; no allocator hook calls us.
    let incarnation = crate::memory::heap::p01_owner::allocation_incarnation(ptr).unwrap_or(0);
    LIFETIMES.lock().create(pa, kind, ptr as u64, incarnation);
}
pub(crate) fn released(pa: u64) { LIFETIMES.lock().release(pa); }

struct Snapshot {
    source: [u64; 16],
    roots: [[u64; 8]; ROOT_CAPACITY],
    // root token, reference role, holder key, object key, parent root PA.
    references: [[u64; 5]; REFERENCE_CAPACITY],
}
impl Snapshot {
    const fn new() -> Self {
        Self { source: [0; 16], roots: [[0; 8]; ROOT_CAPACITY], references: [[0; 5]; REFERENCE_CAPACITY] }
    }
    fn refuse(&mut self, reason: u64) {
        self.source[6] = 0; self.source[7] = 0; self.source[10] = 4; self.source[11] |= reason;
    }
    fn reference(&mut self, pa: u64, role: u64, holder: u64, object: u64, parent: u64) -> Result<(), u64> {
        let index = self.roots[..self.source[6] as usize].iter().position(|row| row[1] == pa).ok_or(REFERENCE)?;
        let root = &mut self.roots[index];
        if !matches!((root[2], role), (1 | 2, 1) | (5, 2) | (6, 3) | (4, 4)) { return Err(REFERENCE); }
        let count = self.source[7] as usize;
        if count == REFERENCE_CAPACITY { return Err(CAPACITY); }
        let row = [root[0], role, holder, object, parent];
        if self.references[..count].contains(&row) { return Err(REFERENCE); }
        root[7] = root[7].checked_add(1).ok_or(CAPACITY)?;
        self.references[count] = row;
        self.source[7] += 1;
        Ok(())
    }
}
static SNAPSHOTS: Spinlock<[Snapshot; 6]> = Spinlock::new([const { Snapshot::new() }; 6]);
pub(crate) fn static_bytes() -> usize {
    core::mem::size_of_val(&LIFETIMES) + core::mem::size_of_val(&ACTIVITIES) + core::mem::size_of_val(&SNAPSHOTS)
}

#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn heap_cutoff_matches(metrics: &[crate::p01_resources::Metric; 16]) -> bool {
    let (used, _, epoch, _) = crate::memory::heap::p01_snapshot();
    metrics[4].status == 1 && metrics[4].epoch_before == metrics[4].epoch_after
        && metrics[4].epoch_before == epoch as u64 && metrics[4].value == used as u64
}

#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn capture(sample: usize, metrics: &[crate::p01_resources::Metric; 16]) {
    // Caller holds the CPU topology's local-IRQ exclusion, with no frame/heap/
    // scheduler/kernel-root lock. Registry visitors precede FRAME_ALLOCATOR.
    let mut storage = SNAPSHOTS.lock();
    let Some(snapshot) = sample.checked_sub(1).and_then(|index| storage.get_mut(index)) else { return; };
    if snapshot.source[0] != 0 {
        crate::p01_measurement::invalidate(crate::p01_measurement::PROTOCOL); return;
    }
    let online = crate::task::smp::online_hart_count() as u64;
    let lifetimes = LIFETIMES.lock();
    snapshot.source = [2, sample as u64, lifetimes.epoch, metrics[1].epoch_before,
        metrics[1].value, online, 0, 0, ROOT_CAPACITY as u64, REFERENCE_CAPACITY as u64,
        4, lifetimes.invalid, ACTIVITIES.load(Ordering::Acquire), lifetimes.births, 7,
        crate::memory::frame::phys_to_virt(0) as u64];
    let mut count = 0usize;
    for row in lifetimes.roots.iter().filter(|row| row[0] != 0) {
        snapshot.roots[count] = *row; count += 1;
    }
    drop(lifetimes);
    snapshot.source[6] = count as u64;
    #[cfg(target_arch = "x86_64")]
    let ready = crate::task::smp::x86_scheduler_ready();
    #[cfg(not(target_arch = "x86_64"))]
    let ready = false;
    if online != 1 || !ready { snapshot.refuse(UNSUPPORTED); return; }
    if snapshot.source[11] != 0 { snapshot.refuse(LIFETIME); return; }
    if snapshot.source[12] != 0 { snapshot.refuse(INFLIGHT); return; }
    #[cfg(target_arch = "x86_64")]
    {
        let mut error = 0;
        crate::hypervisor::svm_registry::p01_visit_roots(|pa, holder, object| {
            if let Err(reason) = snapshot.reference(pa, 1, holder, object, 0) { error |= reason; }
        });
        crate::task::drivers::iommu_x86::p01_visit_roots(|pa, role, holder, object, parent| {
            if let Err(reason) = snapshot.reference(pa, role, holder, object, parent) { error |= reason; }
        });
        if error != 0 { snapshot.refuse(error); return; }
    }
    #[cfg(not(target_arch = "x86_64"))]
    { snapshot.refuse(UNSUPPORTED); return; }
    // Validate heap births before FRAME_ALLOCATOR; never nest its lock with heap.
    if !heap_cutoff_matches(metrics) { snapshot.refuse(HISTORY); return; }
    for row in snapshot.roots.iter().take(count).filter(|row| row[2] >= 3) {
        if row[5].checked_sub(snapshot.source[15]) != Some(row[1])
            || crate::memory::heap::p01_owner::allocation_incarnation(row[5] as usize) != Some(row[6]) {
            snapshot.refuse(LIFETIME); return;
        }
    }
    let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
    let Some(allocator) = frames.as_ref() else { snapshot.refuse(HISTORY); return; };
    if metrics[1].status != 1 || metrics[1].epoch_before != metrics[1].epoch_after
        || allocator.p01_snapshot().0 != snapshot.source[3]
        || allocator.used_frames() as u64 != snapshot.source[4] {
        snapshot.refuse(HISTORY); return;
    }
    for index in 0..count {
        if !allocator.p01_allocated(snapshot.roots[index][1] as usize) {
            snapshot.refuse(HISTORY); return;
        }
        let Some(incarnation) = crate::p01_retained::physical_incarnation_at(snapshot.roots[index][1], snapshot.source[3]) else {
            snapshot.refuse(HISTORY); return;
        };
        snapshot.roots[index][3] = incarnation;
    }
    snapshot.source[10] = 1;
}
pub(crate) fn source(sample: usize) -> Option<[u64; 16]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[0] != 0).then_some(snapshot.source)
}
pub(crate) fn root(sample: usize, index: usize) -> Option<[u64; 8]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < snapshot.source[6] as usize).then(|| snapshot.roots[index])
}
pub(crate) fn reference(sample: usize, index: usize) -> Option<[u64; 5]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < snapshot.source[7] as usize).then(|| snapshot.references[index])
}
pub(crate) fn visit_snapshot(sample: usize, visit: impl FnOnce([u64; 16], &[[u64; 8]])) {
    let storage = SNAPSHOTS.lock();
    if let Some(snapshot) = sample.checked_sub(1).and_then(|index| storage.get(index)) {
        visit(snapshot.source, &snapshot.roots[..snapshot.source[6] as usize]);
    }
}
/// Immutable census and registry references, under the same snapshot lock.
pub(crate) fn visit_structure_snapshot(sample: usize,
    visit: impl FnOnce([u64; 16], &[[u64; 8]], &[[u64; 5]])) {
    let storage = SNAPSHOTS.lock();
    if let Some(snapshot) = sample.checked_sub(1).and_then(|index| storage.get(index)) {
        visit(snapshot.source, &snapshot.roots[..snapshot.source[6] as usize],
            &snapshot.references[..snapshot.source[7] as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_reuse_has_new_birth_identity_and_retirement_advances_epoch() {
        let mut ledger = Lifetimes::new();
        ledger.create(4096, 1, 0, 0);
        let birth = ledger.roots[0][0];
        ledger.release(4096);
        ledger.create(4096, 2, 0, 0);
        assert!(ledger.roots[0][0] > birth);
        assert_eq!(ledger.births, 2);
        assert_eq!(ledger.invalid, 0);
    }
    #[test]
    fn duplicate_birth_missing_free_and_nonfreeing_heap_roots_refuse() {
        for violation in 0..3 {
            let mut ledger = Lifetimes::new();
            ledger.create(4096, 4, 8192, 7);
            match violation {
                0 => ledger.create(4096, 1, 0, 0),
                1 => ledger.release(8192),
                _ => ledger.release(4096),
            }
            assert_eq!(ledger.invalid, LIFETIME);
            assert_eq!(ledger.roots[0][6], 7);
        }
    }
    #[test]
    fn live_root_capacity_and_birth_epoch_exhaustion_are_sticky() {
        let mut ledger = Lifetimes::new();
        for index in 1..=ROOT_CAPACITY + 1 { ledger.create(index as u64 * 4096, 1, 0, 0); }
        assert_eq!(ledger.invalid, CAPACITY);
        ledger.release(4096);
        assert_eq!(ledger.invalid, CAPACITY);
        ledger.epoch = u64::MAX;
        ledger.create(4096, 1, 0, 0);
        assert_eq!(ledger.invalid, CAPACITY | LIFETIME);
    }
    #[test]
    fn registry_references_require_actual_birth_and_correct_root_family() {
        let mut snapshot = Snapshot::new();
        snapshot.source[6] = 1;
        snapshot.roots[0] = [3, 4096, 1, 0, 1, 0, 0, 0];
        assert_eq!(snapshot.reference(8192, 1, 4, 1, 0), Err(REFERENCE));
        assert_eq!(snapshot.reference(4096, 4, 4, 1, 0), Err(REFERENCE));
        assert_eq!(snapshot.reference(4096, 1, 4, 1, 0), Ok(()));
        assert_eq!(snapshot.reference(4096, 1, 4, 1, 0), Err(REFERENCE));
        assert_eq!(snapshot.roots[0][7], 1);
        snapshot.refuse(REFERENCE);
        assert_eq!(&snapshot.source[6..8], &[0, 0]);
    }
    #[test]
    fn vtd_root_and_context_birth_families_have_distinct_registry_roles() {
        let mut ledger = Lifetimes::new();
        ledger.create(4096, 5, 0x101000, 7);
        ledger.create(8192, 6, 0x102000, 8);
        assert_eq!(ledger.invalid, 0);
        let mut snapshot = Snapshot::new();
        snapshot.source[6] = 2;
        snapshot.roots[..2].copy_from_slice(&ledger.roots[..2]);
        assert_eq!(snapshot.reference(4096, 3, 0, 0, 4096), Err(REFERENCE));
        assert_eq!(snapshot.reference(8192, 2, 0, 0, 0), Err(REFERENCE));
        assert_eq!(snapshot.reference(4096, 2, 0, 0, 0), Ok(()));
        assert_eq!(snapshot.reference(8192, 3, 255, 0, 4096), Ok(()));
        assert_ne!(snapshot.roots[0][0], snapshot.roots[1][0]);
    }
}
