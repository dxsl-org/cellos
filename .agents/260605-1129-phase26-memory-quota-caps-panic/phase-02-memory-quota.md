# Phase 02 — Per-Cell Memory Quota

**Status**: 📋 PLANNED  
**Priority**: P0  
**Effort**: 4 days  
**Depends on**: Phase 01 (CellId accessible in task module)

---

## Context Links

- Global allocator: `kernel/src/memory/heap.rs:1-27`
- Cell spawn: `kernel/src/loader.rs:44-82`
- Task exit cleanup: `kernel/src/task/syscall.rs:599-650`
- Scheduler context switch: `kernel/src/task/scheduler.rs:pick_next()`
- Spec quota requirement: `docs/specs/02-memory.md:21-24`

---

## Overview

The current global allocator (`linked_list_allocator::LockedHeap`) has no per-cell accounting. A runaway Cell can exhaust the shared 32 MB kernel heap, causing system-wide OOM that halts everything.

The fix: wrap `LockedHeap` in `QuotaAlloc` — a `GlobalAlloc` implementation that:
1. Reads a `CURRENT_CELL_ID: AtomicUsize` set by the scheduler on every context switch
2. Charges the allocation to the Cell's quota entry in a `BTreeMap<CellId, CellQuota>`
3. Returns `null_mut()` (not `panic!()`) when the quota is exceeded
4. Decrements the counter on dealloc

The Cell's code then gets an `AllocError`, propagates it as a panic (illegal instruction in abort mode), and the trap handler kills the Cell.

---

## Key Insight: `CURRENT_CELL_ID` is the Attribution Primitive

The kernel cannot intercept `Box::new()` / `Vec::push()` at the per-cell level without this global. Setting it on every `pick_next()` context switch means the allocator always knows which Cell is currently executing.

**Important limitation**: allocations made by the kernel itself (cell_id = 0) are unlimited. Allocations made while processing a Cell's syscall are attributed to that Cell (correct: the Cell triggered the kernel work).

---

## Related Code Files

### Create
- `kernel/src/memory/cell_quota.rs` — quota table + `CellQuota` struct

### Modify
- `kernel/src/memory/heap.rs` — replace `LockedHeap` with `QuotaAlloc` wrapper
- `kernel/src/task/scheduler.rs` — add `CURRENT_CELL_ID` atomic, set in `pick_next()`
- `kernel/src/loader.rs` — register quota at cell spawn
- `kernel/src/task/syscall.rs:Exit` — deregister quota on cell exit

---

## Implementation Steps

### Step 1 — `CURRENT_CELL_ID` in scheduler

```rust
// kernel/src/task/scheduler.rs
use core::sync::atomic::{AtomicUsize, Ordering};

/// Cell ID currently running on this hart.  0 = kernel itself (no quota).
/// Set on every context switch by pick_next().
pub static CURRENT_CELL_ID: AtomicUsize = AtomicUsize::new(0);

pub fn current_cell_id() -> usize {
    CURRENT_CELL_ID.load(Ordering::Relaxed)
}
```

In `pick_next()`, after selecting the new task:
```rust
// Set current cell ID for the allocator's per-cell accounting.
if let Some(task) = self.tasks.get(&nid) {
    CURRENT_CELL_ID.store(task.cell_id.0 as usize, Ordering::Relaxed);
}
```

### Step 2 — Create `kernel/src/memory/cell_quota.rs`

**⚠️ Red-team fix**: Using `BTreeMap<CellId, CellQuota>` inside `QuotaAlloc::alloc()` causes a deadlock: `BTreeMap::insert()` triggers `GlobalAlloc::alloc()` → re-enters `charge()` → tries to lock `QUOTA_TABLE` → deadlock.

**Solution (user confirmed)**: Split the quota into two stores:
- `QUOTA_TABLE: Spinlock<BTreeMap<usize, usize>>` — stores only the **limit** per cell (set once at spawn, read-only after). BTreeMap::insert() in `register()` allocates while NOT holding the QuotaAlloc lock — no deadlock.
- `IN_USE: [AtomicUsize; MAX_CELLS]` — stores the live byte counter per cell. Updated in `charge()`/`refund()` with atomics only — no lock, no allocation, no deadlock.

This keeps BTreeMap (unbounded cell count) while eliminating the allocation-inside-lock hazard.

```rust
//! Per-cell heap quota.
//!
//! BTreeMap stores limits (set once at spawn, never inside alloc()).
//! AtomicUsize array stores in_use (updated lock-free inside alloc/dealloc).
//! This split eliminates the allocator-inside-allocator deadlock.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::CellId;

pub const MAX_CELLS: usize = 64;
pub const DEFAULT_QUOTA_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

/// Limit store — BTreeMap (unbounded), locked only in register/deregister.
/// BTreeMap::insert() allocates, but this never runs inside QuotaAlloc::alloc().
static QUOTA_LIMITS: Spinlock<BTreeMap<usize, usize>> = Spinlock::new(BTreeMap::new());

/// Live byte counters — AtomicUsize per cell, no lock needed.
const ZERO: AtomicUsize = AtomicUsize::new(0);
static IN_USE: [AtomicUsize; MAX_CELLS] = [ZERO; MAX_CELLS];

/// Register a new Cell.  Safe to call from spawn context (not inside alloc).
pub fn register(cell_id: CellId, limit: usize) {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS { IN_USE[id].store(0, Ordering::Relaxed); }
    QUOTA_LIMITS.lock().insert(id, limit); // BTreeMap alloc — outside alloc() context
}

/// Deregister on Cell exit.
pub fn deregister(cell_id: CellId) {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS { IN_USE[id].store(0, Ordering::Relaxed); }
    QUOTA_LIMITS.lock().remove(&id);
}

/// Charge `size` bytes.  Lock-free for in_use; brief lock for limit read.
pub fn charge(cell_id_raw: usize, size: usize) -> bool {
    if cell_id_raw == 0 { return true; }
    // Read limit — BTreeMap::get does NOT allocate; lock released immediately.
    let limit = QUOTA_LIMITS.lock()
        .get(&cell_id_raw).copied().unwrap_or(usize::MAX);
    if cell_id_raw >= MAX_CELLS { return true; }
    // Atomic CAS-style: add optimistically, rollback on breach.
    let prev = IN_USE[cell_id_raw].fetch_add(size, Ordering::Relaxed);
    if prev + size > limit {
        IN_USE[cell_id_raw].fetch_sub(size, Ordering::Relaxed);
        false
    } else {
        true
    }
}

/// Refund `size` bytes on dealloc — lock-free.
pub fn refund(cell_id_raw: usize, size: usize) {
    if cell_id_raw == 0 || cell_id_raw >= MAX_CELLS { return; }
    IN_USE[cell_id_raw].fetch_sub(size, Ordering::Relaxed);
}
```

### Step 3 — `QuotaAlloc` wrapper in `heap.rs`

```rust
use core::alloc::{GlobalAlloc, Layout};
use linked_list_allocator::LockedHeap;
use crate::memory::cell_quota;
use crate::task::scheduler::current_cell_id;

pub struct QuotaAlloc {
    inner: LockedHeap,
}

// SAFETY: QuotaAlloc delegates to LockedHeap which is thread-safe.
unsafe impl GlobalAlloc for QuotaAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cell = current_cell_id();
        if !cell_quota::charge(cell, layout.size()) {
            return core::ptr::null_mut(); // quota exceeded — OOM for this Cell
        }
        let ptr = self.inner.alloc(layout);
        if ptr.is_null() {
            // Inner allocator OOM: refund the charge we already took
            cell_quota::refund(cell, layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        cell_quota::refund(current_cell_id(), layout.size());
        self.inner.dealloc(ptr, layout);
    }
}

/// Initialise the kernel heap.
///
/// # Safety
/// Must be called exactly once after physical memory is mapped.
pub unsafe fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR.inner.lock().init(heap_start as *mut u8, heap_size);
}

#[global_allocator]
static ALLOCATOR: QuotaAlloc = QuotaAlloc { inner: LockedHeap::empty() };
```

### Step 4 — Register quota at spawn (`loader.rs`)

At the end of `spawn_from_path()`, after setting caps:
```rust
// Register memory quota for this Cell.
crate::memory::cell_quota::register(task.cell_id, cell_quota::DEFAULT_QUOTA_BYTES);
```

### Step 5 — Deregister on exit (`syscall.rs:Exit`)

After `sched.exit_task(caller_id)`:
```rust
// Release Cell's quota entry to reclaim the BTreeMap slot.
let cell_id = /* retrieved above */;
crate::memory::cell_quota::deregister(cell_id);
```

---

## Critical Limitation: Cross-Cell Dealloc

If Cell A allocates a `Box<T>` and passes it to Cell B via IPC, the dealloc runs while `CURRENT_CELL_ID == B`. Cell B's quota is decremented, not Cell A's. This is a known flaw in shared-heap per-tenant accounting.

**Mitigation**: Law 2 (owned buffers) requires `Box<[u8]>` transfers between cells, which are consumed by the receiver. The receiver allocates its own copy; the sender's buffer is dropped by the kernel IPC path (current_cell_id = 0 during kernel IPC dispatch → refund is ignored). This makes the bias occur only in kernel-to-kernel transfers, not cell-to-cell.

---

## Todo List

- [ ] Add `CURRENT_CELL_ID: AtomicUsize` to `scheduler.rs`; set in `pick_next()`
- [ ] Add `pub fn current_cell_id()` to `task/scheduler.rs` (pub for allocator)
- [ ] Create `kernel/src/memory/cell_quota.rs` (quota table, charge/refund)
- [ ] Add `pub mod cell_quota;` to `kernel/src/memory.rs`
- [ ] Replace `LockedHeap #[global_allocator]` with `QuotaAlloc` in `heap.rs`
- [ ] Call `cell_quota::register(cell_id, DEFAULT_QUOTA_BYTES)` in `loader.rs`
- [ ] Call `cell_quota::deregister(cell_id)` in `syscall.rs:Exit`
- [ ] `cargo check -p vicell-kernel` — zero errors
- [ ] Verify: Cell that over-allocates gets terminated; kernel continues

---

## Success Criteria

- [ ] A test Cell that calls `Vec::with_capacity(5 * 1024 * 1024)` is terminated (> 4 MiB quota)
- [ ] After termination, shell prompt returns — kernel alive
- [ ] Normal Cells (shell, vfs, net) operate normally within 4 MiB each
- [ ] RT heap (Phase 25 `rt_heap.rs`) is unaffected — it bypasses `QuotaAlloc`
- [ ] All 65 existing integration tests pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Spinlock in hot alloc path causes contention | Medium | Single-hart QEMU: no actual contention. BTreeMap lookup = O(log n), n ≤ ~10 cells |
| `layout.size()` under-counts fragmentation overhead | Low | Quota based on requested bytes, not actual. Acceptable conservatism. |
| Kernel init allocations before `register()` called | Medium | `CURRENT_CELL_ID == 0` during boot → charge() returns true (no limit) |
| QuotaAlloc `inner` not yet initialized when first alloc fires | Low | `heap::init()` must be called before any Box/Vec — already enforced in main.rs |
