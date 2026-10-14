//! Private source-owner resource observations. No runtime admission/reclamation decisions.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::p01_measurement::{self as trace, Owner};
use crate::task::Task;

#[derive(Clone, Copy)]
pub(crate) struct Metric {
    pub value: u64,
    pub epoch_before: u64,
    pub epoch_after: u64,
    pub status: u32,
    pub source_owner: u16,
}
impl Metric {
    const fn absent() -> Self {
        Self { value: 0, epoch_before: 0, epoch_after: 0, status: 2, source_owner: 0 }
    }
    fn valid(value: u64, epoch: u64, source_owner: u16) -> Self {
        Self { value, epoch_before: epoch, epoch_after: epoch, status: 1, source_owner }
    }
}

// This is a versioned copy, not a lock: writers never wait for readers or other
// writers. SeqCst places writer entry/exit, values and sample reads in one order.
// An overlapping writer makes the sample changed even if it restores a value.
pub(crate) struct Source {
    writers: AtomicU64,
    epoch: AtomicU64,
    faulted: AtomicBool,
}
impl Source {
    pub(crate) const fn new() -> Self {
        Self { writers: AtomicU64::new(0), epoch: AtomicU64::new(1), faulted: AtomicBool::new(false) }
    }
    pub(crate) fn write(&self) -> SourceWrite<'_> {
        if self.writers.fetch_add(1, Ordering::SeqCst) == u64::MAX { self.fault(); }
        SourceWrite(self)
    }
    pub(crate) fn fault(&self) {
        self.faulted.store(true, Ordering::SeqCst);
        trace::invalidate(trace::COUNTER);
    }
    pub(crate) fn before(&self) -> (u64, bool) {
        let epoch = self.epoch.load(Ordering::SeqCst);
        (epoch, self.writers.load(Ordering::SeqCst) != 0)
    }
    pub(crate) fn metric(&self, value: u64, before: (u64, bool), source_owner: u16) -> Metric {
        let busy = self.writers.load(Ordering::SeqCst) != 0;
        let after = self.epoch.load(Ordering::SeqCst);
        Metric { value, epoch_before: before.0, epoch_after: after,
            status: if self.faulted.load(Ordering::SeqCst) { 4 }
                else if before.1 || busy || before.0 != after { 3 } else { 1 },
            source_owner }
    }
}
pub(crate) struct SourceWrite<'a>(&'a Source);
impl Drop for SourceWrite<'_> {
    fn drop(&mut self) {
        if self.0.epoch.fetch_add(1, Ordering::SeqCst) == u64::MAX { self.0.fault(); }
        if self.0.writers.fetch_sub(1, Ordering::SeqCst) == 0 { self.0.fault(); }
    }
}
// Allocated stack/segment bytes, nonterminal operations, reserved completion slots,
// outstanding release obligations and opaque unknown accounting obligations.
static RESOURCE_SOURCE: Source = Source::new();
static COUNTERS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static LIVE_SUBJECTS: AtomicU64 = AtomicU64::new(0);

fn add(counter: &AtomicU64, delta: u64) {
    if counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst,
        |value| value.checked_add(delta)).is_err() { RESOURCE_SOURCE.fault(); }
}
fn sub(counter: &AtomicU64, delta: u64) {
    if counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst,
        |value| value.checked_sub(delta)).is_err() { RESOURCE_SOURCE.fault(); }
}

/// Copy-only source ownership survives trace admission/guard failures. The trace
/// token is optional; resource counters are updated at the actual source edge.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Obligation {
    trace: Option<trace::Obligation>,
    subject: u8,
    class: u16,
    kind: u8,
    reserved: bool,
    bytes: u64,
}
pub(crate) fn obligation_owner(token: Option<Obligation>) -> Option<Owner> {
    SUBJECTS.get(token?.subject as usize)?.owner()
}
struct Subject {
    ready: AtomicBool,
    tid: AtomicU64,
    root: AtomicU64,
    cell: AtomicU64,
    generation: AtomicU64,
    domain: AtomicU64,
    terminal: AtomicBool,
    dropped: AtomicBool,
    slot: AtomicBool,
    quota: AtomicBool,
    checked: AtomicU64,
    unknown: AtomicU64,
    retained: AtomicU64,
    pending: [AtomicU64; 8],
    retirement_pending: AtomicBool,
    finalized: AtomicBool,
}
impl Subject {
    const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false), tid: AtomicU64::new(0), root: AtomicU64::new(0),
            cell: AtomicU64::new(0), generation: AtomicU64::new(0), domain: AtomicU64::new(0),
            terminal: AtomicBool::new(false), dropped: AtomicBool::new(false),
            slot: AtomicBool::new(false), quota: AtomicBool::new(false),
            checked: AtomicU64::new(0), unknown: AtomicU64::new(0), retained: AtomicU64::new(0),
            pending: [const { AtomicU64::new(0) }; 8], retirement_pending: AtomicBool::new(false),
            finalized: AtomicBool::new(false),
        }
    }
    fn owner(&self) -> Option<Owner> {
        self.ready.load(Ordering::Acquire).then(|| Owner {
            tid: self.tid.load(Ordering::Relaxed), root: self.root.load(Ordering::Relaxed),
            cell: self.cell.load(Ordering::Relaxed), generation: self.generation.load(Ordering::Relaxed),
        })
    }
}
static SUBJECTS: [Subject; 9] = [const { Subject::new() }; 9];
// Every write to this source epoch is under SCHEDULER (publication/terminal).
static SCHEDULER_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Actual compiled fixed diagnostic storage, excluding core trace storage,
/// linker/code overhead and heap-backed queue/object diagnostic fields.
pub(crate) fn static_bytes() -> usize {
    core::mem::size_of_val(&SUBJECTS) + core::mem::size_of_val(&SCHEDULER_EPOCH)
        + core::mem::size_of_val(&RESOURCE_SOURCE) + core::mem::size_of_val(&COUNTERS)
        + core::mem::size_of_val(&LIVE_SUBJECTS) + crate::memory::cell_quota::p01_static_bytes()
        + 2 * core::mem::size_of::<core::sync::atomic::AtomicUsize>() // heap epoch/peak
        + core::mem::size_of::<u64>() + core::mem::size_of::<usize>() // frame epoch/peak
        + crate::memory::pin::p01_static_bytes()
        + crate::p01_retained::static_bytes()
        + crate::p01_topology::static_bytes()
        + crate::p01_noncpu::static_bytes()
        + crate::p01_noncpu_topology::static_bytes()
        + crate::p01_vtd_topology::static_bytes()
        + crate::memory::heap::p01_owner::static_bytes()
}

pub(crate) fn owner(task: &Task) -> Owner {
    Owner { tid: task.id as u64, root: task.root_tid as u64,
        cell: task.cell_id.0, generation: task.cell_generation }
}
fn state(owner: Owner) -> Option<&'static Subject> {
    let index = trace::subject(owner)?.0 as usize;
    SUBJECTS.get(index).filter(|subject| subject.owner() == Some(owner))
}
pub(crate) fn owner_for_tid(tid: usize) -> Option<Owner> {
    SUBJECTS.iter().filter_map(Subject::owner).find(|owner| owner.tid == tid as u64)
}
pub(crate) fn owner_for_domain(domain: u64) -> Option<Owner> {
    if domain == 0 { return None; }
    SUBJECTS.iter().find_map(|subject| {
        let owner = subject.owner()?;
        (subject.domain.load(Ordering::Relaxed) == domain).then_some(owner)
    })
}
fn bump_scheduler_epoch() {
    let epoch = SCHEDULER_EPOCH.load(Ordering::Relaxed);
    match epoch.checked_add(1) {
        Some(next) => SCHEDULER_EPOCH.store(next, Ordering::Relaxed),
        None => RESOURCE_SOURCE.fault(),
    }
}
/// Called after exact trace subject publication and before ready push, under SCHEDULER.
pub(crate) fn published(task: &mut Task) {
    let owner = owner(task);
    let Some(id) = trace::subject(owner) else { return; };
    {
        let _write = RESOURCE_SOURCE.write();
        let subject = &SUBJECTS[id.0 as usize];
        if subject.ready.load(Ordering::Acquire) { trace::invalidate(1 << 12); return; }
        subject.tid.store(owner.tid, Ordering::Relaxed);
        subject.root.store(owner.root, Ordering::Relaxed);
        subject.cell.store(owner.cell, Ordering::Relaxed);
        subject.generation.store(owner.generation, Ordering::Relaxed);
        if let crate::task::tcb::TaskAddressSpace::Domain(space) = &task.address_space {
            subject.domain.store(space.identity().raw(), Ordering::Relaxed);
        }
        crate::memory::cell_quota::p01_bind(id.0, owner.cell, owner.generation);
        add(&LIVE_SUBJECTS, 1);
        subject.ready.store(true, Ordering::Release);
        bump_scheduler_epoch();
    }
    if let Some(stack) = task.kernel_stack.as_mut() { stack.p01_obligation = start(owner, 1, stack.allocated_bytes() as u64, 1); }
    if let Some(stack) = task.user_stack.as_mut() { stack.p01_obligation = start(owner, 1, stack.allocated_bytes() as u64, 1); }
    if let Some(segments) = task.segment_mem.as_mut() { segments.p01_obligation = start(owner, 1, segments.allocated_bytes() as u64, 2); }
    if let Some(queue) = task.completion.as_ref() { queue.p01_bind(owner); }
    // Stack/CellSegments and task/argv transfer witnesses cover specific objects.
    // The heap-origin ledger still cannot certify shared-helper/runtime semantic
    // ownership or a complete simultaneous-allocation bound for class 8.
    unknown(owner, 8);
}
pub(crate) fn terminal(task: &Task) {
    let owner = owner(task);
    if let Some(subject) = state(owner) {
        {
            let _write = RESOURCE_SOURCE.write();
            if subject.terminal.swap(true, Ordering::SeqCst) { return; }
            sub(&LIVE_SUBJECTS, 1);
            subject.retirement_pending.store(true, Ordering::SeqCst);
            add(&COUNTERS[4], 1);
            bump_scheduler_epoch();
        }
        trace::task_terminal(owner);
    }
}
pub(crate) fn checked(owner: Owner, class: u16) {
    if !(1..=8).contains(&class) { trace::invalidate(trace::COUNTER); return; }
    if let Some(subject) = state(owner) {
        let _write = RESOURCE_SOURCE.write();
        if subject.finalized.load(Ordering::Acquire) { trace::invalidate(trace::LATE); return; }
        subject.checked.fetch_or(1 << (class - 1), Ordering::SeqCst);
    }
}
pub(crate) fn unknown(owner: Owner, class: u16) {
    if !(1..=8).contains(&class) { trace::invalidate(trace::COUNTER); return; }
    if let Some(subject) = state(owner) {
        let _write = RESOURCE_SOURCE.write();
        if subject.finalized.load(Ordering::Acquire) { trace::invalidate(trace::LATE); return; }
        // One opaque coverage obligation per owner/class, not an invented count
        // of heap allocations or physical frames inside the unverified set.
        let bit = 1 << (class - 1);
        if subject.unknown.fetch_or(bit, Ordering::SeqCst) & bit == 0 { add(&COUNTERS[5], 1); }
    }
}
pub(crate) fn retained(owner: Owner, class: u16) {
    if !(1..=8).contains(&class) { trace::invalidate(trace::COUNTER); return; }
    if let Some(subject) = state(owner) {
        let _write = RESOURCE_SOURCE.write();
        if subject.finalized.load(Ordering::Acquire) { trace::invalidate(trace::LATE); return; }
        subject.retained.fetch_or(1 << (class - 1), Ordering::SeqCst);
    }
}
fn start(owner: Owner, class: u16, bytes: u64, kind: u8) -> Option<Obligation> {
    if !(1..=8).contains(&class) { trace::invalidate(trace::COUNTER); return None; }
    let id = trace::subject(owner)?.0;
    let subject = SUBJECTS.get(id as usize).filter(|s| s.owner() == Some(owner))?;
    {
        let _write = RESOURCE_SOURCE.write();
        if subject.finalized.load(Ordering::Acquire) { trace::invalidate(trace::LATE); return None; }
        add(&subject.pending[class as usize - 1], 1);
        add(&COUNTERS[4], 1);
        match kind {
            1 | 2 => add(&COUNTERS[kind as usize - 1], bytes),
            3 | 4 => add(&COUNTERS[kind as usize - 1], 1),
            _ => {},
        }
    }
    Some(Obligation { trace: trace::obligation_start(owner, class, bytes),
        subject: id, class, kind, reserved: matches!(kind, 3 | 4), bytes })
}
pub(crate) fn obligation(owner: Owner, class: u16, bytes: u64) -> Option<Obligation> {
    start(owner, class, bytes, 0)
}
pub(crate) fn operation_obligation(owner: Owner) -> Option<Obligation> { start(owner, 2, 0, 3) }
pub(crate) fn completion_obligation(owner: Owner) -> Option<Obligation> { start(owner, 2, 0, 4) }
/// Terminal operations/completions stop being active/reserved, but retain their
/// cleanup obligation until take/drain/release/destruction actually frees them.
pub(crate) fn reservation_terminal(token: &mut Option<Obligation>) {
    let Some(token) = token.as_mut().filter(|token| token.reserved) else { return; };
    let _write = RESOURCE_SOURCE.write();
    if SUBJECTS[token.subject as usize].finalized.load(Ordering::Acquire) {
        trace::invalidate(trace::LATE);
        return;
    }
    sub(&COUNTERS[token.kind as usize - 1], 1);
    token.reserved = false;
}
pub(crate) fn finish(token: Option<Obligation>, result: u16) {
    let Some(token) = token else { return; };
    {
        let _write = RESOURCE_SOURCE.write();
        let subject = &SUBJECTS[token.subject as usize];
        if subject.finalized.load(Ordering::Acquire) { trace::invalidate(trace::LATE); return; }
        sub(&subject.pending[token.class as usize - 1], 1);
        sub(&COUNTERS[4], 1);
        if token.reserved { sub(&COUNTERS[token.kind as usize - 1], 1); }
        if result == 1 {
            if matches!(token.kind, 1 | 2) { sub(&COUNTERS[token.kind as usize - 1], token.bytes); }
        } else if result == 5 {
            subject.retained.fetch_or(1 << (token.class - 1), Ordering::SeqCst);
        } else {
            // Explicit unknown object outcome, separate from an opaque
            // owner/class coverage obligation recorded by unknown().
            add(&COUNTERS[5], 1);
            subject.unknown.fetch_or(1 << (token.class - 1), Ordering::SeqCst);
        }
    }
    if let Some(token) = token.trace { trace::obligation_finish(token, result); }
}
pub(crate) fn retry(owner: Owner, class: u16) {
    trace::component_retry(owner, class);
}

pub(crate) fn prepare_drop(task: &Task) {
    let owner = owner(task);
    if state(owner).is_none() { return; }
    if task.completion.is_none() { checked(owner, 2); }
    if !matches!(&task.address_space, crate::task::tcb::TaskAddressSpace::Domain(_)) {
        checked(owner, 6);
    }
    // Task fields have ownership, but allocator-side historical bytes do not.
    // Do not relabel execution-context quota refunds as object accounting.
    unknown(owner, 8);
}
/// This is after the complete Task destructor, not extraction from the scheduler.
pub(crate) fn dropped(owner: Owner) {
    if let Some(subject) = state(owner) {
        let _write = RESOURCE_SOURCE.write();
        subject.dropped.store(true, Ordering::SeqCst);
        checked(owner, 1);
    }
}
pub(crate) fn slot_result(owner: Owner, released: bool) {
    if let Some(subject) = state(owner) {
        {
            let _write = RESOURCE_SOURCE.write();
            subject.slot.store(released, Ordering::SeqCst);
        }
        trace::owner_slot_result(owner, released);
    }
}
pub(crate) fn quota_result(owner: Owner, released: bool) {
    if let Some(subject) = state(owner) {
        {
            let _write = RESOURCE_SOURCE.write();
            subject.quota.store(released, Ordering::SeqCst);
            if released && subject.dropped.load(Ordering::SeqCst)
                && subject.slot.load(Ordering::SeqCst)
                && subject.retirement_pending.swap(false, Ordering::SeqCst) {
                sub(&COUNTERS[4], 1);
            }
        }
        trace::quota_result(owner, released);
    }
}
pub(crate) fn finalize(frozen: &trace::Frozen, owner: Owner) {
    let Some(subject) = state(owner) else { return; };
    // Lack of source coverage is unknown, never an implicit checked-zero. A
    // pending cross-owner obligation remains incomplete even after task removal.
    let terminal = subject.terminal.load(Ordering::Acquire)
        && subject.dropped.load(Ordering::Acquire)
        && subject.slot.load(Ordering::Acquire)
        && subject.quota.load(Ordering::Acquire);
    let checked = subject.checked.load(Ordering::Acquire);
    let unknown = subject.unknown.load(Ordering::Acquire);
    let retained = subject.retained.load(Ordering::Acquire);
    // Source hooks finish before their terminal quota/slot trace event. Read
    // source pending as well: a refused trace token cannot become checked-zero.
    if subject.finalized.swap(true, Ordering::AcqRel) { return; }
    for class in 1..=8 {
        let bit = 1 << (class - 1);
        let counts = trace::component_counts(frozen, owner, class);
        let result = match counts {
            None => 7,
            Some(counts) if retained & bit != 0 || counts[2] != 0 => 5,
            Some(counts) if unknown & bit != 0 || counts[3] != 0 => 6,
            Some(counts) if !terminal || counts[0] != 0
                || subject.pending[class as usize - 1].load(Ordering::SeqCst) != 0 => 7,
            Some(_) if checked & bit == 0 => 6,
            Some(_) => 1,
        };
        trace::component_final(frozen, owner, class, result);
    }
}

pub(crate) fn finalize_all(frozen: &trace::Frozen) {
    for subject in &SUBJECTS {
        if let Some(owner) = subject.owner() {
            if !subject.finalized.load(Ordering::Acquire) { finalize(frozen, owner); }
        }
    }
}
pub(crate) fn reset_peaks() {
    crate::memory::heap::p01_reset_peak();
    if let Some(allocator) = crate::memory::frame::FRAME_ALLOCATOR.lock().as_mut() { allocator.p01_reset_peak(); }
}
pub(crate) fn peaks() -> [Metric; 2] {
    let (_, _, epoch, heap_peak) = crate::memory::heap::p01_snapshot();
    let heap = Metric::valid(heap_peak as u64, epoch as u64, 2);
    let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
    let frame_peak = frames.as_ref().map(|allocator| {
        let (epoch, peak) = allocator.p01_snapshot();
        Metric::valid(peak as u64, epoch, 1)
    }).unwrap_or_else(Metric::absent);
    [heap, frame_peak]
}
pub(crate) fn snapshot() -> [Metric; 16] {
    let mut metrics = [const { Metric::absent() }; 16];
    {
        let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        if let Some(allocator) = frames.as_ref() {
            let epoch = allocator.p01_snapshot().0;
            metrics[0] = Metric::valid(allocator.total_frames() as u64, epoch, 1);
            metrics[1] = Metric::valid(allocator.used_frames() as u64, epoch, 1);
            metrics[2] = Metric::valid(allocator.free_frames() as u64, epoch, 1);
            metrics[3] = Metric::valid(crate::memory::paging::PAGE_SIZE as u64, epoch, 1);
        }
    }
    {
        let (used, free, epoch, _) = crate::memory::heap::p01_snapshot();
        metrics[4] = Metric::valid(used as u64, epoch as u64, 2);
        metrics[5] = Metric::valid(free as u64, epoch as u64, 2);
    }
    {
        let _scheduler = crate::task::SCHEDULER.lock();
        let epoch = SCHEDULER_EPOCH.load(Ordering::Relaxed);
        metrics[9] = Metric::valid(LIVE_SUBJECTS.load(Ordering::SeqCst), epoch, 3);
        if RESOURCE_SOURCE.faulted.load(Ordering::SeqCst) { metrics[9].status = 4; }
    }
    metrics[6] = crate::memory::cell_quota::p01_snapshot();
    let before = RESOURCE_SOURCE.before();
    let values: [u64; 6] = core::array::from_fn(|i| COUNTERS[i].load(Ordering::SeqCst));
    // Read every value before the shared after-epoch. Separate subsystem values
    // still do not assert a simultaneous heap/frame/quota/resource aggregate.
    for (index, value) in [7, 8, 10, 11, 12, 14].into_iter().zip(values) {
        metrics[index] = RESOURCE_SOURCE.metric(value, before, 6);
    }
    metrics[13] = crate::p01_retained::metric();
    // The collector supplies invalid flags under its separate control owner.
    metrics
}
