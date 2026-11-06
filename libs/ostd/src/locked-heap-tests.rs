//! Host tests for the cell heap lock (audit H-31): threads of one cell share the
//! free list, so concurrent alloc/free must never hand the same bytes to two
//! owners, and a waiter must yield rather than spin out a preempted holder.

use super::CellHeap;
use core::alloc::Layout;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

const ARENA_BYTES: usize = 1 << 20;
const THREADS: usize = 8;
const ROUNDS: usize = 20_000;
const LIVE_PER_THREAD: usize = 8;

fn host_yield() {
    thread::yield_now();
}

/// A heap plus the host arena it carves up. The boxed arena never moves when the
/// fixture does, and it is dropped only with the heap that points into it.
struct HostHeap {
    heap: CellHeap,
    _arena: Box<[u128]>,
}

impl std::ops::Deref for HostHeap {
    type Target = CellHeap;
    fn deref(&self) -> &CellHeap {
        &self.heap
    }
}

fn host_heap(bytes: usize) -> HostHeap {
    let mut arena = vec![0u128; bytes / 16].into_boxed_slice();
    let heap = CellHeap::empty();
    // SAFETY: the 16-byte aligned arena lives in the fixture beside `heap` and is
    // reached only through it.
    unsafe { heap.init(arena.as_mut_ptr().cast(), bytes, host_yield) };
    HostHeap {
        heap,
        _arena: arena,
    }
}

#[test]
fn concurrent_alloc_free_never_hands_out_overlapping_blocks() {
    let heap = Arc::new(host_heap(ARENA_BYTES));
    let workers: Vec<_> = (0..THREADS)
        .map(|t| {
            let heap = Arc::clone(&heap);
            thread::spawn(move || {
                let tag = t as u8 + 1;
                let mut live: Vec<(*mut u8, Layout)> = Vec::with_capacity(LIVE_PER_THREAD);
                for round in 0..ROUNDS {
                    let layout = Layout::from_size_align(16 + (round * 37 + t * 11) % 240, 8)
                        .expect("valid layout");
                    let no_arena = || panic!("heap was initialised up front");
                    // SAFETY: the block is freed below with this same layout.
                    let ptr = unsafe { heap.alloc(layout, host_yield, no_arena) };
                    assert!(!ptr.is_null(), "arena exhausted: free list lost blocks");
                    // SAFETY: `ptr` is a fresh block of `layout.size()` bytes.
                    unsafe { ptr.write_bytes(tag, layout.size()) };
                    live.push((ptr, layout));
                    if live.len() == LIVE_PER_THREAD {
                        for (ptr, layout) in live.drain(..) {
                            // SAFETY: still owned by this thread; size as allocated.
                            let bytes = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
                            assert!(
                                bytes.iter().all(|&b| b == tag),
                                "another thread wrote into this thread's block"
                            );
                            // SAFETY: allocated above with `layout`, freed once.
                            unsafe { heap.dealloc(ptr, layout, host_yield) };
                        }
                    }
                }
                for (ptr, layout) in live {
                    // SAFETY: as above.
                    unsafe { heap.dealloc(ptr, layout, host_yield) };
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker observed heap corruption");
    }
    assert_eq!(
        heap.used(host_yield),
        0,
        "every block was returned exactly once"
    );
}

static RELAX_CALLS: AtomicUsize = AtomicUsize::new(0);

fn counting_yield() {
    RELAX_CALLS.fetch_add(1, Ordering::Relaxed);
    thread::yield_now();
}

#[test]
fn a_waiter_yields_to_the_holder_and_then_acquires() {
    let heap = Arc::new(host_heap(4096));
    let guard = heap.lock(host_yield);
    let done = Arc::new(AtomicBool::new(false));
    let waiter = {
        let heap = Arc::clone(&heap);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            let layout = Layout::from_size_align(64, 8).expect("valid layout");
            // SAFETY: freed immediately with the same layout.
            unsafe {
                let ptr = heap.alloc(layout, counting_yield, || unreachable!());
                assert!(!ptr.is_null());
                heap.dealloc(ptr, layout, counting_yield);
            }
            done.store(true, Ordering::Release);
        })
    };
    // The holder stays in its critical section until the waiter has yielded.
    while RELAX_CALLS.load(Ordering::Relaxed) == 0 {
        thread::yield_now();
    }
    assert!(!done.load(Ordering::Acquire), "waiter entered a held heap");
    drop(guard);
    waiter.join().expect("waiter acquired after release");
    assert!(done.load(Ordering::Acquire));
}

#[test]
fn first_allocation_initialises_the_default_arena_once() {
    let mut arena = vec![0u128; 4096 / 16].into_boxed_slice();
    let base = arena.as_mut_ptr().cast::<u8>();
    let heap = CellHeap::empty();
    let mut inits = 0;
    let layout = Layout::from_size_align(32, 8).expect("valid layout");
    // SAFETY: `arena` outlives `heap` (declared first, dropped last) and is
    // reached only through it.
    let first = unsafe {
        heap.alloc(layout, host_yield, || {
            inits += 1;
            (base, 4096)
        })
    };
    // SAFETY: as above; a second init would reset the free list under `first`.
    let second = unsafe { heap.alloc(layout, host_yield, || panic!("arena re-initialised")) };
    assert_eq!(inits, 1);
    assert!(first >= base && second >= base && first != second);
}
