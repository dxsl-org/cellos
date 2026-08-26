# Phase 03 — Signal Conditional Retain

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P3 — eliminates O(n) Vec scan on every notify() in steady state

---

## Problem

`signal.rs` `notify()` always calls `retain()` at the end:
```rust
self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
```

`retain()` is O(n) — iterates every subscriber to check if its handle is alive.
In a typical embedded ViUI with 10 signals, each with 1-2 subscribers:
- Every `signal.set()` → `notify()` → `retain()` over n subscribers
- At 60Hz, a counter signal updating every frame → 60 × n `retain()` iterations

In steady state (handles created at startup, never dropped), `retain()` NEVER
removes anything — it's O(n) work that finds nothing to remove every time.

---

## Solution

Track whether any subscriber was found dead during the notify loop. Only call
`retain()` when we know at least one handle was dropped. In steady state (all
handles alive), `retain()` is skipped entirely — zero O(n) scan per notify.

**Key insight**: we can detect dead entries by checking `Rc::strong_count(rc)`
BEFORE cloning. If count == 1, only the subs Vec holds a reference → handle was
dropped. Mark `any_dead = true`. After the loop, conditionally `retain()`.

---

## Requirements

- `notify()` semantics preserved: live subscribers called; dead entries pruned
  (possibly one notify after drop — this is the existing behavior)
- `any_dead` is a plain `bool` local — no extra heap allocation, no struct changes
- No changes to `SubscriptionHandle` or public API
- All 5 existing signal tests pass (verifiable by re-running cargo test on host
  if available, or by inspection)
- `cargo check -p viui` clean

---

## Architecture

### `libs/viui/src/signal.rs` — replace `notify()`

**Current (after P09):**
```rust
fn notify(&self) {
    if self.inner.notifying.get() { return; }
    self.inner.notifying.set(true);
    let len = self.inner.subs.borrow().len();
    for i in 0..len {
        let sub = self.inner.subs.borrow().get(i).cloned();
        if let Some(rc) = sub {
            if Rc::strong_count(&rc) > 1 { rc(); }
        }
    }
    self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
    self.inner.notifying.set(false);
}
```

**After:**
```rust
fn notify(&self) {
    if self.inner.notifying.get() { return; }
    self.inner.notifying.set(true);
    let len = self.inner.subs.borrow().len();
    let mut any_dead = false;
    for i in 0..len {
        // Check liveness before cloning — count unmodified by our own borrow here
        let is_live = self.inner.subs.borrow()
            .get(i)
            .map(|rc| Rc::strong_count(rc) > 1)
            .unwrap_or(false);
        if !is_live {
            any_dead = true;
            continue;
        }
        let sub = self.inner.subs.borrow().get(i).cloned();
        if let Some(rc) = sub { rc(); }
    }
    // Prune dead entries only when we know at least one handle was dropped.
    // In steady state (all handles alive), this is never called — zero cost.
    if any_dead {
        self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
    }
    self.inner.notifying.set(false);
}
```

**Correctness trace:**

*Case A — handle alive:*
- Before clone: `strong_count = 2` (subs + handle) → `is_live = true` ✓
- Clone: `strong_count = 3` momentarily
- `rc()` called; `rc` drops → count back to 2 ✓

*Case B — handle dropped, first notify after:*
- Before clone: `strong_count = 1` (only subs) → `is_live = false`
- `any_dead = true`; subscriber NOT called (live semantics: dropped handle skips one
  notify earlier than before, but dead subscribers calling once more is a don't-care)
- After loop: `retain()` runs → removes the dead entry ✓

*Case C — empty subs:*
- `len = 0` → loop doesn't run → `any_dead = false` → `retain()` skipped ✓

*Case D — re-entrant notify on same signal:*
- `notifying.get()` returns `true` → early return ✓ (unchanged)

**Behavior change vs P09**: In P09, a dead subscriber was called ONE MORE TIME
after its handle dropped (the clone bumped count to 2, fooling the check). In P10a,
dead subscribers are NOT called after handle drop — they're detected immediately
via the pre-clone liveness check. This is strictly more correct behavior.

**Extra borrow per iteration for live subscribers**: the liveness check is one
extra `RefCell::borrow()` before the clone borrow. `RefCell::borrow()` on a
single-threaded non-contended cell = read Cell<usize>, increment, return ref.
~2-3 cycles. Amortized over the callback execution cost, negligible.

---

## Related Code Files

**Modify:**
- `libs/viui/src/signal.rs` — `fn notify()` body only

---

## Implementation Steps

1. Replace `notify()` body in `signal.rs` as shown above
2. `cargo check -p viui` — no errors
3. Code review: verify `any_dead` tracking is correct for all 4 cases above

---

## Todo List

- [ ] Replace notify() with any_dead conditional retain
- [ ] cargo check -p viui passes
- [ ] Verify 4 correctness cases by inspection

---

## Success Criteria

- `retain()` not called when all handles alive (verifiable: `any_dead` stays false)
- Dead handles removed on the notify after drop (retain fires once when needed)
- `cargo check -p viui` passes

---

## Risk

- **Behavior change — dead subscriber no longer called**: in P09, a dropped handle's
  subscriber was still called once more on the next notify (because the clone bumped
  count to 2). In P10a, it is NOT called (detected via pre-clone count check).
  This is MORE correct behavior — a dropped handle should not fire. The existing
  tests do not test this edge case (they only test that dropped handle stops firing
  after prune). No regression expected.
- **`any_dead` false when it should be true**: can happen if strong_count somehow
  returns > 1 for a dead handle. On single-threaded `Rc`, strong_count is
  authoritative — if count == 1 and we hold no clone, handle was dropped. No false
  negatives possible.
- **struct change needed**: `any_dead` is a function-local `bool` — NO struct changes.
  No `SignalInner` fields added. No `SubscriptionHandle` changes.
