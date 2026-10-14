//! Disposable allocation-origin and exclusive-object-owner evidence, not quota policy.
//! All Ledger access is serialized by the existing ALLOCATOR.inner heap guard.
//! No allocator hook allocates, consults SCHEDULER, or acquires another lock.
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::p01_measurement::{self as trace, Attempt, Owner};

// Observation ceiling, not a fit certificate. The resource-attribution contract
// bounds exclusive preparation objects separately from shared/runtime storage.
pub(crate) const CAPACITY: usize = 1024;
const ATTEMPTS: usize = 10;
// Selected scope: at most eight child attempts plus two driver route attempts.
// Bindings are historical and never recycled merely because an attempt closes.
const HARTS: usize = crate::task::smp::MAX_HARTS;
const BOOKKEEPING: u64 = u64::MAX;
static CONTEXT: [AtomicU64; HARTS] = [const { AtomicU64::new(0) }; HARTS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    KernelBookkeeping,
    OutsideSelected,
    // Execution-origin evidence is not proof of semantic object ownership.
    Execution(Owner),
    Preparation(u64),
    Unknown,
}
#[derive(Clone, Copy)]
pub(crate) struct Provenance { origin: Origin, adopted: u64, semantic: Semantic }
#[derive(Clone, Copy)]
struct Entry { ptr: usize, size: usize, align: usize, origin: Origin, adopted: u64, semantic: Semantic, incarnation: u64, charged: usize, birth_epoch: u64, prearm: bool }
impl Entry { const EMPTY: Self = Self { ptr: 0, size: 0, align: 0, origin: Origin::Unknown, adopted: 0, semantic: Semantic::EMPTY, incarnation: 0, charged: 0, birth_epoch: 0, prearm: false }; }
#[derive(Clone, Copy, PartialEq, Eq)]
struct Semantic { kind: u64, owner: Option<Owner>, from: Option<Owner> }
impl Semantic { const EMPTY: Self = Self { kind: 0, owner: None, from: None }; }
pub(crate) const OPERATIONS: u64 = 1;
pub(crate) const REPLY: u64 = 2;
pub(crate) const MAILBOX: u64 = 3;
pub(crate) const QUEUED_WIRE: u64 = 4;
pub(crate) const SENDER_WIRE: u64 = 5;
pub(crate) const QUOTA_SHARED: u64 = 6;
pub(crate) const LOG_SHARED: u64 = 7;
pub(crate) const LOG_PATH: u64 = 8;
pub(crate) const DEATH_SHARED: u64 = 9;
pub(crate) const COMPLETION_SHARED: u64 = 10;
pub(crate) const RECEIVE_SCRATCH: u64 = 11;
pub(crate) const SYNTHETIC_WIRE: u64 = 12;
pub(crate) const LEGACY_PAYLOAD: u64 = 13;
pub(crate) const MIGRATION_COPY: u64 = 14;
pub(crate) const DEATH_QUEUE: u64 = 15;
pub(crate) const TASK_BACKING: u64 = 64;
pub(crate) const TASK_SEGMENTS: u64 = 65;
pub(crate) const SCHEDULER_SHARED: u64 = 32;
pub(crate) const SIGNING_SHARED: u64 = 33;
pub(crate) const FILESYSTEM_SHARED: u64 = 34;
pub(crate) const DRIVER_SHARED: u64 = 35;
pub(crate) const SERVICE_SHARED: u64 = 36;
pub(crate) const BOOT_METADATA_SHARED: u64 = 37;
pub(crate) const SYSTEM_CONFIG_SHARED: u64 = 38;
pub(crate) const FILESYSTEM_SCRATCH_SHARED: u64 = 39;
pub(crate) const FUTEX_SHARED: u64 = 40;
pub(crate) const GRANT_SHARED: u64 = 41;
pub(crate) const DOMAIN_SHARED: u64 = 42;
pub(crate) const TEST_FIXTURE_SHARED: u64 = 43;
pub(crate) const PIPE_SHARED: u64 = 47;
pub(crate) const DOMAIN_BACKING: u64 = 66;
pub(crate) const TASK_FIELDS: u64 = 67;
pub(crate) const TASK_IO: u64 = 68;
fn shared_kind(kind: u64) -> bool {
    matches!(kind, QUOTA_SHARED | LOG_SHARED | LOG_PATH | DEATH_SHARED | COMPLETION_SHARED | SYNTHETIC_WIRE | SCHEDULER_SHARED | SIGNING_SHARED | FILESYSTEM_SHARED | DRIVER_SHARED | SERVICE_SHARED | BOOT_METADATA_SHARED | SYSTEM_CONFIG_SHARED | FILESYSTEM_SCRATCH_SHARED | FUTEX_SHARED | GRANT_SHARED | DOMAIN_SHARED | TEST_FIXTURE_SHARED | PIPE_SHARED)
}
fn source_kind(kind: u64) -> bool {
    (1..=DEATH_QUEUE).contains(&kind) && kind != QUEUED_WIRE
        || shared_kind(kind) || matches!(kind, TASK_BACKING | TASK_SEGMENTS | DOMAIN_BACKING | TASK_FIELDS | TASK_IO)
}
#[derive(Clone, Copy)]
struct Binding { attempt: u64, owner: Option<Owner>, closed: bool }
impl Binding { const EMPTY: Self = Self { attempt: 0, owner: None, closed: false }; }
// Birth is witnessed at successful physical allocation, never at a later join.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Boot, Armed, Frozen }
struct Baseline { entries: [Entry; CAPACITY], source: Option<[u64; 12]>, charged_source: Option<[u64; 22]> }
// Samples 2..6 retain the same Entry (including physical birth and charge) as
// the live census. Empty sources distinguish unused stages from empty censuses.
struct StageSnapshot { entries: [Entry; CAPACITY], source: Option<[u64; 22]> }
impl StageSnapshot { const EMPTY: Self = Self { entries: [Entry::EMPTY; CAPACITY], source: None }; }
struct Ledger {
    entries: [Entry; CAPACITY], baseline: Baseline, stages: [StageSnapshot; 5], bindings: [Binding; ATTEMPTS],
    contexts: [Semantic; HARTS], epoch: u64, invalid: bool, phase: Phase,
    exhausted: bool, prearm_allocations: u64, live: usize, highwater: usize,
    frozen_accounting: [u64; 4], frozen_source: Option<[u64; 22]>,
    first_fault: Option<[u64; 6]>,
}
struct Storage(UnsafeCell<Ledger>);
// SAFETY: every access to the UnsafeCell occurs under ALLOCATOR.inner.lock(),
// including scope setup/clear, publication adoption and snapshot export.
unsafe impl Sync for Storage {}
static STORAGE: Storage = Storage(UnsafeCell::new(Ledger {
    entries: [Entry::EMPTY; CAPACITY], bindings: [Binding::EMPTY; ATTEMPTS],
    baseline: Baseline { entries: [Entry::EMPTY; CAPACITY], source: None, charged_source: None },
    stages: [const { StageSnapshot::EMPTY }; 5],
    contexts: [Semantic::EMPTY; HARTS], epoch: 1, invalid: false, phase: Phase::Boot,
    exhausted: false, prearm_allocations: 0, live: 0, highwater: 0,
    frozen_accounting: [0; 4], frozen_source: None,
    first_fault: None,
}));
// One immutable numeric faultsite, copied only outside the heap lock. This
// diagnostic is not an owner witness and never changes fail-closed validity.
#[track_caller]
fn fault(ledger: &mut Ledger, bits: u64) {
    if ledger.first_fault.is_none() {
        ledger.first_fault = Some([1, bits, core::panic::Location::caller().line() as u64,
            ledger.epoch, match ledger.phase { Phase::Boot => 0, Phase::Armed => 1, Phase::Frozen => 2 },
            crate::task::hart_local::current_hart_id() as u64]);
    }
    ledger.invalid = true;
    trace::invalidate(bits);
}
fn bump(ledger: &mut Ledger) {
    match ledger.epoch.checked_add(1) {
        Some(next) => ledger.epoch = next,
        None => { ledger.exhausted = true; fault(ledger, trace::COUNTER); }
    }
}
// SAFETY: private callers hold the existing heap lock throughout this access.
unsafe fn ledger() -> &'static mut Ledger { &mut *STORAGE.0.get() }

// Census admission is deliberately independent of the ordinary trace gate.
// Boot -> armed never closes tracking; STOP drains ordinary writers, then the
// collector takes the heap lock once to freeze this source and its accounting.
pub(super) fn tracking() -> bool { unsafe { ledger() }.phase != Phase::Frozen }
pub(crate) fn arm() {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if ledger.phase != Phase::Boot { fault(ledger, trace::PROTOCOL); return; }
    ledger.phase = Phase::Armed;
    bump(ledger);
}
pub(crate) fn freeze(_proof: &trace::Frozen) {
    let heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if ledger.phase == Phase::Frozen { return; }
    ledger.phase = Phase::Frozen;
    bump(ledger);
    ledger.frozen_accounting = [ledger.epoch, super::P01_EPOCH.load(Ordering::Relaxed) as u64, heap.used() as u64, heap.free() as u64];
    ledger.frozen_source = Some(charged_source(ledger, heap.used(), heap.free(), false));
}
fn mutable(ledger: &Ledger) -> bool { ledger.phase != Phase::Frozen && !ledger.exhausted }
// Private v1 charged source: sample/phase, coherent source epochs/accounting,
// measured partitions and observation ceilings. Neither coverage nor fit proof.
fn charged_source(ledger: &mut Ledger, used: usize, free: usize, baseline: bool) -> [u64; 22] {
    charged_source_for_sample(ledger, used, free, baseline as u64).0
}
fn charged_source_for_sample(ledger: &mut Ledger, used: usize, free: usize, sample: u64) -> ([u64; 22], bool) {
    let mut count = 0u64;
    let mut requested = 0u64;
    let mut charged = 0u64;
    let mut partitions = [0u64; 3]; // unowned, shared, exclusive
    let mut prearm_live = 0u64;
    let mut overflow = false;
    for entry in ledger.entries.iter().filter(|e| e.ptr != 0) {
        for (sum, value) in [(&mut count, 1), (&mut prearm_live, entry.prearm as u64)] {
            if let Some(next) = sum.checked_add(value) { *sum = next; } else { overflow = true; }
        }
        let partition = if entry.semantic.kind == 0 { 0 } else if entry.semantic.owner.is_none() { 1 } else { 2 };
        for (sum, value) in [(&mut requested, entry.size as u64), (&mut charged, entry.charged as u64), (&mut partitions[partition], entry.charged as u64)] {
            if let Some(next) = sum.checked_add(value) { *sum = next; } else { overflow = true; }
        }
    }
    if overflow || charged > used as u64 { fault(ledger, trace::COUNTER); }
    let count_matches = count == ledger.live as u64;
    if !count_matches { fault(ledger, trace::INCOMPLETE); }
    let unmatched = (used as u64).checked_sub(charged).unwrap_or(u64::MAX);
    if unmatched != 0 { fault(ledger, trace::INCOMPLETE); }
    ([1, sample, match ledger.phase { Phase::Boot => 0, Phase::Armed => 1, Phase::Frozen => 2 },
        ledger.epoch, super::P01_EPOCH.load(Ordering::Relaxed) as u64, used as u64, free as u64,
        count, requested, charged, unmatched, partitions[0], partitions[1], partitions[2],
        ledger.prearm_allocations, prearm_live, ledger.highwater as u64, ledger.exhausted as u64,
        CAPACITY as u64, static_bytes() as u64, ledger.invalid as u64, 2],
        !overflow && count_matches && charged == used as u64 && !ledger.exhausted)
}
pub(crate) fn copy_charged_source(baseline: bool) -> Option<[u64; 22]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if baseline { ledger.baseline.charged_source } else { ledger.frozen_source }
}
pub(crate) fn copy_charged_entry(index: usize, baseline: bool) -> Option<[u64; 5]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    let entry = if baseline { ledger.baseline.entries.get(index)? } else { ledger.entries.get(index)? };
    (entry.ptr != 0).then_some([entry.ptr as u64, entry.incarnation, entry.charged as u64, entry.birth_epoch, entry.prearm as u64])
}

pub(crate) fn copy_first_fault() -> Option<[u64; 6]> {
    let _heap = super::ALLOCATOR.inner.lock();
    unsafe { ledger() }.first_fault
}

pub(crate) fn current_attempt() -> Option<u64> {
    let id = CONTEXT.get(crate::task::hart_local::current_hart_id())?.load(Ordering::Acquire);
    (id != 0 && id != BOOKKEEPING).then_some(id)
}
fn current_origin() -> Origin {
    if CONTEXT.get(crate::task::hart_local::current_hart_id())
        .is_some_and(|context| context.load(Ordering::Acquire) == BOOKKEEPING) {
        return Origin::KernelBookkeeping;
    }
    if let Some(id) = current_attempt() { return Origin::Preparation(id); }
    let cell = crate::task::hart_local::current_cell_id() as u64;
    let generation = crate::task::hart_local::current_cell_generation();
    if cell == 0 { Origin::KernelBookkeeping }
    else { trace::selected_owner(cell, generation).map_or(Origin::OutsideSelected, Origin::Execution) }
}

/// Logical context only, never an observer held across preparation/publication.
/// It must remain on its originating hart; migration/nesting invalidates evidence.
pub(crate) struct Scope { hart: usize, id: u64, _not_send: PhantomData<*mut ()> }
pub(crate) fn scope(attempt: Option<Attempt>) -> Scope {
    let hart = crate::task::hart_local::current_hart_id();
    let mut scope = Scope { hart, id: 0, _not_send: PhantomData };
    let Some(attempt) = attempt.filter(|a| a.selected()) else { return scope; };
    let Some(_observation) = trace::scoped_observation() else { return scope; };
    if attempt.id == 0 || attempt.id == BOOKKEEPING {
        trace::invalidate(trace::IDENTITY); return scope;
    }
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return scope; }
    let Some(context) = CONTEXT.get(hart) else { fault(ledger, trace::IDENTITY); return scope; };
    if context.load(Ordering::Acquire) != 0 || ledger.bindings.iter().any(|b| b.attempt == attempt.id) {
        fault(ledger, trace::PROTOCOL); return scope;
    }
    let Some(binding) = ledger.bindings.iter_mut().find(|b| b.attempt == 0) else {
        fault(ledger, trace::FULL); return scope;
    };
    *binding = Binding { attempt: attempt.id, owner: None, closed: false };
    context.store(attempt.id, Ordering::Release);
    scope.id = attempt.id;
    bump(ledger);
    scope
}
impl Drop for Scope {
    fn drop(&mut self) {
        if self.id == 0 { return; }
        let Some(_observation) = trace::scoped_observation() else { return; };
        let _heap = super::ALLOCATOR.inner.lock();
        let ledger = unsafe { ledger() };
        if !mutable(ledger) { return; }
        if crate::task::hart_local::current_hart_id() != self.hart {
            fault(ledger, trace::IDENTITY); return;
        }
        if CONTEXT[self.hart].compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire).is_err() {
            fault(ledger, trace::PROTOCOL); return;
        }
        if let Some(binding) = ledger.bindings.iter_mut().find(|b| b.attempt == self.id) {
            binding.closed = true;
        } else { fault(ledger, trace::IDENTITY); }
        bump(ledger);
    }
}

pub(crate) struct Bookkeeping { hart: usize, id: u64, active: bool, _not_send: PhantomData<*mut ()> }
/// Suspend child-attempt attribution around the actual scheduler/container
/// publication bookkeeping. No runtime accounting identity is changed.
pub(crate) fn bookkeeping() -> Bookkeeping {
    let hart = crate::task::hart_local::current_hart_id();
    let mut guard = Bookkeeping { hart, id: 0, active: false, _not_send: PhantomData };
    let Some(_observation) = trace::observation() else { return guard; };
    let _heap = super::ALLOCATOR.inner.lock();
    if !mutable(unsafe { ledger() }) { return guard; }
    if let Some(context) = CONTEXT.get(hart) {
        guard.id = context.swap(BOOKKEEPING, Ordering::AcqRel);
        guard.active = true;
    } else { fault(unsafe { ledger() }, trace::IDENTITY); }
    guard
}
impl Drop for Bookkeeping {
    fn drop(&mut self) {
        if !self.active { return; }
        let Some(_observation) = trace::scoped_observation() else { return; };
        let _heap = super::ALLOCATOR.inner.lock();
        let ledger = unsafe { ledger() };
        if !mutable(ledger) { return; }
        if crate::task::hart_local::current_hart_id() != self.hart
            || CONTEXT[self.hart].compare_exchange(BOOKKEEPING, self.id, Ordering::AcqRel, Ordering::Acquire).is_err()
        { fault(ledger, trace::IDENTITY); }
    }
}

/// Bind an immutable attempt to the exact committed incarnation. Entries retain
/// their original token; no rewrite/owner guess occurs at publication or free.
pub(crate) fn bind(attempt: Option<Attempt>, owner: Owner) {
    let Some(attempt) = attempt.filter(|a| a.selected()) else { return; };
    let Some(_observation) = trace::scoped_observation() else { return; };
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return; }
    let Some(binding) = ledger.bindings.iter_mut().find(|b| b.attempt == attempt.id) else {
        fault(ledger, trace::IDENTITY); return;
    };
    if binding.owner.is_some() || trace::subject(owner).is_none() || owner.tid != owner.root {
        fault(ledger, trace::IDENTITY); return;
    }
    binding.owner = Some(owner);
    // Full source witnesses can precede the selected trace publication. Join
    // them now to the committed binding without rewriting allocation origins.
    for entry in ledger.entries.iter_mut().filter(|e| e.ptr != 0 && e.semantic.owner == Some(owner)) {
        entry.adopted = attempt.id;
    }
    bump(ledger);
}
pub(crate) fn resolve_attempt(id: u64) -> Option<Owner> {
    let _heap = super::ALLOCATOR.inner.lock();
    unsafe { ledger() }.bindings.iter().find(|b| b.attempt == id)?.owner
}
/// Attach an exclusively owned Vec buffer; preserve its allocation origin.
pub(crate) fn adopt_vec<T>(buffer: &alloc::vec::Vec<T>) {
    if buffer.capacity() == 0 || core::mem::size_of::<T>() == 0 { return; }
    let Ok(layout) = Layout::array::<T>(buffer.capacity()) else {
        trace::invalidate(trace::COUNTER); return;
    };
    adopt_buffer(buffer.as_ptr() as usize, layout);
}
pub(crate) fn adopt_task(task: &alloc::boxed::Box<crate::task::Task>) {
    adopt_buffer(&**task as *const crate::task::Task as usize, Layout::new::<crate::task::Task>());
    for text in [&task.name, &task.cwd] {
        if text.capacity() != 0 {
            // String uses a Vec<u8> backing allocation with capacity Layout.
            adopt_buffer(text.as_ptr() as usize, Layout::array::<u8>(text.capacity()).unwrap());
        }
    }
    adopt_vec(&task.allowed_drivers);
    if let Some(argv) = task.inherited_argv.as_ref() { adopt_vec(argv); }
    // Other task containers start empty in Task::new; inherited dirs, capabilities
    // and stack descriptors are inline. Runtime mailbox/operation/completion
    // storage is not an exclusive preparation buffer and must not be adopted here.
}
/// Join actual backing allocations just before scheduler publication. The
/// object's full identity is a semantic witness, never a replacement birth.
pub(crate) fn task_backing(task: &alloc::boxed::Box<crate::task::Task>) {
    let owner = Owner::of(task);
    runtime_buffer(&**task as *const crate::task::Task as usize, Layout::new::<crate::task::Task>(), owner, TASK_BACKING);
    for text in [&task.name, &task.cwd] {
        if text.capacity() != 0 {
            runtime_buffer(text.as_ptr() as usize, Layout::array::<u8>(text.capacity()).unwrap(), owner, TASK_BACKING);
        }
    }
    runtime_vec(&task.allowed_drivers, owner, TASK_BACKING);
    if let Some(argv) = task.inherited_argv.as_ref() { runtime_vec(argv, owner, TASK_BACKING); }
    if let Some(argv) = task.staged_argv.as_ref() { runtime_vec(argv, owner, TASK_FIELDS); }
    runtime_vec(&task.waiters, owner, TASK_FIELDS);
    runtime_vec(&task.pending_owner_deaths, owner, DEATH_QUEUE);
    runtime_vec(&task.pending_deaths, owner, DEATH_QUEUE);
    if let Some(segments) = task.segment_mem.as_ref() { segments.p01_backing(owner); }
}
fn adopt_buffer(ptr: usize, layout: Layout) {
    let Some(attempt) = current_attempt() else { return; };
    let Some(_observation) = trace::scoped_observation() else { return; };
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return; }
    if !ledger.bindings.iter().any(|binding| binding.attempt == attempt && !binding.closed) {
        fault(ledger, trace::IDENTITY); return;
    }
    if let Some(entry) = ledger.entries.iter_mut().find(|entry| entry.ptr == ptr) {
        if entry.size != layout.size() || entry.align != layout.align()
            || entry.semantic.kind != 0
            || (entry.adopted != 0 && entry.adopted != attempt) { fault(ledger, trace::IDENTITY); return; }
        if entry.adopted == attempt { return; }
        entry.adopted = attempt;
    } else {
        fault(ledger, trace::INCOMPLETE); return;
    }
    bump(ledger);
}
/// Source-owned allocation region. Labels actual allocator Layouts (including
/// opaque map nodes and Arc headers), not guessed container representation.
/// Nested regions restore their previous witness; no production identity changes.
pub(crate) struct Creation {
    hart: usize, previous: Semantic, witness: Semantic, active: bool, _not_send: PhantomData<*mut ()>,
}
pub(crate) fn creation(owner: Option<Owner>, kind: u64) -> Creation {
    creation_inner(owner, kind, false)
}
/// Preserve an enclosing task/file owner; classify boot-open handles otherwise.
pub(crate) fn creation_fallback(kind: u64) -> Creation {
    creation_inner(None, kind, true)
}
fn creation_inner(owner: Option<Owner>, kind: u64, fallback: bool) -> Creation {
    let hart = crate::task::hart_local::current_hart_id();
    let mut guard = Creation { hart, previous: Semantic::EMPTY, witness: Semantic::EMPTY, active: false, _not_send: PhantomData };
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return guard; }
    let shared = shared_kind(kind);
    if !source_kind(kind) || shared != owner.is_none() || hart >= HARTS {
        fault(ledger, trace::IDENTITY); return guard;
    }
    if fallback && ledger.contexts[hart] != Semantic::EMPTY { return guard; }
    guard.previous = ledger.contexts[hart];
    guard.witness = Semantic { kind, owner, from: None };
    ledger.contexts[hart] = guard.witness;
    guard.active = true;
    guard
}
impl Drop for Creation {
    fn drop(&mut self) {
        if !self.active { return; }
        let _heap = super::ALLOCATOR.inner.lock();
        let ledger = unsafe { ledger() };
        if !mutable(ledger) { return; }
        if crate::task::hart_local::current_hart_id() != self.hart {
            fault(ledger, trace::IDENTITY); return;
        }
        if ledger.contexts[self.hart] != self.witness {
            fault(ledger, trace::PROTOCOL); return;
        }
        ledger.contexts[self.hart] = self.previous;
    }
}
pub(crate) fn shared_buffer(ptr: usize, layout: Layout, kind: u64) {
    witnessed_buffer(ptr, layout, None, kind);
}
/// Join retained source-owned storage using its actual public buffer capacity.
/// This is a semantic publication witness, not an allocation or birth record.
pub(crate) fn shared_vec<T>(buffer: &alloc::vec::Vec<T>, kind: u64) {
    if buffer.capacity() == 0 || core::mem::size_of::<T>() == 0 { return; }
    let Ok(layout) = Layout::array::<T>(buffer.capacity()) else {
        trace::invalidate(trace::COUNTER); return;
    };
    shared_buffer(buffer.as_ptr() as usize, layout, kind);
}
pub(crate) fn shared_string(buffer: &alloc::string::String, kind: u64) {
    if buffer.capacity() == 0 { return; }
    let Ok(layout) = Layout::array::<u8>(buffer.capacity()) else {
        trace::invalidate(trace::COUNTER); return;
    };
    shared_buffer(buffer.as_ptr() as usize, layout, kind);
}


/// Exact object-owner witness copied under SCHEDULER, never execution attribution.
/// Lock order is SCHEDULER -> heap; allocator hooks never acquire SCHEDULER.
pub(crate) fn runtime_vec<T>(buffer: &alloc::vec::Vec<T>, owner: Owner, kind: u64) {
    if buffer.capacity() == 0 || core::mem::size_of::<T>() == 0 { return; }
    let Ok(layout) = Layout::array::<T>(buffer.capacity()) else {
        trace::invalidate(trace::COUNTER); return;
    };
    runtime_buffer(buffer.as_ptr() as usize, layout, owner, kind);
}
pub(crate) fn runtime_buffer(ptr: usize, layout: Layout, owner: Owner, kind: u64) {
    witnessed_buffer(ptr, layout, Some(owner), kind);
}
fn witnessed_buffer(ptr: usize, layout: Layout, owner: Option<Owner>, kind: u64) {
    if layout.size() == 0 { return; }
    // The census has its own heap-lock lifecycle, including pre-arm witnesses.
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return; }
    let shared = shared_kind(kind);
    if !source_kind(kind) || shared != owner.is_none() {
        fault(ledger, trace::IDENTITY); return;
    }
    let adopted = owner.and_then(|owner| ledger.bindings.iter().find(|b| b.owner == Some(owner))).map_or(0, |b| b.attempt);
    let existing = ledger.entries.iter().position(|e| e.ptr == ptr);
    if let Some(index) = existing {
        let entry = &ledger.entries[index];
        if entry.size != layout.size() || entry.align != layout.align() {
            fault(ledger, trace::IDENTITY); return;
        }
        if entry.semantic.kind != 0
            && (entry.semantic.kind != kind || entry.semantic.owner != owner) {
            fault(ledger, trace::IDENTITY); return;
        }
        ledger.entries[index].adopted = adopted;
        ledger.entries[index].semantic = Semantic { kind, owner, from: None };
    } else {
        fault(ledger, trace::INCOMPLETE); return;
    }
    bump(ledger);
}

/// Validate sender scalar header against the immutable full creation witness.
/// The payload is exclusively borrowed by the scheduler-owned queue at commit.
pub(crate) fn transfer_wire(ptr: usize, layout: Layout, receiver: Owner, tid: u64, cell: u64, generation: u64) {
    if layout.size() == 0 { return; }
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if !mutable(ledger) { return; }
    let Some(index) = ledger.entries.iter().position(|e| e.ptr == ptr) else {
        if trace::subject(receiver).is_some() || trace::selected_owner(cell, generation).is_some() {
            fault(ledger, trace::INCOMPLETE);
        }
        return;
    };
    let entry = &ledger.entries[index];
    let synthetic = entry.semantic.kind == SYNTHETIC_WIRE && entry.semantic.owner.is_none();
    let sender = entry.semantic.owner;
    if sender.is_none() && !synthetic {
        if trace::subject(receiver).is_some() || trace::selected_owner(cell, generation).is_some() {
            fault(ledger, trace::INCOMPLETE);
        }
        return;
    }
    if entry.size != layout.size() || entry.align != layout.align()
        || (!synthetic && entry.semantic.kind != SENDER_WIRE)
        || sender.is_some_and(|sender| (sender.tid, sender.cell, sender.generation) != (tid, cell, generation)) {
        fault(ledger, trace::IDENTITY); return;
    }
    let adopted = ledger.bindings.iter().find(|b| b.owner == Some(receiver)).map_or(0, |b| b.attempt);
    ledger.entries[index].adopted = adopted;
    ledger.entries[index].semantic = Semantic {
        kind: if synthetic { LEGACY_PAYLOAD } else { QUEUED_WIRE }, owner: Some(receiver), from: sender,
    };
    bump(ledger);
}

/// Called only under the existing heap lock after successful physical allocation.
pub(super) unsafe fn allocated(ptr: *mut u8, layout: Layout, provenance: Option<Provenance>, charged: usize, birth_epoch: u64) {
    let ledger = ledger();
    if !mutable(ledger) { return; }
    if ledger.phase == Phase::Boot {
        let Some(next) = ledger.prearm_allocations.checked_add(1) else { fault(ledger, trace::COUNTER); return; };
        ledger.prearm_allocations = next;
    }
    if ledger.entries.iter().any(|e| e.ptr == ptr as usize) { fault(ledger, trace::IDENTITY); return; }
    let provenance = provenance.unwrap_or_else(|| {
        let semantic = ledger.contexts.get(crate::task::hart_local::current_hart_id()).copied().unwrap_or(Semantic::EMPTY);
        let adopted = semantic.owner.and_then(|owner| ledger.bindings.iter().find(|b| b.owner == Some(owner))).map_or(0, |b| b.attempt);
        Provenance { origin: current_origin(), adopted, semantic }
    });
    let Some(entry) = ledger.entries.iter_mut().find(|e| e.ptr == 0) else {
        ledger.exhausted = true;
        fault(ledger, trace::FULL); return;
    };
    *entry = Entry { ptr: ptr as usize, size: layout.size(), align: layout.align(),
        origin: provenance.origin, adopted: provenance.adopted, semantic: provenance.semantic,
        incarnation: ledger.epoch.saturating_add(1), charged, birth_epoch, prearm: ledger.phase == Phase::Boot };
    ledger.live += 1;
    ledger.highwater = ledger.highwater.max(ledger.live);
    bump(ledger);
}
/// Copy immutable allocation origin, not the reallocating quota identity.
pub(super) unsafe fn realloc_origin(ptr: *mut u8, layout: Layout) -> Provenance {
    let ledger = ledger();
    if mutable(ledger) {
        if let Some(entry) = ledger.entries.iter().find(|e| e.ptr == ptr as usize) {
            if entry.size == layout.size() && entry.align == layout.align() {
                return Provenance { origin: entry.origin, adopted: entry.adopted, semantic: entry.semantic };
            }
            fault(ledger, trace::IDENTITY);
        } else { fault(ledger, trace::INCOMPLETE); }
    }
    Provenance { origin: Origin::Unknown, adopted: 0, semantic: Semantic::EMPTY }
}
/// Actual free under the same heap lock; no guessed pre-arm entry is inserted.
pub(super) unsafe fn freed(ptr: *mut u8, layout: Layout, charged: usize) {
    let ledger = ledger();
    if !mutable(ledger) { return; }
    if let Some(entry) = ledger.entries.iter_mut().find(|e| e.ptr == ptr as usize) {
        let mismatch = entry.size != layout.size() || entry.align != layout.align() || entry.charged != charged;
        *entry = Entry::EMPTY;
        ledger.live -= 1;
        if mismatch { fault(ledger, trace::IDENTITY); }
        bump(ledger);
    } else { fault(ledger, trace::INCOMPLETE); }
}

#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub epoch: u64,
    pub invalid: bool,
    pub fit_proven: bool,
    pub coverage_proven: bool,
    pub live_objects: u64,
    pub requested_bytes: u64,
    pub attempt_objects: u64,
    pub kernel_objects: u64,
    pub outside_selected_objects: u64,
    pub unknown_objects: u64,
    pub unresolved_attempts: u64,
}
/// Coherent heap-lock copy of requested Layout bytes, not physical extents.
/// Owner filters prefer explicit semantic joins; unjoined entries remain an
/// origin cohort, never a complete per-generation semantic heap total.
pub(crate) fn snapshot(owner: Option<Owner>) -> Snapshot {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    let mut out = Snapshot { epoch: ledger.epoch, invalid: ledger.invalid,
        fit_proven: false, coverage_proven: false, live_objects: 0, requested_bytes: 0,
        attempt_objects: 0, kernel_objects: 0, outside_selected_objects: 0,
        unknown_objects: 0, unresolved_attempts: 0 };
    for binding in ledger.bindings.iter().filter(|b| b.attempt != 0) {
        let live = ledger.entries.iter().any(|e| e.ptr != 0
            && (e.origin == Origin::Preparation(binding.attempt) || e.adopted == binding.attempt));
        if !binding.closed || (binding.owner.is_none() && live) { out.unresolved_attempts += 1; }
    }
    let mut overflow = false;
    for entry in ledger.entries.iter().filter(|e| e.ptr != 0) {
        let bound = if entry.semantic.kind != 0 { entry.semantic.owner } else if entry.adopted != 0 {
            ledger.bindings.iter().find(|binding| binding.attempt == entry.adopted).and_then(|binding| binding.owner)
        } else { match entry.origin {
            Origin::Preparation(id) => ledger.bindings.iter().find(|b| b.attempt == id).and_then(|b| b.owner),
            Origin::Execution(owner) => Some(owner),
            Origin::KernelBookkeeping | Origin::OutsideSelected | Origin::Unknown => None,
        }};
        if owner.is_some() && bound != owner { continue; }
        if entry.adopted == 0 {
            match entry.origin {
                Origin::KernelBookkeeping => out.kernel_objects += 1,
                Origin::OutsideSelected => out.outside_selected_objects += 1,
                Origin::Unknown => out.unknown_objects += 1,
                _ => {},
            }
        }
        out.live_objects += 1;
        if let Some(bytes) = out.requested_bytes.checked_add(entry.size as u64) { out.requested_bytes = bytes; }
        else { overflow = true; out.invalid = true; }
        if entry.adopted != 0 || matches!(entry.origin, Origin::Preparation(_)) { out.attempt_objects += 1; }
    }
    if overflow { fault(ledger, trace::COUNTER); }
    // Named source-owned shared/runtime partitions are joined; pre-arm containers,
    // unused-path exclusion and the complete background/failure-path live maximum
    // still lack a pinned certificate. No observation promotes fit or coverage.
    out
}
/// Enumerate only after STOP+active=0; compare snapshot epochs around the entire
/// enumeration. Each copy takes the existing heap lock, never logs while held.
pub(crate) fn copy_entry(index: usize) -> Option<[u64; 19]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let entry = unsafe { ledger() }.entries.get(index)?;
    if entry.ptr == 0 { return None; }
    Some(entry_words(entry))
}
fn entry_words(entry: &Entry) -> [u64; 19] {
    let (kind, attempt, owner) = match entry.origin {
        Origin::KernelBookkeeping => (1, 0, None),
        Origin::OutsideSelected => (2, 0, None),
        Origin::Execution(owner) => (3, 0, Some(owner)),
        Origin::Preparation(attempt) => (4, attempt, None),
        Origin::Unknown => (5, 0, None),
    };
    let owner = owner.unwrap_or(Owner { tid: 0, root: 0, cell: 0, generation: 0 });
    let zero = Owner { tid: 0, root: 0, cell: 0, generation: 0 };
    let semantic = entry.semantic.owner.unwrap_or(zero);
    let from = entry.semantic.from.unwrap_or(zero);
    // Flag0x10 identifies a non-selected task, never a selected-subject join.
    let semantic_kind = entry.semantic.kind | if entry.semantic.owner.is_some_and(|owner| trace::subject(owner).is_none()) { 16 } else { 0 };
    [entry.ptr as u64, entry.size as u64, entry.align as u64, kind,
        attempt, entry.adopted, owner.tid, owner.root, owner.cell, owner.generation,
        semantic_kind, semantic.tid, semantic.root, semantic.cell, semantic.generation,
        from.tid, from.root, from.cell, from.generation]
}
/// One bounded static copy admitted by the caller's observation before STOP.
/// The existing heap lock binds this inventory to actual allocator used/free and
/// source epoch. Requested Layout bytes are not allocator extent/accounting bytes.
pub(crate) fn capture_baseline(_guard: &trace::Observation) -> Option<(u64, u64, u64)> {
    let heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if ledger.phase == Phase::Frozen { return None; }
    if ledger.baseline.source.is_some() { fault(ledger, trace::IDENTITY); return None; }
    let mut count = 0u64;
    let mut requested = 0u64;
    for entry in ledger.entries.iter().filter(|e| e.ptr != 0) {
        count += 1;
        let Some(next) = requested.checked_add(entry.size as u64) else {
            fault(ledger, trace::COUNTER); return None;
        };
        requested = next;
    }
    let epoch = super::P01_EPOCH.load(Ordering::Relaxed) as u64;
    let used = heap.used() as u64;
    let free = heap.free() as u64;
    ledger.baseline.charged_source = Some(charged_source(ledger, heap.used(), heap.free(), true));
    ledger.baseline.entries.copy_from_slice(&ledger.entries);
    ledger.baseline.source = Some([1, 1, ledger.epoch, ledger.invalid as u64,
        count, requested, epoch, used, free, 2, CAPACITY as u64,
        core::mem::size_of::<Baseline>() as u64]);
    Some((used, free, epoch))
}
/// One admitted, immutable census for an actually present sample 2..6. This is
/// the sole cutoff for both its source header and heap metrics 4/5. Capture does
/// not bump the allocator/ledger epochs or promote fit/coverage.
pub(crate) fn capture_stage(sample: usize, _guard: &trace::Observation) -> Option<(u64, u64, u64)> {
    let heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    // Frozen evidence must not change, even when a caller still holds a proof.
    if ledger.phase == Phase::Frozen { return None; }
    let Some(index) = sample.checked_sub(2).filter(|index| *index < ledger.stages.len()) else {
        fault(ledger, trace::IDENTITY); return None;
    };
    if ledger.phase != Phase::Armed { fault(ledger, trace::PROTOCOL); return None; }
    if ledger.stages[index].source.is_some() { fault(ledger, trace::IDENTITY); return None; }
    let (source, coherent) = charged_source_for_sample(ledger, heap.used(), heap.free(), sample as u64);
    if !coherent { return None; }
    ledger.stages[index].entries.copy_from_slice(&ledger.entries);
    ledger.stages[index].source = Some(source);
    Some((source[5], source[6], source[4]))
}
/// Collector-only enumeration after STOP, guard drain and heap freeze. Each
/// copy uses the original heap lock; no logging or allocation occurs while held.
pub(crate) fn copy_stage_source(sample: usize) -> Option<[u64; 22]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    ledger.stages.get(sample.checked_sub(2)?)?.source
}
pub(crate) fn copy_stage_entry(sample: usize, index: usize) -> Option<[u64; 19]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let stage = unsafe { ledger() }.stages.get(sample.checked_sub(2)?)?;
    stage.source?;
    let entry = stage.entries.get(index)?;
    (entry.ptr != 0).then(|| entry_words(entry))
}
pub(crate) fn copy_stage_charged_entry(sample: usize, index: usize) -> Option<[u64; 5]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let stage = unsafe { ledger() }.stages.get(sample.checked_sub(2)?)?;
    stage.source?;
    let entry = stage.entries.get(index)?;
    (entry.ptr != 0).then_some([entry.ptr as u64, entry.incarnation, entry.charged as u64, entry.birth_epoch, entry.prearm as u64])
}
pub(crate) fn copy_baseline_source() -> Option<[u64; 12]> {
    let _heap = super::ALLOCATOR.inner.lock();
    unsafe { ledger() }.baseline.source
}
pub(crate) fn copy_baseline_entry(index: usize) -> Option<[u64; 20]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let entry = unsafe { ledger() }.baseline.entries.get(index)?;
    if entry.ptr == 0 { return None; }
    let mut out = [0; 20];
    out[0] = entry.incarnation;
    out[1..].copy_from_slice(&entry_words(entry));
    Some(out)
}
pub(crate) fn copy_incarnation(index: usize) -> Option<[u64; 2]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let entry = unsafe { ledger() }.entries.get(index)?;
    (entry.ptr != 0).then_some([entry.ptr as u64, entry.incarnation])
}
/// Actual live allocation identity; never inferred from a physical backing page.
pub(crate) fn allocation_incarnation(ptr: usize) -> Option<u64> {
    let _heap = super::ALLOCATOR.inner.lock();
    unsafe { ledger() }.entries.iter()
        .find(|entry| entry.ptr == ptr && ptr != 0).map(|entry| entry.incarnation)
}
pub(crate) fn accounting() -> [u64; 4] {
    let heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    if ledger.phase == Phase::Frozen { ledger.frozen_accounting }
    else { [ledger.epoch, super::P01_EPOCH.load(Ordering::Relaxed) as u64, heap.used() as u64, heap.free() as u64] }
}
pub(crate) const fn attempt_capacity() -> usize { ATTEMPTS }
pub(crate) fn copy_binding(index: usize) -> Option<[u64; 6]> {
    let _heap = super::ALLOCATOR.inner.lock();
    let binding = unsafe { ledger() }.bindings.get(index)?;
    if binding.attempt == 0 { return None; }
    let owner = binding.owner.unwrap_or(Owner { tid: 0, root: 0, cell: 0, generation: 0 });
    Some([binding.attempt, owner.tid, owner.root, owner.cell, owner.generation,
        binding.closed as u64])
}
pub(crate) fn static_bytes() -> usize {
    core::mem::size_of_val(&STORAGE) + core::mem::size_of_val(&CONTEXT)
}
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn reset_for_test(epoch: u64) {
    let _heap = super::ALLOCATOR.inner.lock();
    let ledger = unsafe { ledger() };
    ledger.entries.fill(Entry::EMPTY);
    ledger.baseline.entries.fill(Entry::EMPTY);
    ledger.baseline.source = None;
    ledger.baseline.charged_source = None;
    for stage in &mut ledger.stages {
        stage.entries.fill(Entry::EMPTY);
        stage.source = None;
    }
    ledger.frozen_source = None;
    ledger.frozen_accounting = [0; 4];
    ledger.first_fault = None;
    ledger.phase = Phase::Boot;
    ledger.exhausted = false;
    ledger.prearm_allocations = 0;
    ledger.live = 0;
    ledger.highwater = 0;
    ledger.bindings.fill(Binding::EMPTY);
    ledger.contexts.fill(Semantic::EMPTY);
    ledger.epoch = epoch;
    ledger.invalid = false;
    for context in &CONTEXT { context.store(0, Ordering::Release); }
}
