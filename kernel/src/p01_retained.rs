//! Disposable private physical-retention observations; never allocator policy.
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use crate::p01_measurement::{self as trace, Owner};
use crate::p01_resources::{Metric, Source};

// Observation ceiling, NOT a proven workload bound or an admission limit.
// Fragmented frame inventories are dynamic. Exhaustion poisons the observation.
const CAPACITY: usize = 2048;
const PAGE: u64 = crate::memory::paging::PAGE_SIZE as u64;
static SOURCE: Source = Source::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token { slot: u16, serial: u64 }
#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum Reason {
    PinWithheld = 1, PinFull = 2, CacheSync = 3, Deferred = 4,
    AddressSpaceQuarantine = 5, TaskStackForget = 6, ThreadStackForget = 7,
    PrivateMapRollback = 10, StackMapRollback = 11, TaskStackUnmap = 12,
    PrivatePageUnmap = 13, GrantMapRollback = 14, GrantPageUnmap = 15,
    RootTeardown = 16, AsidLeaseRelease = 17,
}
impl Reason {
    pub(crate) fn deferred(reason: &str) -> Option<Self> {
        Some(match reason {
            "private map rollback invalidation unacknowledged" => Self::PrivateMapRollback,
            "stack map rollback invalidation unacknowledged" => Self::StackMapRollback,
            "task-stack unmap invalidation unacknowledged" => Self::TaskStackUnmap,
            "private-page unmap invalidation unacknowledged" => Self::PrivatePageUnmap,
            "grant map rollback invalidation unacknowledged" => Self::GrantMapRollback,
            "grant-page unmap invalidation unacknowledged" => Self::GrantPageUnmap,
            "root teardown invalidation unacknowledged" => Self::RootTeardown,
            "asid lease release invalidation unacknowledged" => Self::AsidLeaseRelease,
            _ => return None,
        })
    }
}
struct Entry {
    state: AtomicU8, serial: AtomicU64, start: AtomicU64, end: AtomicU64,
    tid: AtomicU64, root: AtomicU64, cell: AtomicU64, generation: AtomicU64,
    reason: AtomicU8, origin_reason: AtomicU8,
}
impl Entry {
    const fn new() -> Self {
        Self { state: AtomicU8::new(0), serial: AtomicU64::new(0),
            start: AtomicU64::new(0), end: AtomicU64::new(0),
            tid: AtomicU64::new(0), root: AtomicU64::new(0),
            cell: AtomicU64::new(0), generation: AtomicU64::new(0),
            reason: AtomicU8::new(0), origin_reason: AtomicU8::new(0) }
    }
    fn owner(&self) -> Owner {
        Owner { tid: self.tid.load(Ordering::SeqCst), root: self.root.load(Ordering::SeqCst),
            cell: self.cell.load(Ordering::SeqCst), generation: self.generation.load(Ordering::SeqCst) }
    }
}
static ENTRIES: [Entry; CAPACITY] = [const { Entry::new() }; CAPACITY];
pub(crate) fn fault() { SOURCE.fault(); }
pub(crate) fn release_write() -> crate::p01_resources::SourceWrite<'static> { SOURCE.write() }
pub(crate) fn static_bytes() -> usize {
    core::mem::size_of_val(&ENTRIES) + core::mem::size_of_val(&SOURCE) + core::mem::size_of_val(&NEXT)
        + core::mem::size_of_val(&PHYSICAL) + core::mem::size_of_val(&PHYSICAL_SOURCE)
        + core::mem::size_of_val(&PHYSICAL_NEXT)
}
/// `None` is only permitted for a source positively outside the selected inventory.
/// A source losing a selected identity must call `fault`, not reinterpret it as None.
pub(crate) fn begin(owner: Option<Owner>, base: usize, pages: usize, reason: Reason) -> Option<Token> {
    let owner = owner?;
    if trace::subject(owner).is_none() {
        if pages != 0 && crate::memory::heap::p01_owner::current_attempt().is_some() { fault(); }
        return None;
    }
    if pages == 0 { return None; }
    let _observation = match trace::scoped_observation() {
        Some(guard) => guard, None => { fault(); return None; }
    };
    let _write = SOURCE.write();
    let start = base as u64;
    let Some(end) = (pages as u64).checked_mul(PAGE).and_then(|bytes| start.checked_add(bytes)) else {
        fault(); return None;
    };
    if start % PAGE != 0 || end <= start { fault(); return None; }
    // Bounded reservation, no spinning even when interrupted by another writer.
    let Some((index, entry)) = ENTRIES.iter().enumerate().find(|(_, entry)|
        entry.state.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_ok()) else {
        fault(); return None;
    };
    let serial = NEXT.load(Ordering::SeqCst);
    let Some(next) = serial.checked_add(1) else {
        fault(); entry.state.store(0, Ordering::SeqCst); return None;
    };
    if serial == 0 || NEXT.compare_exchange(serial, next, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        fault(); entry.state.store(0, Ordering::SeqCst); return None;
    }
    entry.serial.store(serial, Ordering::SeqCst);
    entry.start.store(start, Ordering::SeqCst); entry.end.store(end, Ordering::SeqCst);
    entry.tid.store(owner.tid, Ordering::SeqCst); entry.root.store(owner.root, Ordering::SeqCst);
    entry.cell.store(owner.cell, Ordering::SeqCst); entry.generation.store(owner.generation, Ordering::SeqCst);
    entry.origin_reason.store(reason as u8, Ordering::SeqCst);
    entry.reason.store(reason as u8, Ordering::SeqCst); entry.state.store(2, Ordering::SeqCst);
    Some(Token { slot: index as u16, serial })
}
fn claim(token: Token) -> Option<&'static Entry> {
    let entry = ENTRIES.get(token.slot as usize)?;
    if entry.state.compare_exchange(2, 3, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return None;
    }
    // Check the serial only after exclusive mutation admission. A stale token
    // must never mark/retire a recycled slot between validation and mutation.
    if entry.serial.load(Ordering::SeqCst) != token.serial {
        entry.state.store(2, Ordering::SeqCst);
        return None;
    }
    Some(entry)
}
pub(crate) fn mark(token: Option<Token>, reason: Reason) {
    let Some(token) = token else { return; };
    let _observation = match trace::scoped_observation() {
        Some(guard) => guard, None => { fault(); return; }
    };
    let _write = SOURCE.write();
    match claim(token) {
        Some(entry) => {
            entry.reason.store(reason as u8, Ordering::SeqCst);
            entry.state.store(2, Ordering::SeqCst);
        }
        None => fault(),
    }
}
/// Called only after actual bitmap release. A failed allocator release stays live.
pub(crate) fn retire(token: Option<Token>) {
    let Some(token) = token else { return; };
    let _observation = match trace::scoped_observation() {
        Some(guard) => guard, None => { fault(); return; }
    };
    let _write = SOURCE.write();
    match claim(token) {
        Some(entry) => entry.state.store(0, Ordering::SeqCst),
        None => fault(),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Range {
    pub token: u64,
    pub owner: Owner,
    pub base: u64,
    pub end: u64,
    pub reason: u8,
    pub origin_reason: u8,
}
pub(crate) const fn capacity() -> usize { CAPACITY }
pub(crate) fn snapshot_before() -> (u64, bool) { SOURCE.before() }
pub(crate) fn snapshot_after(before: (u64, bool)) -> Metric { SOURCE.metric(0, before, 7) }
/// Copy-only payload. Enclose the ENTIRE enumeration in snapshot_before/after
/// and reject an overlapping writer or changed epoch.
pub(crate) fn copy_entry(index: usize) -> Option<Range> {
    let entry = ENTRIES.get(index)?;
    if entry.state.load(Ordering::SeqCst) != 2 { return None; }
    Some(Range { token: entry.serial.load(Ordering::SeqCst), owner: entry.owner(),
        base: entry.start.load(Ordering::SeqCst), end: entry.end.load(Ordering::SeqCst),
        reason: entry.reason.load(Ordering::SeqCst),
        origin_reason: entry.origin_reason.load(Ordering::SeqCst) })
}
/// Fixed-storage interval sweep, no heap or large stack scratch. Repeated scans
/// select the next start and transitively extend its union; overlapping owners
/// are unprovable, never resolved by choosing one. O(C^2), safe-point only.
pub(crate) fn metric() -> Metric {
    let before = SOURCE.before();
    let mut total = 0u64;
    let mut cursor = 0u64;
    let mut valid = true;
    for _ in 0..CAPACITY {
        let mut first: Option<(u64, u64, Owner)> = None;
        for entry in &ENTRIES {
            if entry.state.load(Ordering::SeqCst) != 2 { continue; }
            let start = entry.start.load(Ordering::SeqCst);
            let end = entry.end.load(Ordering::SeqCst);
            if start % PAGE != 0 || end <= start || end % PAGE != 0 { valid = false; continue; }
            if end <= cursor { continue; }
            if first.is_none_or(|current| start < current.0) { first = Some((start, end, entry.owner())); }
        }
        let Some((start, mut end, owner)) = first else { break; };
        for _ in 0..CAPACITY {
            let old_end = end;
            for entry in &ENTRIES {
                if entry.state.load(Ordering::SeqCst) != 2 { continue; }
                let other_start = entry.start.load(Ordering::SeqCst);
                let other_end = entry.end.load(Ordering::SeqCst);
                if other_start < end && other_end > start {
                    if entry.owner() != owner { valid = false; }
                    end = end.max(other_end);
                }
            }
            if end == old_end { break; }
        }
        match end.checked_sub(start).and_then(|bytes| total.checked_add(bytes)) {
            Some(value) => total = value, None => valid = false,
        }
        cursor = end;
    }
    let mut metric = SOURCE.metric(total, before, 7);
    if !valid && metric.status == 1 { fault(); metric.status = 4; }
    if metric.status != 1 { metric.value = 0; }
    metric
}

// A separate bitmap-source inventory. It is NOT the selected retained union:
// kind 0 explicitly leaves semantic ownership unidentified. Freed incarnations
// remain exportable, so background release is evidence rather than a leak label.
const PHYSICAL_CAPACITY: usize = 8192;
static PHYSICAL_SOURCE: Source = Source::new();
static PHYSICAL_NEXT: AtomicU64 = AtomicU64::new(1);
struct PhysicalEntry {
    state: AtomicU8,
    words: [AtomicU64; 12],
}
impl PhysicalEntry {
    const fn new() -> Self {
        Self { state: AtomicU8::new(0), words: [const { AtomicU64::new(0) }; 12] }
    }
}
static PHYSICAL: [PhysicalEntry; PHYSICAL_CAPACITY] =
    [const { PhysicalEntry::new() }; PHYSICAL_CAPACITY];

/// Hold across the actual bitmap transition, using ambient admission: ordinary
/// allocation/free outside the capture does not manufacture a late observation.
pub(crate) struct PhysicalWrite {
    _observation: trace::Observation,
    _write: crate::p01_resources::SourceWrite<'static>,
}
pub(crate) fn physical_write() -> Option<PhysicalWrite> {
    Some(PhysicalWrite { _observation: trace::observation()?,
        _write: PHYSICAL_SOURCE.write() })
}
/// Exact bitmap census at arm, under FRAME_ALLOCATOR and before observation
/// admission opens. Zero birth/attempt means unknown, NOT an allocation at arm.
/// Kind 6 carries the actual census source epoch in word 11.
pub(crate) fn physical_preexisting(frame: usize, census_epoch: u64) {
    let _write = PHYSICAL_SOURCE.write();
    physical_insert(frame, 0, 0, 6, census_epoch);
}
pub(crate) fn physical_fault() { PHYSICAL_SOURCE.fault(); }
pub(crate) fn physical_allocated(_write: &PhysicalWrite, frame: usize, epoch: u64) {
    if epoch == 0 { PHYSICAL_SOURCE.fault(); return; }
    physical_insert(frame, epoch,
        crate::memory::heap::p01_owner::current_attempt().unwrap_or(0), 0, 0);
}
fn physical_insert(frame: usize, epoch: u64, attempt: u64, kind: u64, census_epoch: u64) {
    if (epoch == 0 && (kind != 6 || census_epoch == 0))
        || !(frame as u64).is_multiple_of(PAGE) {
        PHYSICAL_SOURCE.fault(); return;
    }
    // History slots are never recycled. Once exhausted, keep rejecting evidence
    // without rescanning the full ledger for every remaining census bitmap bit.
    if PHYSICAL_NEXT.load(Ordering::SeqCst) > PHYSICAL_CAPACITY as u64 {
        PHYSICAL_SOURCE.fault(); return;
    }
    if PHYSICAL.iter().any(|entry| entry.state.load(Ordering::SeqCst) == 2
        && entry.words[1].load(Ordering::SeqCst) == frame as u64) {
        PHYSICAL_SOURCE.fault(); return;
    }
    let Some(entry) = PHYSICAL.iter().find(|entry|
        entry.state.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_ok())
        else { PHYSICAL_SOURCE.fault(); return; };
    let token = PHYSICAL_NEXT.load(Ordering::SeqCst);
    let Some(next) = token.checked_add(1) else {
        PHYSICAL_SOURCE.fault(); entry.state.store(0, Ordering::SeqCst); return;
    };
    if token == 0 || PHYSICAL_NEXT.compare_exchange(token, next,
        Ordering::SeqCst, Ordering::SeqCst).is_err() {
        PHYSICAL_SOURCE.fault(); entry.state.store(0, Ordering::SeqCst); return;
    }
    for (word, value) in entry.words.iter().zip([token, frame as u64, epoch, 0,
        attempt, kind, 0, 0, 0, 0, 0, census_epoch]) {
        word.store(value, Ordering::SeqCst);
    }
    entry.state.store(2, Ordering::SeqCst);
}
fn physical_claim(frame: usize) -> Option<&'static PhysicalEntry> {
    let entry = PHYSICAL.iter().find(|entry| entry.state.load(Ordering::SeqCst) == 2
        && entry.words[1].load(Ordering::SeqCst) == frame as u64)?;
    if entry.state.compare_exchange(2, 3, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        PHYSICAL_SOURCE.fault(); return None;
    }
    Some(entry)
}
pub(crate) fn physical_freed(_write: &PhysicalWrite, frame: usize, epoch: u64) {
    let Some(entry) = physical_claim(frame) else { PHYSICAL_SOURCE.fault(); return; };
    let birth = entry.words[2].load(Ordering::SeqCst);
    let earliest = if birth == 0 { entry.words[11].load(Ordering::SeqCst) } else { birth };
    if epoch <= earliest
        || (matches!(entry.words[5].load(Ordering::SeqCst), 1 | 5 | 7)
            && entry.words[8].load(Ordering::SeqCst) != 2) {
        // Keep the actual free witness, but never assert a valid detached-before-
        // recycle lifecycle when its source edge was absent or contradictory.
        PHYSICAL_SOURCE.fault();
    }
    entry.words[3].store(epoch, Ordering::SeqCst);
    entry.state.store(4, Ordering::SeqCst);
}
/// Allocation callback provenance, before the HAL publishes the table pointer.
pub(crate) fn physical_table_created(frame: usize, root: usize, va: usize) {
    let Some(_guard) = physical_write() else { return; };
    let Some(entry) = physical_claim(frame) else { PHYSICAL_SOURCE.fault(); return; };
    if entry.words[5].load(Ordering::SeqCst) != 0 {
        PHYSICAL_SOURCE.fault();
    } else {
        entry.words[5].store(1, Ordering::SeqCst);
        entry.words[6].store(root as u64, Ordering::SeqCst);
        entry.words[7].store(va as u64, Ordering::SeqCst);
        entry.words[8].store(1, Ordering::SeqCst);
    }
    entry.state.store(2, Ordering::SeqCst);
}
/// Successful paths share existing tables; touches are cumulative observations,
/// not a reference count or an assertion of exclusive Cell ownership.
pub(crate) fn physical_table_touched(frame: usize, root: usize, failed: bool) {
    let Some(_guard) = physical_write() else { return; };
    let Some(entry) = physical_claim(frame) else { PHYSICAL_SOURCE.fault(); return; };
    let kind = entry.words[5].load(Ordering::SeqCst);
    if matches!(kind, 0 | 6) {
        // Path evidence is not creation evidence. Kind 7 retains an unknown
        // pre-arm birth and word-11 census epoch; kind 5 has a known birth.
        entry.words[5].store(if kind == 6 { 7 } else { 5 }, Ordering::SeqCst);
        entry.words[6].store(root as u64, Ordering::SeqCst);
        entry.words[8].store(1, Ordering::SeqCst);
    } else if !matches!(entry.words[5].load(Ordering::SeqCst), 1 | 5 | 7)
        || entry.words[6].load(Ordering::SeqCst) != root as u64 {
        PHYSICAL_SOURCE.fault();
        entry.state.store(2, Ordering::SeqCst); return;
    }
    let index = if failed { 10 } else { 9 };
    let old = entry.words[index].load(Ordering::SeqCst);
    if let Some(next) = old.checked_add(1) { entry.words[index].store(next, Ordering::SeqCst); }
    else { PHYSICAL_SOURCE.fault(); }
    entry.state.store(2, Ordering::SeqCst);
}
pub(crate) fn physical_table_detached(frame: usize) {
    let Some(_guard) = physical_write() else { return; };
    let Some(entry) = physical_claim(frame) else { PHYSICAL_SOURCE.fault(); return; };
    if matches!(entry.words[5].load(Ordering::SeqCst), 1 | 5 | 7) {
        if entry.words[8].load(Ordering::SeqCst) != 1 { PHYSICAL_SOURCE.fault(); }
        entry.words[8].store(2, Ordering::SeqCst);
    }
    entry.state.store(2, Ordering::SeqCst);
}
/// Exact post-load allocation failure observed before the production RAII unwind.
/// edge 2=final_flags, 3=mapped_pages, 4=writable_pages. Birth and allocation
/// attempt remain immutable; the actual subsequent free is recorded separately.
pub(crate) fn physical_prepare_failed(frame: usize, va: usize, load_base: usize, edge: u64) {
    let selected_preparation = crate::memory::heap::p01_owner::current_attempt().is_some();
    let observation = if selected_preparation { trace::scoped_observation() }
        else { trace::observation() };
    let Some(_observation) = observation else {
        if selected_preparation { PHYSICAL_SOURCE.fault(); }
        return;
    };
    let _write = PHYSICAL_SOURCE.write();
    let Some(entry) = physical_claim(frame) else { PHYSICAL_SOURCE.fault(); return; };
    if !(2..=4).contains(&edge) || entry.words[5].load(Ordering::SeqCst) != 0 {
        PHYSICAL_SOURCE.fault();
    } else {
        entry.words[5].store(edge, Ordering::SeqCst);
        entry.words[7].store(va as u64, Ordering::SeqCst);
        entry.words[11].store(load_base as u64, Ordering::SeqCst);
    }
    entry.state.store(2, Ordering::SeqCst);
}
pub(crate) const fn physical_capacity() -> usize { PHYSICAL_CAPACITY }
pub(crate) fn physical_snapshot_before() -> (u64, bool) { PHYSICAL_SOURCE.before() }
pub(crate) fn physical_snapshot_after(before: (u64, bool)) -> Metric {
    PHYSICAL_SOURCE.metric(0, before, 9)
}
/// [incarnation, frame, alloc_epoch, free_epoch, allocation_attempt, kind,
///  shared_root, creation_or_failure_va, table_state, path_touches,
///  failed_path_touches, failure_load_base_or_preexisting_census_epoch].
/// Kinds 6/7 have unknown birth (alloc_epoch=0); word 11 is their exact arm census
/// epoch, not creation provenance. Enclose ALL copies in one source epoch read.
pub(crate) fn copy_physical_entry(index: usize) -> Option<[u64; 12]> {
    let entry = PHYSICAL.get(index)?;
    if !matches!(entry.state.load(Ordering::SeqCst), 2 | 4) { return None; }
    Some(core::array::from_fn(|index| entry.words[index].load(Ordering::SeqCst)))
}
/// Join a bitmap-owned table to exactly one historical incarnation at the
/// allocator cutoff. Census tokens are real identities even with unknown birth;
/// the census epoch bounds their visibility without inventing allocation time.
pub(crate) fn physical_incarnation_at(frame: u64, cutoff: u64) -> Option<u64> {
    let mut found = None;
    for index in 0..PHYSICAL_CAPACITY {
        let Some(words) = copy_physical_entry(index) else { continue; };
        if words[1] != frame { continue; }
        let start = if words[2] == 0 {
            if !matches!(words[5], 6 | 7) { return None; }
            words[11]
        } else { words[2] };
        if start == 0 || start > cutoff || (words[3] != 0 && words[3] <= cutoff) { continue; }
        if words[0] == 0 || found.replace(words[0]).is_some() { return None; }
    }
    found
}
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn reset_for_test(next: u64) {
    for entry in &ENTRIES { entry.state.store(0, Ordering::SeqCst); }
    for entry in &PHYSICAL { entry.state.store(0, Ordering::SeqCst); }
    PHYSICAL_NEXT.store(next, Ordering::SeqCst);
    NEXT.store(next, Ordering::SeqCst);
}
