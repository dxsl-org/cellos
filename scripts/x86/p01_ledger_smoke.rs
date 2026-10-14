//! Host behavior tests of the actual private ledger modules, not source-text tests.
//! Only observer admission, hart identity and owner-lock plumbing are substitutes;
//! this does not prove the kernel allocator, STOP gate, SMP or workload fit.
//! Run from the repository root: rustc --edition=2021 --test scripts/x86/p01_ledger_smoke.rs -o /tmp/p01-ledger-smoke
//! Then: /tmp/p01-ledger-smoke --test-threads=1
#![allow(dead_code)]
extern crate alloc;
#[path = "../../kernel/src/sync.rs"]
mod sync;
use sync::{Spinlock, SpinlockGuard};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};

struct Heap;
static TEST_HEAP_USED: AtomicU64 = AtomicU64::new(0);
static TEST_HEAP_FREE: AtomicU64 = AtomicU64::new(4096);
static P01_EPOCH: AtomicU64 = AtomicU64::new(1);
impl Heap {
    fn used(&self) -> usize { TEST_HEAP_USED.load(SeqCst) as usize }
    fn free(&self) -> usize { TEST_HEAP_FREE.load(SeqCst) as usize }
}
struct Allocator { inner: Spinlock<Heap> }
static ALLOCATOR: Allocator = Allocator { inner: Spinlock::new(Heap) };
static TEST_SERIAL: Spinlock<()> = Spinlock::new(());

mod task {
    pub mod smp { pub const MAX_HARTS: usize = 2; }
    pub mod hart_local {
        pub fn current_hart_id() -> usize { 0 }
        pub fn current_cell_id() -> usize { 3 }
        pub fn current_cell_generation() -> u64 { 1 }
    }
    pub struct Task {
        pub id: u64, pub root_tid: u64, pub cell_id: u64, pub cell_generation: u64,
        pub name: alloc::string::String, pub cwd: alloc::string::String,
        pub allowed_drivers: alloc::vec::Vec<usize>, pub inherited_argv: Option<alloc::vec::Vec<u8>>,
        pub segment_mem: Option<Segments>,
        pub staged_argv: Option<alloc::vec::Vec<u8>>,
        pub waiters: alloc::vec::Vec<usize>,
        pub pending_deaths: alloc::vec::Vec<(usize, usize)>,
        pub pending_owner_deaths: alloc::vec::Vec<(u64, usize, usize)>,
    }
    pub struct Segments;
    impl Segments {
        pub fn p01_backing(&self, _: crate::p01_measurement::Owner) {}
    }
}
mod p01_measurement {
    use super::*;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Owner { pub tid: u64, pub root: u64, pub cell: u64, pub generation: u64 }
    impl Owner {
        pub fn of(task: &crate::task::Task) -> Self {
            Self { tid: task.id, root: task.root_tid, cell: task.cell_id, generation: task.cell_generation }
        }
    }
    #[derive(Clone, Copy)]
    pub struct Attempt { pub id: u64 }
    impl Attempt { pub fn selected(self) -> bool { self.id != 0 } }
    pub const IDENTITY: u64 = 1; pub const COUNTER: u64 = 2; pub const PROTOCOL: u64 = 4;
    pub const FULL: u64 = 8; pub const INCOMPLETE: u64 = 16;
    pub const LATE: u64 = 32;
    pub static FLAGS: AtomicU64 = AtomicU64::new(0);
    pub static ADMITTED: AtomicBool = AtomicBool::new(true);
    pub struct Guard;
    pub type Observation = Guard;
    pub struct Frozen;
    pub fn observation() -> Option<Guard> { ADMITTED.load(SeqCst).then_some(Guard) }
    pub fn scoped_observation() -> Option<Guard> {
        if !ADMITTED.load(SeqCst) { invalidate(LATE); return None; }
        Some(Guard)
    }
    pub fn invalidate(bits: u64) { FLAGS.fetch_or(bits, SeqCst); }
    pub fn subject(owner: Owner) -> Option<u8> {
        ((owner.cell == 3 || owner.cell == 4) && owner.tid == owner.root && owner.generation == 1).then_some(0)
    }
    pub fn selected_owner(cell: u64, generation: u64) -> Option<Owner> {
        let owner = Owner { tid: 10, root: 10, cell, generation };
        subject(owner).map(|_| owner)
    }
}
mod p01_resources {
    use super::*;
    pub static EPOCH: AtomicU64 = AtomicU64::new(1);
    pub static INVALID: AtomicBool = AtomicBool::new(false);
    #[derive(Clone, Copy)]
    pub struct Metric { pub value: u64, pub status: u8 }
    // Minimal coherent single-thread owner adapter, not a replacement test of
    // the kernel's Source memory-order/interleaving implementation.
    pub struct Source;
    impl Source {
        pub const fn new() -> Self { Self }
        pub fn fault(&self) { INVALID.store(true, SeqCst); }
        pub fn write(&self) -> SourceWrite<'_> { EPOCH.fetch_add(1, SeqCst); SourceWrite(self) }
        pub fn before(&self) -> (u64, bool) { (EPOCH.load(SeqCst), INVALID.load(SeqCst)) }
        pub fn metric(&self, value: u64, before: (u64, bool), _: u8) -> Metric {
            let status = if before.1 || INVALID.load(SeqCst) { 4 }
                else if before.0 != EPOCH.load(SeqCst) { 2 } else { 1 };
            Metric { value, status }
        }
    }
    pub struct SourceWrite<'a>(&'a Source);
    impl Drop for SourceWrite<'_> { fn drop(&mut self) { EPOCH.fetch_add(1, SeqCst); } }
}
#[path = "../../kernel/src/memory/heap/p01_owner.rs"]
mod p01_owner;
#[path = "../../kernel/src/p01_retained.rs"]
mod p01_retained;
#[path = "../../kernel/src/p01_topology.rs"]
mod p01_topology;
#[path = "../../kernel/src/p01_noncpu.rs"]
mod p01_noncpu;
#[path = "../../kernel/src/p01_noncpu_topology.rs"]
mod p01_noncpu_topology;
#[path = "../../kernel/src/p01_vtd_topology.rs"]
mod p01_vtd_topology;
mod memory {
    pub mod paging { pub const PAGE_SIZE: usize = 4096; }
    pub mod heap { pub(crate) use crate::p01_owner; }
}
#[path = "p01_physical_smoke.rs"]
mod physical_smoke;

fn reset() -> SpinlockGuard<'static, ()> {
    let serial = TEST_SERIAL.lock();
    p01_measurement::FLAGS.store(0, SeqCst);
    p01_measurement::ADMITTED.store(true, SeqCst);
    p01_resources::EPOCH.store(1, SeqCst);
    p01_resources::INVALID.store(false, SeqCst);
    TEST_HEAP_USED.store(0, SeqCst);
    TEST_HEAP_FREE.store(4096, SeqCst);
    P01_EPOCH.store(1, SeqCst);
    p01_owner::reset_for_test(1);
    p01_retained::reset_for_test(1);
    serial
}
fn owner() -> p01_measurement::Owner {
    p01_measurement::Owner { tid: 10, root: 10, cell: 3, generation: 1 }
}
fn record(ptr: *mut u8, layout: core::alloc::Layout) {
    record_charged(ptr, layout, None, layout.size());
}
fn record_charged(ptr: *mut u8, layout: core::alloc::Layout, provenance: Option<p01_owner::Provenance>, charged: usize) {
    let _heap = ALLOCATOR.inner.lock();
    TEST_HEAP_USED.fetch_add(charged as u64, SeqCst);
    TEST_HEAP_FREE.fetch_sub(charged as u64, SeqCst);
    let epoch = P01_EPOCH.fetch_add(1, SeqCst) + 1;
    unsafe { p01_owner::allocated(ptr, layout, provenance, charged, epoch); }
}
fn release(ptr: *mut u8, layout: core::alloc::Layout) {
    let charged = (0..p01_owner::CAPACITY).filter_map(|index| p01_owner::copy_charged_entry(index, false))
        .find(|row| row[0] == ptr as u64).map_or(layout.size(), |row| row[2] as usize);
    let _heap = ALLOCATOR.inner.lock();
    TEST_HEAP_USED.fetch_sub(charged as u64, SeqCst);
    TEST_HEAP_FREE.fetch_add(charged as u64, SeqCst);
    P01_EPOCH.fetch_add(1, SeqCst);
    unsafe { p01_owner::freed(ptr, layout, charged); }
}

#[test]
fn heap_actual_buffer_origin_transfer_realloc_and_release() {
    let _serial = reset();
    let mut argv = Vec::<u8>::with_capacity(32);
    argv.extend_from_slice(b"hello");
    // This allocation predates the selected preparation; record the actual
    // pointer/layout as an execution-origin allocation before exclusive transfer.
    let layout = core::alloc::Layout::array::<u8>(argv.capacity()).unwrap();
    record(argv.as_mut_ptr(), layout);
    let scope = p01_owner::scope(Some(p01_measurement::Attempt { id: 1 }));
    p01_owner::adopt_vec(&argv);
    p01_owner::adopt_vec(&argv); // repeated exact ownership witness is idempotent
    p01_owner::bind(Some(p01_measurement::Attempt { id: 1 }), owner());
    let before = p01_owner::copy_entry(0).unwrap();
    assert_eq!(&before[3..6], &[3, 0, 1]);
    let provenance = {
        let _heap = ALLOCATOR.inner.lock();
        unsafe { p01_owner::realloc_origin(argv.as_mut_ptr(), layout) }
    };
    // Failed physical realloc has no allocation/free hook, so old storage stays.
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
    let new_layout = core::alloc::Layout::from_size_align(64, 1).unwrap();
    let new_ptr = unsafe { std::alloc::alloc(new_layout) };
    assert!(!new_ptr.is_null());
    record_charged(new_ptr, new_layout, Some(provenance), new_layout.size());
    unsafe { core::ptr::copy_nonoverlapping(argv.as_ptr(), new_ptr, argv.len()); }
    release(argv.as_mut_ptr(), layout);
    drop(argv);
    let new_entry = p01_owner::copy_entry(1).unwrap();
    assert_eq!(&new_entry[3..], &before[3..]);
    drop(scope);
    let snapshot = p01_owner::snapshot(Some(owner()));
    assert_eq!((snapshot.live_objects, snapshot.requested_bytes, snapshot.attempt_objects), (1, 64, 1));
    assert!(!snapshot.invalid);
    assert!(!snapshot.coverage_proven && !snapshot.fit_proven);
    unsafe { std::alloc::dealloc(new_ptr, new_layout); }
    release(new_ptr, new_layout);
    assert_eq!(p01_owner::snapshot(None).live_objects, 0);
}

#[test]
fn prearm_birth_origin_is_preserved_and_missing_join_is_not_invented() {
    let _serial = reset();
    let argv = vec![1u8, 2, 3];
    record(argv.as_ptr() as *mut u8, core::alloc::Layout::array::<u8>(argv.capacity()).unwrap());
    let scope = p01_owner::scope(Some(p01_measurement::Attempt { id: 1 }));
    p01_owner::adopt_vec(&argv);
    assert_eq!(&p01_owner::copy_entry(0).unwrap()[3..6], &[3, 0, 1]);
    let bookkeeping = p01_owner::bookkeeping();
    assert_eq!(p01_owner::current_attempt(), None);
    let layout = core::alloc::Layout::from_size_align(8, 8).unwrap();
    record(0x1000 as *mut u8, layout);
    assert_eq!(p01_owner::copy_entry(1).unwrap()[3], 1);
    drop(bookkeeping);
    assert_eq!(p01_owner::current_attempt(), Some(1));
    release(argv.as_ptr() as *mut u8, core::alloc::Layout::array::<u8>(argv.capacity()).unwrap());
    drop(argv);
    drop(scope);
    assert_eq!(p01_owner::snapshot(None).unresolved_attempts, 0);
    assert_eq!(p01_owner::snapshot(Some(owner())).kernel_objects, 0);
}

#[test]
fn heap_duplicate_layout_overflow_and_capacity_are_sticky_invalid() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(8, 8).unwrap();
    record(0x1000 as *mut u8, layout);
    record(0x1000 as *mut u8, layout);
    assert!(p01_owner::snapshot(None).invalid);
    p01_owner::reset_for_test(1);
    record(0x1000 as *mut u8, layout);
    release(0x1000 as *mut u8, core::alloc::Layout::from_size_align(16, 8).unwrap());
    assert!(p01_owner::snapshot(None).invalid);
    assert_eq!(p01_owner::snapshot(None).live_objects, 0);
    p01_owner::reset_for_test(u64::MAX);
    record(0x1000 as *mut u8, layout);
    assert!(p01_owner::snapshot(None).invalid);
    p01_owner::reset_for_test(1);
    for index in 0..=p01_owner::CAPACITY { record(((index + 1) * 16) as *mut u8, layout); }
    let snapshot = p01_owner::snapshot(None);
    assert!(snapshot.invalid);
    assert_eq!(snapshot.live_objects, p01_owner::CAPACITY as u64);
    let epoch = snapshot.epoch;
    for index in 0..100 {
        record((0x100000 + index * 16) as *mut u8, layout);
        release((0x100000 + index * 16) as *mut u8, layout);
    }
    assert_eq!(p01_owner::snapshot(None).epoch, epoch);
}

#[test]
fn retained_union_mark_and_actual_release_preserve_origin() {
    let _serial = reset();
    use p01_retained::Reason;
    let a = p01_retained::begin(Some(owner()), 4096, 3, Reason::RootTeardown).unwrap();
    let b = p01_retained::begin(Some(owner()), 8192, 1, Reason::Deferred).unwrap();
    let c = p01_retained::begin(Some(owner()), 12288, 2, Reason::Deferred).unwrap();
    assert_eq!(p01_retained::metric().value, 4 * 4096);
    p01_retained::mark(Some(a), Reason::AddressSpaceQuarantine);
    let range = p01_retained::copy_entry(0).unwrap();
    assert_eq!((range.reason, range.origin_reason), (5, 16));
    // A failed bitmap release never calls retire; the full union remains live.
    assert_eq!(p01_retained::metric().value, 4 * 4096);
    p01_retained::retire(Some(a));
    assert_eq!(p01_retained::metric().value, 3 * 4096);
    p01_retained::retire(Some(b));
    p01_retained::retire(Some(c));
    assert_eq!(p01_retained::metric().value, 0);
    assert_eq!(p01_retained::metric().status, 1);
}

#[test]
fn stale_retained_token_cannot_mutate_recycled_slot() {
    let _serial = reset();
    use p01_retained::Reason;
    let old = p01_retained::begin(Some(owner()), 4096, 1, Reason::Deferred).unwrap();
    p01_retained::retire(Some(old));
    let new = p01_retained::begin(Some(owner()), 8192, 1, Reason::RootTeardown).unwrap();
    p01_retained::mark(Some(old), Reason::AddressSpaceQuarantine);
    assert_eq!(p01_retained::copy_entry(0).unwrap().reason, 16);
    p01_retained::retire(Some(old));
    assert!(p01_retained::copy_entry(0).is_some());
    assert_eq!(p01_retained::metric().status, 4);
    p01_retained::retire(Some(new));
    assert!(p01_retained::copy_entry(0).is_none());
}

#[test]
fn retained_owner_overlap_overflow_and_capacity_fail_closed() {
    let _serial = reset();
    use p01_retained::Reason;
    p01_retained::begin(Some(owner()), 4096, 2, Reason::Deferred).unwrap();
    let other = p01_measurement::Owner { tid: 11, root: 11, cell: 4, generation: 1 };
    p01_retained::begin(Some(other), 8192, 1, Reason::Deferred).unwrap();
    assert_eq!(p01_retained::metric().status, 4);
    assert_eq!(p01_retained::metric().value, 0);
    p01_retained::reset_for_test(u64::MAX);
    assert!(p01_retained::begin(Some(owner()), 4096, 1, Reason::Deferred).is_none());
    p01_retained::reset_for_test(1);
    assert!(p01_retained::begin(Some(owner()), usize::MAX & !4095, 2, Reason::Deferred).is_none());
    p01_retained::reset_for_test(1);
    for index in 0..p01_retained::capacity() {
        assert!(p01_retained::begin(Some(owner()), (index + 1) * 4096, 1, Reason::Deferred).is_some());
    }
    assert!(p01_retained::begin(Some(owner()), 0, 1, Reason::Deferred).is_none());
    assert_eq!(p01_retained::metric().status, 4);
}

#[test]
fn exact_task_backing_buffers_are_adopted_without_inventing_origins() {
    let _serial = reset();
    let task = Box::new(task::Task {
        id: 10, root_tid: 10, cell_id: 3, cell_generation: 1,
        name: String::from("probe"), cwd: String::from("/"),
        allowed_drivers: Vec::new(), inherited_argv: Some(vec![1u8, 2, 3]),
        segment_mem: None,
        staged_argv: None, waiters: Vec::new(),
        pending_deaths: Vec::new(), pending_owner_deaths: Vec::new(),
    });
    record(&*task as *const task::Task as *mut u8, core::alloc::Layout::new::<task::Task>());
    for text in [&task.name, &task.cwd] {
        record(text.as_ptr() as *mut u8, core::alloc::Layout::array::<u8>(text.capacity()).unwrap());
    }
    let argv = task.inherited_argv.as_ref().unwrap();
    record(argv.as_ptr() as *mut u8, core::alloc::Layout::array::<u8>(argv.capacity()).unwrap());
    let scope = p01_owner::scope(Some(p01_measurement::Attempt { id: 1 }));
    p01_owner::adopt_task(&task);
    p01_owner::task_backing(&task); // exact owner before trace publication bind
    p01_owner::bind(Some(p01_measurement::Attempt { id: 1 }), owner());
    drop(scope);
    let snapshot = p01_owner::snapshot(Some(owner()));
    assert_eq!((snapshot.live_objects, snapshot.attempt_objects), (4, 4));
    assert_eq!(snapshot.unknown_objects, 0);
    assert!(!snapshot.invalid && !snapshot.coverage_proven && !snapshot.fit_proven);
    let entries: Vec<_> = (0..p01_owner::CAPACITY).filter_map(p01_owner::copy_entry).collect();
    assert!(entries.iter().all(|entry| entry[3] == 3 && entry[5] == 1 && entry[10] == p01_owner::TASK_BACKING));
    drop(task);
    for entry in entries {
        release(entry[0] as *mut u8,
            core::alloc::Layout::from_size_align(entry[1] as usize, entry[2] as usize).unwrap());
    }
    assert_eq!(p01_owner::snapshot(Some(owner())).live_objects, 0);
}

fn bind_runtime_owner(attempt: u64, owner: p01_measurement::Owner) {
    let scope = p01_owner::scope(Some(p01_measurement::Attempt { id: attempt }));
    p01_owner::bind(Some(p01_measurement::Attempt { id: attempt }), owner);
    drop(scope);
}

#[test]
fn runtime_wire_queue_commit_transfers_only_payload_and_free_uses_stored_owner() {
    let _serial = reset();
    let receiver = p01_measurement::Owner { tid: 20, root: 20, cell: 4, generation: 1 };
    bind_runtime_owner(1, owner());
    bind_runtime_owner(2, receiver);
    let wire = vec![1u8, 2, 3].into_boxed_slice();
    let reply = vec![0u8; 4096].into_boxed_slice();
    let wire_layout = core::alloc::Layout::array::<u8>(wire.len()).unwrap();
    let reply_layout = core::alloc::Layout::array::<u8>(reply.len()).unwrap();
    let bookkeeping = p01_owner::bookkeeping();
    record(wire.as_ptr() as *mut u8, wire_layout);
    record(reply.as_ptr() as *mut u8, reply_layout);
    drop(bookkeeping);
    p01_owner::runtime_buffer(wire.as_ptr() as usize, wire_layout, owner(), p01_owner::SENDER_WIRE);
    p01_owner::runtime_buffer(reply.as_ptr() as usize, reply_layout, owner(), p01_owner::REPLY);
    let before = p01_owner::copy_entry(0).unwrap();
    // Failed queue publication has no transfer hook; both allocations stay caller-owned.
    assert_eq!(p01_owner::snapshot(Some(owner())).requested_bytes, 4099);
    assert_eq!(p01_owner::snapshot(Some(receiver)).live_objects, 0);
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
    p01_owner::transfer_wire(wire.as_ptr() as usize, wire_layout, receiver, 10, 3, 1);
    let queued = p01_owner::copy_entry(0).unwrap();
    assert_eq!(queued[3], 1, "kernel allocation origin must remain immutable");
    assert_eq!(queued[5], 2, "prefer receiver's existing exact attempt binding");
    assert_eq!(&queued[10..15], &[p01_owner::QUEUED_WIRE, 20, 20, 4, 1]);
    assert_eq!(&queued[15..19], &[10, 10, 3, 1]);
    assert_eq!(p01_owner::snapshot(Some(owner())).requested_bytes, 4096);
    assert_eq!(p01_owner::snapshot(Some(receiver)).requested_bytes, 3);
    assert_eq!(p01_owner::copy_entry(1).unwrap()[10], p01_owner::REPLY);
    release(wire.as_ptr() as *mut u8, wire_layout);
    release(reply.as_ptr() as *mut u8, reply_layout);
    assert_eq!(p01_owner::snapshot(None).live_objects, 0);
    assert!(!p01_owner::snapshot(None).invalid);
}

#[test]
fn runtime_vec_realloc_keeps_exact_semantic_join_and_failed_realloc_keeps_old_entry() {
    let _serial = reset();
    bind_runtime_owner(1, owner());
    let rows = Vec::<u64>::with_capacity(4);
    let layout = core::alloc::Layout::array::<u64>(rows.capacity()).unwrap();
    let bookkeeping = p01_owner::bookkeeping();
    record(rows.as_ptr() as *mut u8, layout);
    drop(bookkeeping);
    p01_owner::runtime_vec(&rows, owner(), p01_owner::OPERATIONS);
    let before = p01_owner::copy_entry(0).unwrap();
    let provenance = {
        let _heap = ALLOCATOR.inner.lock();
        unsafe { p01_owner::realloc_origin(rows.as_ptr() as *mut u8, layout) }
    };
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
    let larger = core::alloc::Layout::array::<u64>(8).unwrap();
    let ptr = unsafe { std::alloc::alloc(larger) };
    assert!(!ptr.is_null());
    record_charged(ptr, larger, Some(provenance), larger.size());
    release(rows.as_ptr() as *mut u8, layout);
    assert_eq!(&p01_owner::copy_entry(1).unwrap()[3..], &before[3..]);
    assert_eq!(p01_owner::snapshot(Some(owner())).requested_bytes, 64);
    unsafe { std::alloc::dealloc(ptr, larger); }
    release(ptr, larger);
    assert!(!p01_owner::snapshot(None).invalid);
}

#[test]
fn runtime_wire_forged_sender_and_reply_transfer_are_rejected_without_rewriting_entry() {
    let _serial = reset();
    bind_runtime_owner(1, owner());
    let receiver = p01_measurement::Owner { tid: 20, root: 20, cell: 4, generation: 1 };
    bind_runtime_owner(2, receiver);
    let bytes = vec![1u8, 2, 3];
    let layout = core::alloc::Layout::array::<u8>(bytes.capacity()).unwrap();
    record(bytes.as_ptr() as *mut u8, layout);
    p01_owner::runtime_buffer(bytes.as_ptr() as usize, layout, owner(), p01_owner::SENDER_WIRE);
    let before = p01_owner::copy_entry(0).unwrap();
    p01_owner::transfer_wire(bytes.as_ptr() as usize, layout, receiver, 10, 3, 2);
    assert!(p01_owner::snapshot(None).invalid);
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
    p01_owner::reset_for_test(1);
    bind_runtime_owner(1, owner());
    bind_runtime_owner(2, receiver);
    record(bytes.as_ptr() as *mut u8, layout);
    p01_owner::runtime_buffer(bytes.as_ptr() as usize, layout, owner(), p01_owner::REPLY);
    let before = p01_owner::copy_entry(0).unwrap();
    p01_owner::transfer_wire(bytes.as_ptr() as usize, layout, receiver, 10, 3, 1);
    assert!(p01_owner::snapshot(None).invalid);
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
}

#[test]
fn runtime_queue_transfer_to_unselected_receiver_does_not_retain_selected_sender_join() {
    let _serial = reset();
    bind_runtime_owner(1, owner());
    let outside = p01_measurement::Owner { tid: 30, root: 30, cell: 5, generation: 1 };
    let bytes = vec![1u8, 2, 3];
    let layout = core::alloc::Layout::array::<u8>(bytes.capacity()).unwrap();
    let bookkeeping = p01_owner::bookkeeping();
    record(bytes.as_ptr() as *mut u8, layout);
    drop(bookkeeping);
    p01_owner::runtime_buffer(bytes.as_ptr() as usize, layout, owner(), p01_owner::SENDER_WIRE);
    p01_owner::transfer_wire(bytes.as_ptr() as usize, layout, outside, 10, 3, 1);
    let entry = p01_owner::copy_entry(0).unwrap();
    assert_eq!((entry[3], entry[5], entry[10]), (1, 0, 20));
    assert_eq!(&entry[11..15], &[30, 30, 5, 1]);
    assert_eq!(&entry[15..19], &[10, 10, 3, 1]);
    assert_eq!(p01_owner::snapshot(Some(owner())).live_objects, 0);
    assert_eq!(p01_owner::snapshot(Some(outside)).requested_bytes, 3);
    release(bytes.as_ptr() as *mut u8, layout);
    record(bytes.as_ptr() as *mut u8, layout);
    p01_owner::runtime_buffer(bytes.as_ptr() as usize, layout, outside, p01_owner::SENDER_WIRE);
    p01_owner::transfer_wire(bytes.as_ptr() as usize, layout, owner(), 30, 5, 1);
    let received = p01_owner::copy_entry(0).unwrap();
    assert_eq!((received[5], received[10]), (1, p01_owner::QUEUED_WIRE));
    assert_eq!(&received[11..15], &[10, 10, 3, 1]);
    assert_eq!(&received[15..19], &[30, 30, 5, 1]);
    assert_eq!(p01_owner::snapshot(Some(owner())).requested_bytes, 3);
    release(bytes.as_ptr() as *mut u8, layout);
    assert!(!p01_owner::snapshot(None).invalid);
}

#[test]
fn census_tracks_closed_gate_boot_and_arm_gap_then_freezes_under_heap_lock() {
    let _serial = reset();
    let outside = p01_measurement::Owner { tid: 30, root: 30, cell: 0, generation: 0 };
    let wire = vec![1u8, 2, 3];
    let layout = core::alloc::Layout::array::<u8>(wire.capacity()).unwrap();
    p01_measurement::ADMITTED.store(false, SeqCst);
    assert!(p01_measurement::observation().is_none());
    let creation = p01_owner::creation(Some(outside), p01_owner::SENDER_WIRE);
    record_charged(wire.as_ptr() as *mut u8, layout, None, 16);
    drop(creation);
    assert_eq!(p01_owner::copy_entry(0).unwrap()[10], p01_owner::SENDER_WIRE | 16);
    p01_owner::arm();
    record_charged(0x1000 as *mut u8, layout, None, 16);
    p01_measurement::ADMITTED.store(true, SeqCst);
    let guard = p01_measurement::observation().unwrap();
    p01_owner::capture_baseline(&guard).unwrap();
    let baseline = p01_owner::copy_charged_source(true).unwrap();
    assert_eq!(&baseline[7..14], &[2, 6, 32, 0, 16, 0, 16]);
    assert_eq!(&baseline[14..17], &[1, 1, 2]);
    p01_measurement::ADMITTED.store(false, SeqCst);
    release(0x1000 as *mut u8, layout);
    p01_owner::freeze(&p01_measurement::Frozen);
    let before = p01_owner::copy_entry(0).unwrap();
    let source = p01_owner::copy_charged_source(false).unwrap();
    let accounting = p01_owner::accounting();
    let epoch = p01_owner::snapshot(None).epoch;
    p01_owner::runtime_buffer(wire.as_ptr() as usize, layout, owner(), p01_owner::MAILBOX);
    drop(p01_owner::creation(None, p01_owner::QUOTA_SHARED));
    record_charged(0x2000 as *mut u8, layout, None, 16);
    release(wire.as_ptr() as *mut u8, layout);
    assert_eq!(p01_owner::copy_entry(0).unwrap(), before);
    assert_eq!(p01_owner::copy_charged_source(false).unwrap(), source);
    assert_eq!(p01_owner::accounting(), accounting);
    assert_eq!(p01_owner::snapshot(None).epoch, epoch);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
    assert!(p01_measurement::observation().is_none());
    assert!(p01_measurement::scoped_observation().is_none());
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::LATE);
}

#[test]
fn shared_creation_nested_owner_scratch_and_realloc_preserve_actual_joins() {
    let _serial = reset();
    let preparation = p01_owner::scope(Some(p01_measurement::Attempt { id: 1 }));
    p01_owner::bind(Some(p01_measurement::Attempt { id: 1 }), owner());
    let shared = p01_owner::creation(None, p01_owner::QUOTA_SHARED);
    let quota = Vec::<u64>::with_capacity(4);
    let quota_layout = core::alloc::Layout::array::<u64>(quota.capacity()).unwrap();
    record(quota.as_ptr() as *mut u8, quota_layout);
    let scratch_scope = p01_owner::creation(Some(owner()), p01_owner::RECEIVE_SCRATCH);
    let scratch = Vec::<u8>::with_capacity(3);
    let scratch_layout = core::alloc::Layout::array::<u8>(scratch.capacity()).unwrap();
    record(scratch.as_ptr() as *mut u8, scratch_layout);
    drop(scratch_scope);
    let second = Vec::<u64>::with_capacity(2);
    let second_layout = core::alloc::Layout::array::<u64>(second.capacity()).unwrap();
    record(second.as_ptr() as *mut u8, second_layout);
    drop(shared);
    drop(preparation);
    let first = p01_owner::copy_entry(0).unwrap();
    assert_eq!((first[3], first[4], first[5], first[10]), (4, 1, 0, p01_owner::QUOTA_SHARED));
    assert_eq!(&first[11..19], &[0; 8]);
    assert_eq!(p01_owner::copy_entry(2).unwrap()[10], p01_owner::QUOTA_SHARED);
    assert_eq!(p01_owner::snapshot(Some(owner())).requested_bytes, 3);
    let replacement = Vec::<u64>::with_capacity(8);
    let replacement_layout = core::alloc::Layout::array::<u64>(replacement.capacity()).unwrap();
    let provenance = {
        let _heap = ALLOCATOR.inner.lock();
        unsafe { p01_owner::realloc_origin(quota.as_ptr() as *mut u8, quota_layout) }
    };
    record_charged(replacement.as_ptr() as *mut u8, replacement_layout, Some(provenance), replacement_layout.size());
    release(quota.as_ptr() as *mut u8, quota_layout);
    let moved = p01_owner::copy_entry(3).unwrap();
    assert_eq!(&moved[3..19], &first[3..19]);
    release(replacement.as_ptr() as *mut u8, replacement_layout);
    release(second.as_ptr() as *mut u8, second_layout);
    release(scratch.as_ptr() as *mut u8, scratch_layout);
    let final_state = p01_owner::snapshot(None);
    assert_eq!(final_state.live_objects, 0);
    assert!(!final_state.invalid && !final_state.fit_proven && !final_state.coverage_proven);
}

#[test]
fn kernel_log_handoff_preserves_preparation_and_prearm_unknown_origin() {
    let _serial = reset();
    let scope = p01_owner::scope(Some(p01_measurement::Attempt { id: 1 }));
    let path = Vec::<u8>::with_capacity(32);
    let layout = core::alloc::Layout::array::<u8>(path.capacity()).unwrap();
    record(path.as_ptr() as *mut u8, layout);
    p01_owner::shared_buffer(path.as_ptr() as usize, layout, p01_owner::LOG_PATH);
    p01_owner::bind(Some(p01_measurement::Attempt { id: 1 }), owner());
    drop(scope);
    assert_eq!(p01_owner::snapshot(Some(owner())).live_objects, 0);
    assert_eq!((p01_owner::copy_entry(0).unwrap()[3], p01_owner::copy_entry(0).unwrap()[10]), (4, 8));
    release(path.as_ptr() as *mut u8, layout);
    let prearm = Vec::<u8>::with_capacity(16);
    let layout = core::alloc::Layout::array::<u8>(prearm.capacity()).unwrap();
    record(prearm.as_ptr() as *mut u8, layout);
    p01_owner::shared_buffer(prearm.as_ptr() as usize, layout, p01_owner::LOG_SHARED);
    let entry = p01_owner::copy_entry(0).unwrap();
    assert_eq!((entry[3], entry[5], entry[10]), (3, 0, 7));
    assert_eq!(p01_owner::snapshot(None).unknown_objects, 0);
    release(prearm.as_ptr() as *mut u8, layout);
}

#[test]
fn synthetic_commit_and_migration_copy_do_not_invent_sender_incarnation() {
    let _serial = reset();
    bind_runtime_owner(1, owner());
    let synthetic = vec![1u8, 2, 3];
    let layout = core::alloc::Layout::array::<u8>(synthetic.capacity()).unwrap();
    record(synthetic.as_ptr() as *mut u8, layout);
    p01_owner::shared_buffer(synthetic.as_ptr() as usize, layout, p01_owner::SYNTHETIC_WIRE);
    p01_owner::transfer_wire(synthetic.as_ptr() as usize, layout, owner(), 0, 0, 0);
    let entry = p01_owner::copy_entry(0).unwrap();
    assert_eq!((entry[5], entry[10]), (1, p01_owner::LEGACY_PAYLOAD));
    assert_eq!(&entry[15..19], &[0; 4]);
    let migration = p01_owner::creation(Some(owner()), p01_owner::MIGRATION_COPY);
    let copy = synthetic.clone();
    let copy_layout = core::alloc::Layout::array::<u8>(copy.capacity()).unwrap();
    record(copy.as_ptr() as *mut u8, copy_layout);
    drop(migration);
    assert_eq!(p01_owner::copy_entry(1).unwrap()[10], p01_owner::MIGRATION_COPY);
    release(copy.as_ptr() as *mut u8, copy_layout);
    release(synthetic.as_ptr() as *mut u8, layout);
    assert!(!p01_owner::snapshot(None).invalid);
}

#[test]
fn census_creation_guards_work_prearm_and_reject_owner_forgery() {
    let _serial = reset();
    p01_measurement::ADMITTED.store(false, SeqCst);
    drop(p01_owner::creation(None, p01_owner::QUOTA_SHARED));
    drop(p01_owner::creation(Some(owner()), p01_owner::RECEIVE_SCRATCH));
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
    assert_eq!(p01_owner::snapshot(None).epoch, 1);
    p01_measurement::ADMITTED.store(true, SeqCst);
    drop(p01_owner::creation(Some(owner()), p01_owner::COMPLETION_SHARED));
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::IDENTITY);
}

#[test]
fn heap_sample1_static_copy_keeps_layout_semantics_and_pointer_reuse_incarnations() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(32, 8).unwrap();
    let creation = p01_owner::creation(None, p01_owner::QUOTA_SHARED);
    record_charged(0x1000 as *mut u8, layout, None, 64);
    drop(creation);
    TEST_HEAP_USED.store(64, SeqCst);
    TEST_HEAP_FREE.store(4032, SeqCst);
    P01_EPOCH.store(7, SeqCst);
    let guard = p01_measurement::scoped_observation().unwrap();
    assert_eq!(p01_owner::capture_baseline(&guard), Some((64, 4032, 7)));
    drop(guard);
    let baseline = p01_owner::copy_baseline_entry(0).unwrap();
    let source = p01_owner::copy_baseline_source().unwrap();
    assert_eq!(&source[..11], &[1, 1, 2, 0, 1, 32, 7, 64, 4032, 2, 1024]);
    assert_eq!(&baseline[1..], &p01_owner::copy_entry(0).unwrap());
    assert_eq!(baseline[11], p01_owner::QUOTA_SHARED);
    assert!(p01_owner::static_bytes() as u64 > source[11]);
    release(0x1000 as *mut u8, layout);
    let creation = p01_owner::creation(None, p01_owner::LOG_SHARED);
    record(0x1000 as *mut u8, layout);
    drop(creation);
    assert_eq!(p01_owner::copy_baseline_entry(0).unwrap(), baseline);
    let final_id = p01_owner::copy_incarnation(0).unwrap();
    assert_eq!(final_id[0], baseline[1]);
    assert!(final_id[1] > source[2]);
    assert_eq!(p01_owner::copy_baseline_source().unwrap(), source);
    p01_measurement::ADMITTED.store(false, SeqCst);
    assert_eq!(p01_owner::copy_baseline_entry(0).unwrap(), baseline);
    assert!(!p01_owner::snapshot(None).invalid);
}

#[test]
fn heap_baseline_duplicate_and_bounded_enumeration_are_rejected_without_dynamic_storage() {
    let _serial = reset();
    let guard = p01_measurement::scoped_observation().unwrap();
    assert!(p01_owner::capture_baseline(&guard).is_some());
    assert!(p01_owner::capture_baseline(&guard).is_none());
    assert_ne!(p01_measurement::FLAGS.load(SeqCst) & p01_measurement::IDENTITY, 0);
    drop(guard);
    p01_owner::reset_for_test(1);
    let guard = p01_measurement::scoped_observation().unwrap();
    assert!(p01_owner::capture_baseline(&guard).is_some());
    assert!(p01_owner::copy_baseline_entry(p01_owner::CAPACITY).is_none());
    assert_eq!(p01_owner::copy_baseline_source().unwrap()[4..6], [0, 0]);
}

#[test]
fn heap_stage_cutoff_waits_for_heap_lock_and_frozen_capture_cannot_mutate_it() {
    let _serial = reset();
    p01_owner::arm();
    let layout = core::alloc::Layout::from_size_align(32, 8).unwrap();
    let heap = ALLOCATOR.inner.lock();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (captured_tx, captured_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let guard = p01_measurement::observation().unwrap();
        started_tx.send(()).unwrap();
        captured_tx.send(p01_owner::capture_stage(2, &guard)).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(matches!(captured_rx.recv_timeout(std::time::Duration::from_millis(25)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
    // Physical allocation/accounting and its ledger hook occur under the very
    // same guard that the stage capture is waiting to acquire.
    TEST_HEAP_USED.store(64, SeqCst);
    TEST_HEAP_FREE.store(4032, SeqCst);
    P01_EPOCH.store(7, SeqCst);
    unsafe { p01_owner::allocated(0x1000 as *mut u8, layout, None, 64, 7); }
    drop(heap);
    assert_eq!(captured_rx.recv().unwrap(), Some((64, 4032, 7)));
    worker.join().unwrap();
    let source = p01_owner::copy_stage_source(2).unwrap();
    let entry = p01_owner::copy_stage_entry(2, 0).unwrap();
    let charged = p01_owner::copy_stage_charged_entry(2, 0).unwrap();
    assert_eq!(&source[..19], &[1, 2, 1, 3, 7, 64, 4032, 1, 32, 64, 0, 64, 0, 0, 0, 0, 1, 0, 1024]);
    assert_eq!(source[19], p01_owner::static_bytes() as u64);
    assert_eq!(&source[20..], &[0, 2]);
    assert_eq!(entry, p01_owner::copy_entry(0).unwrap());
    assert_eq!(charged, [0x1000, 3, 64, 7, 0]);
    p01_measurement::ADMITTED.store(false, SeqCst);
    p01_owner::freeze(&p01_measurement::Frozen);
    let frozen = p01_owner::copy_charged_source(false).unwrap();
    let accounting = p01_owner::accounting();
    let epoch = p01_owner::snapshot(None).epoch;
    // A previously admitted observation cannot reopen a frozen inventory.
    let guard = p01_measurement::Guard;
    for sample in [0, 1, 2, 3, 6, 7, usize::MAX] {
        assert_eq!(p01_owner::capture_stage(sample, &guard), None);
    }
    p01_owner::runtime_buffer(0x1000, layout, owner(), p01_owner::MAILBOX);
    release(0x1000 as *mut u8, layout);
    record(0x2000 as *mut u8, layout);
    assert_eq!(p01_owner::copy_stage_source(2).unwrap(), source);
    assert_eq!(p01_owner::copy_stage_entry(2, 0).unwrap(), entry);
    assert_eq!(p01_owner::copy_stage_charged_entry(2, 0).unwrap(), charged);
    assert!(p01_owner::copy_stage_source(3).is_none());
    assert_eq!(p01_owner::copy_charged_source(false).unwrap(), frozen);
    assert_eq!(p01_owner::accounting(), accounting);
    assert_eq!(p01_owner::snapshot(None).epoch, epoch);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
}

#[test]
fn heap_stages_allow_missing_denied_samples_and_reject_duplicates_bounds_and_boot_capture() {
    let _serial = reset();
    let guard = p01_measurement::observation().unwrap();
    assert!(p01_owner::capture_stage(2, &guard).is_none());
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::PROTOCOL);
    p01_owner::reset_for_test(1);
    p01_measurement::FLAGS.store(0, SeqCst);
    p01_owner::capture_baseline(&guard).unwrap();
    p01_owner::arm();
    assert_eq!(p01_owner::capture_stage(3, &guard), Some((0, 4096, 1)));
    let denied = p01_owner::copy_stage_source(3).unwrap();
    for sample in [0, 1, 2, 4, 5, 6, 7, usize::MAX] {
        assert!(p01_owner::copy_stage_source(sample).is_none());
        assert!(p01_owner::copy_stage_entry(sample, 0).is_none());
        assert!(p01_owner::copy_stage_charged_entry(sample, 0).is_none());
    }
    assert!(p01_owner::copy_stage_entry(3, 0).is_none());
    assert!(p01_owner::copy_stage_charged_entry(3, p01_owner::CAPACITY).is_none());
    assert!(p01_owner::capture_stage(3, &guard).is_none());
    for sample in [0, 1, 7, usize::MAX] {
        assert!(p01_owner::capture_stage(sample, &guard).is_none());
    }
    assert_eq!(p01_owner::copy_stage_source(3).unwrap(), denied);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::IDENTITY);
    let summary = p01_owner::snapshot(None);
    assert!(summary.invalid && !summary.fit_proven && !summary.coverage_proven);
}

#[test]
fn heap_stages_preserve_surviving_origin_charge_and_birth_across_transfer_and_pointer_reuse() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(32, 8).unwrap();
    let creation = p01_owner::creation(Some(owner()), p01_owner::SENDER_WIRE);
    record_charged(0x1000 as *mut u8, layout, None, 64);
    drop(creation);
    let guard = p01_measurement::observation().unwrap();
    p01_owner::capture_baseline(&guard).unwrap();
    let baseline = p01_owner::copy_baseline_entry(0).unwrap();
    let baseline_charge = p01_owner::copy_charged_entry(0, true).unwrap();
    p01_owner::arm();
    p01_owner::capture_stage(2, &guard).unwrap();
    let before = p01_owner::copy_stage_entry(2, 0).unwrap();
    let receiver = p01_measurement::Owner { tid: 20, root: 20, cell: 4, generation: 1 };
    p01_owner::transfer_wire(0x1000, layout, receiver, owner().tid, owner().cell, owner().generation);
    p01_owner::capture_stage(3, &guard).unwrap();
    let transferred = p01_owner::copy_stage_entry(3, 0).unwrap();
    assert_eq!(&before[..10], &transferred[..10]);
    assert_eq!(&baseline[1..11], &before[..10]);
    assert_eq!(before[10], p01_owner::SENDER_WIRE);
    assert_eq!(transferred[10], p01_owner::QUEUED_WIRE);
    assert_eq!(&transferred[11..15], &[20, 20, 4, 1]);
    assert_eq!(&transferred[15..], &[10, 10, 3, 1]);
    assert_eq!(p01_owner::copy_stage_charged_entry(2, 0).unwrap(), baseline_charge);
    assert_eq!(p01_owner::copy_stage_charged_entry(3, 0).unwrap(), baseline_charge);
    release(0x1000 as *mut u8, layout);
    p01_owner::capture_stage(4, &guard).unwrap();
    assert_eq!(p01_owner::copy_stage_source(4).unwrap()[7..14], [0; 7]);
    assert!(p01_owner::copy_stage_entry(4, 0).is_none());
    let creation = p01_owner::creation(None, p01_owner::LOG_SHARED);
    record_charged(0x1000 as *mut u8, layout, None, 48);
    drop(creation);
    p01_owner::capture_stage(5, &guard).unwrap();
    p01_owner::capture_stage(6, &guard).unwrap();
    let reused = p01_owner::copy_stage_charged_entry(5, 0).unwrap();
    assert_eq!(reused[0], baseline_charge[0]);
    assert!(reused[1] > baseline_charge[1] && reused[3] > baseline_charge[3]);
    assert_eq!((reused[2], reused[4]), (48, 0));
    assert_eq!(p01_owner::copy_stage_entry(5, 0).unwrap()[10], p01_owner::LOG_SHARED);
    assert_eq!(p01_owner::copy_stage_charged_entry(6, 0).unwrap(), reused);
    p01_measurement::ADMITTED.store(false, SeqCst);
    p01_owner::freeze(&p01_measurement::Frozen);
    for sample in 2..=6 {
        let source = p01_owner::copy_stage_source(sample).unwrap();
        assert_eq!((source[1], source[2], source[10], source[20]), (sample as u64, 1, 0, 0));
        assert_eq!(source[19], p01_owner::static_bytes() as u64);
        assert!(p01_owner::copy_stage_entry(sample, p01_owner::CAPACITY).is_none());
        assert!(p01_owner::copy_stage_charged_entry(sample, p01_owner::CAPACITY).is_none());
    }
    assert_eq!(p01_owner::copy_stage_entry(2, 0).unwrap(), before);
    assert_eq!(p01_owner::copy_stage_entry(3, 0).unwrap(), transferred);
    assert_eq!(p01_owner::copy_stage_charged_entry(2, 0).unwrap(), baseline_charge);
    assert_eq!(p01_owner::copy_charged_entry(0, false).unwrap(), reused);
    let summary = p01_owner::snapshot(None);
    assert!(!summary.invalid && !summary.fit_proven && !summary.coverage_proven);
}

#[test]
fn heap_stage_missing_witness_unmatched_used_and_charge_overflow_refuse_storage() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(8, 8).unwrap();
    let guard = p01_measurement::observation().unwrap();
    p01_owner::arm();
    p01_owner::runtime_buffer(0x1000, layout, owner(), p01_owner::MAILBOX);
    assert!(p01_owner::copy_entry(0).is_none());
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::INCOMPLETE);
    TEST_HEAP_USED.store(16, SeqCst);
    assert!(p01_owner::capture_stage(2, &guard).is_none());
    assert!(p01_owner::copy_stage_source(2).is_none());
    assert!(p01_owner::copy_stage_entry(2, 0).is_none());
    p01_owner::reset_for_test(1);
    p01_measurement::FLAGS.store(0, SeqCst);
    TEST_HEAP_USED.store(0, SeqCst);
    p01_owner::arm();
    record_charged(0x1000 as *mut u8, layout, None, usize::MAX);
    record_charged(0x2000 as *mut u8, layout, None, 1);
    TEST_HEAP_USED.store(u64::MAX, SeqCst);
    assert!(p01_owner::capture_stage(2, &guard).is_none());
    assert!(p01_owner::copy_stage_source(2).is_none());
    assert!(p01_owner::copy_stage_charged_entry(2, 0).is_none());
    assert_ne!(p01_measurement::FLAGS.load(SeqCst) & p01_measurement::COUNTER, 0);
    assert!(p01_owner::snapshot(None).invalid);
}

#[test]
fn heap_baseline_and_stages_copy_all_1024_slots_without_promoting_capacity_fit() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(8, 8).unwrap();
    let creation = p01_owner::creation(None, p01_owner::QUOTA_SHARED);
    for index in 0..p01_owner::CAPACITY {
        record_charged((0x1000 + index * 16) as *mut u8, layout, None, 16);
    }
    drop(creation);
    TEST_HEAP_USED.store(16384, SeqCst);
    TEST_HEAP_FREE.store(16384, SeqCst);
    let guard = p01_measurement::scoped_observation().unwrap();
    assert!(p01_owner::capture_baseline(&guard).is_some());
    drop(guard);
    let source = p01_owner::copy_baseline_source().unwrap();
    assert_eq!(&source[4..6], &[1024, 8192]);
    assert_eq!(source[7], 16384);
    assert!(source[11] >= 1024 * 160);
    for index in 0..p01_owner::CAPACITY {
        let entry = p01_owner::copy_baseline_entry(index).unwrap();
        assert_eq!(entry[1], (0x1000 + index * 16) as u64);
        assert_eq!(entry[11], p01_owner::QUOTA_SHARED);
    }
    p01_owner::arm();
    let guard = p01_measurement::observation().unwrap();
    for sample in 2..=6 {
        assert_eq!(p01_owner::capture_stage(sample, &guard), Some((16384, 16384, 1025)));
    }
    drop(guard);
    p01_measurement::ADMITTED.store(false, SeqCst);
    p01_owner::freeze(&p01_measurement::Frozen);
    for sample in 2..=6 {
        let stage_source = p01_owner::copy_stage_source(sample).unwrap();
        assert_eq!(&stage_source[7..14], &[1024, 8192, 16384, 0, 0, 16384, 0]);
        assert_eq!(&stage_source[14..17], &[1024, 1024, 1024]);
        assert_eq!(stage_source[19], p01_owner::static_bytes() as u64);
        for index in 0..p01_owner::CAPACITY {
            let baseline = p01_owner::copy_baseline_entry(index).unwrap();
            let entry = p01_owner::copy_stage_entry(sample, index).unwrap();
            let charged = p01_owner::copy_stage_charged_entry(sample, index).unwrap();
            assert_eq!(&entry, &baseline[1..]);
            assert_eq!(charged, p01_owner::copy_charged_entry(index, true).unwrap());
        }
    }
    let snapshot = p01_owner::snapshot(None);
    assert!(!snapshot.invalid && !snapshot.fit_proven && !snapshot.coverage_proven);
}

#[test]
fn missing_allocation_witness_and_unmatched_charge_fail_closed_without_birth() {
    let _serial = reset();
    let buffer = Vec::<u8>::with_capacity(32);
    p01_owner::runtime_vec(&buffer, owner(), p01_owner::TASK_BACKING);
    assert!(p01_owner::copy_entry(0).is_none());
    assert!(p01_owner::snapshot(None).invalid);
    p01_owner::reset_for_test(1);
    TEST_HEAP_USED.store(16, SeqCst);
    let guard = p01_measurement::observation().unwrap();
    p01_owner::capture_baseline(&guard).unwrap();
    let source = p01_owner::copy_charged_source(true).unwrap();
    assert_eq!((source[7], source[9], source[10], source[20]), (0, 0, 16, 1));
    assert!(p01_owner::snapshot(None).invalid);
}

#[test]
fn census_first_fault_is_bounded_prearm_and_immutable_through_freeze() {
    let _serial = reset();
    assert!(p01_owner::copy_first_fault().is_none());
    p01_measurement::ADMITTED.store(false, SeqCst);
    drop(p01_owner::creation(Some(owner()), p01_owner::COMPLETION_SHARED));
    let first = p01_owner::copy_first_fault().unwrap();
    // The site is the actual rejecting ledger branch, not this host call.
    assert_eq!(first[0..2], [1, p01_measurement::IDENTITY]);
    assert!(first[2] > 0);
    assert_eq!(first[3..], [1, 0, 0]);
    p01_owner::arm();
    drop(p01_owner::creation(None, p01_owner::RECEIVE_SCRATCH));
    assert_eq!(p01_owner::copy_first_fault().unwrap(), first);
    assert!(p01_owner::snapshot(None).invalid);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::IDENTITY);
    p01_owner::freeze(&p01_measurement::Frozen);
    drop(p01_owner::creation(None, p01_owner::RECEIVE_SCRATCH));
    assert_eq!(p01_owner::copy_first_fault().unwrap(), first);
    p01_owner::reset_for_test(1);
    assert!(p01_owner::copy_first_fault().is_none());
}

#[test]
fn prearm_task_backing_uses_full_object_identity_and_rejects_reconstruction() {
    let _serial = reset();
    p01_measurement::ADMITTED.store(false, SeqCst);
    let mut task = Box::new(task::Task {
        id: 30, root_tid: 10, cell_id: 3, cell_generation: 7,
        name: String::new(), cwd: String::new(), allowed_drivers: Vec::new(),
        inherited_argv: None, segment_mem: None,
        staged_argv: None, waiters: Vec::new(),
        pending_deaths: Vec::new(), pending_owner_deaths: Vec::new(),
    });
    let ptr = &*task as *const task::Task as *mut u8;
    let layout = core::alloc::Layout::new::<task::Task>();
    record(ptr, layout);
    p01_owner::task_backing(&task);
    p01_owner::task_backing(&task);
    let original = p01_owner::copy_entry(0).unwrap();
    assert_eq!(&original[11..15], &[30, 10, 3, 7]);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
    task.cell_generation = 8;
    p01_owner::task_backing(&task);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::IDENTITY);
    assert_eq!(p01_owner::copy_entry(0).unwrap(), original);
    assert!(p01_owner::snapshot(None).invalid);
    release(ptr, layout);
}

#[test]
fn retained_source_publication_preserves_birth_origin_and_charged_extent() {
    let _serial = reset();
    p01_measurement::ADMITTED.store(false, SeqCst);
    let entries = Vec::<u64>::with_capacity(4);
    let path = String::from("/bin/config");
    let entries_layout = core::alloc::Layout::array::<u64>(entries.capacity()).unwrap();
    let path_layout = core::alloc::Layout::array::<u8>(path.capacity()).unwrap();
    record_charged(entries.as_ptr() as *mut u8, entries_layout, None, 48);
    record_charged(path.as_ptr() as *mut u8, path_layout, None, 16);
    let origins: Vec<_> = (0..2).map(|index| p01_owner::copy_entry(index).unwrap()).collect();
    let births: Vec<_> = (0..2).map(|index| p01_owner::copy_charged_entry(index, false).unwrap()).collect();
    p01_owner::shared_vec(&entries, p01_owner::SYSTEM_CONFIG_SHARED);
    p01_owner::shared_string(&path, p01_owner::SYSTEM_CONFIG_SHARED);
    for index in 0..2 {
        let joined = p01_owner::copy_entry(index).unwrap();
        assert_eq!(joined[..10], origins[index][..10]);
        assert_eq!(joined[10], p01_owner::SYSTEM_CONFIG_SHARED);
        assert_eq!(joined[11..19], [0; 8]);
        assert_eq!(p01_owner::copy_charged_entry(index, false).unwrap(), births[index]);
    }
    assert_eq!(p01_owner::snapshot(None).unknown_objects, 0);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
    release(entries.as_ptr() as *mut u8, entries_layout);
    release(path.as_ptr() as *mut u8, path_layout);
}

#[test]
fn new_source_kinds_require_shared_or_exact_exclusive_creation_identity() {
    let _serial = reset();
    let layout = core::alloc::Layout::from_size_align(16, 8).unwrap();
    let shared = [p01_owner::BOOT_METADATA_SHARED, p01_owner::SYSTEM_CONFIG_SHARED,
        p01_owner::FILESYSTEM_SCRATCH_SHARED, p01_owner::FUTEX_SHARED,
        p01_owner::GRANT_SHARED, p01_owner::DOMAIN_SHARED, p01_owner::TEST_FIXTURE_SHARED,
        p01_owner::PIPE_SHARED];
    let exclusive = [p01_owner::DOMAIN_BACKING, p01_owner::TASK_FIELDS, p01_owner::TASK_IO];
    for (index, kind) in shared.into_iter().chain(exclusive).enumerate() {
        let is_shared = index < shared.len();
        let witness = p01_owner::creation(if is_shared { None } else { Some(owner()) }, kind);
        let ptr = (0x1000 + index * 32) as *mut u8;
        record(ptr, layout);
        drop(witness);
        let entry = p01_owner::copy_entry(index).unwrap();
        assert_eq!(entry[10], kind);
        assert_eq!(entry[11..15], if is_shared { [0; 4] } else { [10, 10, 3, 1] });
        assert_eq!(entry[15..19], [0; 4]);
    }
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
    drop(p01_owner::creation(Some(owner()), p01_owner::BOOT_METADATA_SHARED));
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), p01_measurement::IDENTITY);
}

static TEST_TOPOLOGY: Spinlock<p01_topology::Snapshot> =
    Spinlock::new(p01_topology::Snapshot::new());
fn topology_fixture() -> SpinlockGuard<'static, p01_topology::Snapshot> {
    let mut snapshot = TEST_TOPOLOGY.lock();
    snapshot.source = [1, 1, 10, 8, 1, 0, 0, 0, 64, 1024, 16384, 4, 0, 7, 0, 0];
    snapshot.roots.fill([0; 8]);
    snapshot.roots[0] = [0x1000, 0, 0, 0, 0, 0, 0, 1];
    snapshot
}

#[test]
fn topology_current_graph_deduplicates_shared_external_tables_and_keeps_leaf_aliases() {
    let _serial = reset();
    let mut snapshot = topology_fixture();
    snapshot.roots[1] = [0x2000, 11, 10, 3, 1, 20, 21, 2];
    snapshot.roots[2] = [0x2000, 12, 10, 3, 1, 20, 21, 2];
    p01_topology::capture_graph(&mut snapshot, 3, |pa, index| Some(match (pa, index) {
        (0x1000, 256) | (0x2000, 256) => 0x3001,
        (0x3000, 0) => 0x4001,
        (0x4000, 0) => 0x5001,
        (0x5000, 0) | (0x5000, 1) => 0x8001,
        _ => 0,
    }), |pa, _| if pa == 0x3000 { 2 } else { 1 }, |pa| Some(pa / 4096 + 1));
    assert_eq!(snapshot.source[11], 1);
    assert_eq!(&snapshot.source[5..8], &[3, 5, 6]);
    assert_eq!(&snapshot.source[14..16], &[5, 6]);
    let shared = snapshot.nodes[..5].iter().find(|node| node[0] == 0x3000).unwrap();
    assert_eq!(*shared, [0x3000, 3, 0, 2, 2]);
    assert_eq!(snapshot.nodes.iter().filter(|node| node[0] == 0x2000).count(), 1);
    let leaves: Vec<_> = snapshot.edges[..6].iter().filter(|edge| edge[4] == 4096).collect();
    assert_eq!(leaves.len(), 2);
    assert_eq!(leaves[0][3], leaves[1][3]);
}

#[test]
fn topology_huge_geometry_never_dereferences_leaves_and_pt_pat_is_not_ps() {
    let _serial = reset();
    let mut snapshot = topology_fixture();
    p01_topology::capture_graph(&mut snapshot, 1, |pa, index| Some(match (pa, index) {
        (0x1000, 0) => 0x2001,
        (0x2000, 0) => 0x4000_1081, // 1 GiB leaf with PAT.
        (0x2000, 1) => 0x3001,
        (0x3000, 0) => 0x20_1081, // 2 MiB leaf with PAT.
        (0x3000, 1) => 0x4001,
        (0x4000, 0) => 0x8081, // PT PAT, ordinary 4 KiB leaf.
        _ => 0,
    }), |_, bytes| if bytes > 4096 { 2 } else { 1 }, |_| Some(1));
    assert_eq!(snapshot.source[11], 1);
    assert_eq!(&snapshot.source[6..8], &[4, 6]);
    assert!(snapshot.edges[..6].contains(&[0x2000, 0, 0x4000_1081, 0x4000_0000, 1073741824, 2, 1]));
    assert!(snapshot.edges[..6].contains(&[0x3000, 0, 0x20_1081, 0x20_0000, 2097152, 2, 1]));
    assert!(snapshot.edges[..6].contains(&[0x4000, 0, 0x8081, 0x8000, 4096, 1, 1]));
    assert!(p01_topology::descriptor(3, 0x4000_2081).is_err());
    assert!(p01_topology::descriptor(2, 0x20_2081).is_err());
    assert!(p01_topology::descriptor(4, 0x2081).is_err());
    assert_eq!(p01_topology::descriptor(1, 0x8081), Ok(Some((0x8000, 4096))));
}

#[test]
fn topology_rejects_cycles_level_conflicts_free_tables_and_missing_history() {
    let _serial = reset();
    for failure in 0..4 {
        let mut snapshot = topology_fixture();
        p01_topology::capture_graph(&mut snapshot, 1, |pa, index| Some(match (pa, index, failure) {
            (0x1000, 0, 0) => 0x1001, // root reachable at an incompatible level.
            (0x1000, 0, _) => 0x2001,
            (0x2000, 0, 1) => 0x3001,
            (0x2000, 1, 1) => 0x4001,
            (0x4000, 0, 1) => 0x3001, // same physical table at levels 2 and 1.
            _ => 0,
        }), |pa, _| if failure == 2 && pa == 0x2000 { 3 } else { 1 },
            |pa| if failure == 3 && pa == 0x2000 { None } else { Some(1) });
        assert_eq!(snapshot.source[11], 4);
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
        assert_eq!(snapshot.source[12], if failure == 3 { 16 } else { 8 });
    }
}

#[test]
fn topology_count_fill_rejects_source_changes_including_absent_descriptors() {
    let _serial = reset();
    for changed in [false, true] {
        let mut snapshot = topology_fixture();
        let mut reads = 0usize;
        p01_topology::capture_graph(&mut snapshot, 1, |_, index| {
            reads += 1;
            // Empty root preflight uses 512 reads; introduce a new table only
            // in the fill pass, or make an absent descriptor unreadable.
            if reads > 512 && index == 0 {
                if changed { Some(0x2001) } else { None }
            } else { Some(0) }
        }, |_, _| 1, |_| Some(1));
        assert_eq!(snapshot.source[11], 4);
        assert_eq!(snapshot.source[12], 8);
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
    }
}

#[test]
fn topology_hardware_accessed_dirty_changes_do_not_change_mapping_references() {
    let _serial = reset();
    let mut snapshot = topology_fixture();
    let mut reads = 0usize;
    p01_topology::capture_graph(&mut snapshot, 1, |pa, index| {
        reads += 1;
        let flags = if reads > 1024 { 0x60 } else { 0 };
        Some(match (pa, index) {
            (0x1000, 0) => 0x2001 | flags,
            (0x2000, 0) => 0x4000_0081 | flags,
            _ => 0,
        })
    }, |_, _| 1, |_| Some(1));
    assert_eq!(snapshot.source[11], 1);
    assert_eq!(snapshot.source[6..8], [2, 2]);
    assert_eq!(snapshot.edges[1][2], 0x4000_0081);
}

#[test]
fn topology_leaf_runs_preserve_every_address_permission_and_table_boundary() {
    let _serial = reset();
    let mut snapshot = topology_fixture();
    p01_topology::capture_graph(&mut snapshot, 1, |pa, index| Some(match pa {
        0x1000 => if index == 0 { 0x2001 } else { 0 },
        0x2000 => if index == 0 { 0x3001 } else { 0 },
        0x3000 => if index == 0 { 0x4001 } else { 0 },
        0x4000 => if index < 256 { 0x100001 + index as u64 * 4096 }
            else { 0x400003 + index as u64 * 4096 },
        _ => 0,
    }), |_, _| 2, |_| None);
    assert_eq!(snapshot.source[11], 1);
    for index in 0..512u64 {
        let run = snapshot.edges[..snapshot.source[7] as usize].iter()
            .find(|row| row[0] == 0x4000 && row[1] <= index && index < row[1] + row[6]).unwrap();
        let expected = if index < 256 { 0x100001 + index * 4096 } else { 0x400003 + index * 4096 };
        assert_eq!(run[2] + (index - run[1]) * 4096, expected);
        assert!(run[1] + run[6] <= 512);
    }
}

#[test]
fn topology_leaf_run_fill_rejects_a_changed_middle_descriptor() {
    let _serial = reset();
    let mut snapshot = topology_fixture();
    let mut pass = 0;
    p01_topology::capture_graph(&mut snapshot, 1, |pa, index| {
        if pa == 0x4000 && index == 0 { pass += 1; }
        Some(match pa {
            0x1000 => if index == 0 { 0x2001 } else { 0 },
            0x2000 => if index == 0 { 0x3001 } else { 0 },
            0x3000 => if index == 0 { 0x4001 } else { 0 },
            0x4000 => 0x100001 + index as u64 * 4096
                + if pass == 2 && index == 257 { 4096 } else { 0 },
            _ => 0,
        })
    }, |_, _| 2, |_| None);
    assert_eq!(snapshot.source[12], 8);
    assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
}

#[test]
fn topology_preflight_refuses_node_edge_and_root_capacity_without_partial_publication() {
    let _serial = reset();
    {
        let mut snapshot = topology_fixture();
        p01_topology::capture_graph(&mut snapshot, 65, |_, _| panic!("root capacity must preflight"),
            |_, _| 1, |_| Some(1));
        assert_eq!(snapshot.source[12], 4);
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
    }
    {
        let mut snapshot = topology_fixture();
        p01_topology::capture_graph(&mut snapshot, 1, |pa, index| Some(match pa {
            0x1000 => if index < 2 { (index as u64 + 2) * 4096 | 1 } else { 0 },
            0x2000 => (index as u64 + 4) * 4096 | 1,
            0x3000 => (index as u64 + 516) * 4096 | 1,
            _ => 0,
        }), |_, _| 1, |_| Some(1));
        assert_eq!(snapshot.source[12], 4);
        assert_eq!(snapshot.source[14], 1025); // lower bound on aborted count.
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
    }
    {
        let mut snapshot = topology_fixture();
        p01_topology::capture_graph(&mut snapshot, 1, |pa, index| Some(match pa {
            0x1000 => if index == 0 { 0x2001 } else { 0 },
            0x2000 => if index < 33 { (index as u64 + 3) * 4096 | 1 } else { 0 },
            _ => 0x81, // 33*512 distinct current 2 MiB leaf descriptors.
        }), |_, _| 2, |_| panic!("external nodes have no manufactured incarnation"));
        assert_eq!(snapshot.source[12], 4);
        assert_eq!(snapshot.source[14], 35);
        assert_eq!(snapshot.source[15], 1 + 33 + 33 * 512); // completed exact count.
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
    }
}

#[test]
fn topology_history_join_preserves_unknown_birth_and_exact_reuse_cutoff() {
    let _serial = reset();
    p01_retained::physical_preexisting(0x1000, 3);
    assert_eq!(p01_retained::physical_incarnation_at(0x1000, 2), None);
    let census = p01_retained::physical_incarnation_at(0x1000, 3).unwrap();
    assert_eq!(p01_retained::copy_physical_entry(0).unwrap()[2], 0);
    let write = p01_retained::physical_write().unwrap();
    p01_retained::physical_freed(&write, 0x1000, 4);
    p01_retained::physical_allocated(&write, 0x1000, 5);
    drop(write);
    assert_eq!(p01_retained::physical_incarnation_at(0x1000, 3), Some(census));
    assert_eq!(p01_retained::physical_incarnation_at(0x1000, 4), None);
    let reused = p01_retained::physical_incarnation_at(0x1000, 5).unwrap();
    assert_ne!(census, reused);
    let mut snapshot = topology_fixture();
    p01_topology::capture_graph(&mut snapshot, 1, |_, _| Some(0), |_, _| 1,
        |pa| p01_retained::physical_incarnation_at(pa, 3));
    assert_eq!(snapshot.source[11], 1);
    assert_eq!(snapshot.nodes[0][2], census);
}
