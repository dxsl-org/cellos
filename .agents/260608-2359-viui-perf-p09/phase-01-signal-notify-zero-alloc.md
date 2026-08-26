# Phase 01 — Signal Notify Zero-Alloc

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P1 — highest impact; called on every signal.set()

---

## Problem

`signal.rs:105`:
```rust
let subs: Vec<Rc<dyn Fn()>> = self.inner.subs.borrow().clone();
```
Every `Signal::set()` or `update()` clones the entire subscriber Vec into a new
heap allocation — even for signals with 1 subscriber. In a typical counter UI:
- Button click → `count.update()` → notify → 1 Vec alloc
- `count.map(|n| format!(...))` subscriber fires → `Computed` signal notifies →
  1 more Vec alloc
Total: 2+ Vec allocs per user interaction.

---

## Solution

Replace Vec clone with per-element `Rc::clone()` using index-based iteration.
`Rc::clone()` = 1 read + 1 write (ref-count increment). No heap allocation.

**Why it is safe**: The clone was needed to release the `subs` RefCell borrow
before calling callbacks (re-entrancy guard). The new approach borrows, clones
one `Rc`, then drops the borrow — same safety property, no Vec alloc.

**Re-entrancy**: `notifying` Cell still guards against recursive `set()` on the
same signal. If a callback calls `subscribe()`, the new entry is appended past
`len` and not called this round — correct.

---

## Requirements

- `signal.rs::SignalInner::notify()` modified only
- Zero new allocations when `notify()` fires
- All 4 existing signal tests still pass (`cargo test -p viui`)
- Behaviour identical: same subscribers called in same order; dead subscribers pruned

---

## Architecture

### `libs/viui/src/signal.rs` — replace `notify()`

**Current** (`signal.rs:100-113`):
```rust
fn notify(&self) {
    if self.inner.notifying.get() { return; }
    self.inner.notifying.set(true);
    let subs: Vec<Rc<dyn Fn()>> = self.inner.subs.borrow().clone(); // ← ALLOC
    for sub in &subs {
        if Rc::strong_count(sub) > 1 { sub(); }
    }
    self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
    self.inner.notifying.set(false);
}
```

**After**:
```rust
fn notify(&self) {
    if self.inner.notifying.get() { return; }
    self.inner.notifying.set(true);
    let len = self.inner.subs.borrow().len();
    for i in 0..len {
        // Borrow → Rc::clone (cheap ref-count bump) → drop borrow → call
        let sub = self.inner.subs.borrow().get(i).cloned();
        if let Some(rc) = sub {
            if Rc::strong_count(&rc) > 1 { rc(); }
        }
    }
    self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
    self.inner.notifying.set(false);
}
```

**Key**: `subs.borrow().get(i).cloned()` → borrows, calls `Rc::clone()` on
the i-th element (2 integer ops), drops the borrow. No Vec::new() anywhere.

---

## Edge cases

- **Subscriber added during notify**: appended at index ≥ `len` → not called this
  round (captured `len` at start). Correct — same as current behaviour.
- **Subscriber dropped during notify**: strong_count falls to 1 → `if Rc::strong_count(&rc) > 1` → skipped. Correct.
- **Empty subscriber list** (`len == 0`): loop body never executes. No alloc. Correct.
- **Re-entrant set() on same signal**: `notifying` guard returns early. Correct.
- **Re-entrant set() on different signal**: allowed — only `notifying` for `self` is set.

---

## Related Code Files

**Modify:**
- `libs/viui/src/signal.rs` — `fn notify()` body only (lines 100-113)

---

## Implementation Steps

1. Replace `notify()` body in `signal.rs` as shown above
2. `cargo test -p viui --target x86_64-pc-windows-msvc` — all 4 signal tests pass
3. `cargo check -p viui` clean

---

## Todo List

- [ ] Replace notify() body
- [ ] cargo test signal tests pass
- [ ] cargo check clean

---

## Success Criteria

- Zero `Vec::new()` or `Vec::clone()` calls during `notify()` (verifiable by reading code)
- All 4 existing tests in `signal.rs::tests` pass unchanged
- `SubscriptionHandle` drop-unsubscribe behaviour preserved

---

## Risk

- **Concurrent modification during iteration**: RefCell panics if borrow is re-entered.
  The new code borrows exactly once per iteration, drops immediately → no overlap. Safe.
- **Test coverage**: existing tests cover set/drop/map/reentrant. No new edge cases
  introduced by this change; all paths tested.
