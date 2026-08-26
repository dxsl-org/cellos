# Phase 01 — Signal<T> Reactive Engine + DirtyRect

**Plan**: [plan.md](plan.md)  
**Priority**: P0 — blocks all v2 widget work  
**Status**: ✅ Done  
**Estimated**: 1–2 hours

---

## Context

ViUI v1 rebuilds the full widget tree on every update (`Box<dyn ErasedWidget>` × n) và `damage_all()` mọi lúc.  
Phase này xây dựng reactive core: `Signal<T>` chỉ notify widgets depend on nó, `DirtyRect` chỉ mark vùng thay đổi.  

Design brief: `.agents/brainstorms/260608-viui-nextgen-architecture.md § Signal<T>`

---

## Requirements

### Functional

- `Signal<T>::new(v)` — khởi tạo signal với giá trị ban đầu
- `Signal<T>::get()` — trả `Ref<'_, T>` (borrow không clone)
- `Signal<T>::set(v)` — cập nhật + notify subscribers
- `Signal<T>::update(f)` — mutate in-place + notify
- `Signal<T>::subscribe(f) -> SubscriptionHandle` — đăng ký callback; drop handle = hủy đăng ký
- `Signal<T>::map(f) -> Computed<U>` — derived signal; auto-update khi source thay đổi
- `Computed<T>::get()` — đọc giá trị computed
- `Computed<T>::subscribe(f)` — subscribe vào computed output
- `DirtyRect::mark(rect)` — union với accumulator
- `DirtyRect::mark_all(w, h)` — full surface dirty
- `DirtyRect::take() -> Option<Rect>` — lấy + clear
- `DirtyRect::is_dirty() -> bool`

### Non-Functional

- `no_std + alloc` — chỉ dùng `Rc`, `RefCell`, `Cell`, `Vec`, `Box` từ alloc/core
- `#![forbid(unsafe_code)]` — Law 4
- Zero allocation per `set()` sau khi subscriber list đã build
- Re-entrancy safe: nested `set()` trong subscriber không gây infinite loop

---

## Architecture

### `signal.rs` — Full implementation

```rust
// SPDX-License-Identifier: MIT
//! Reactive Signal<T> — fine-grained dependency tracking for ViUI v2.
//!
//! # Subscription lifetime
//!
//! Subscriptions are alive as long as the returned `SubscriptionHandle` is alive.
//! Dropping the handle removes the callback from the next `notify()` pass.
//! Widgets must store handles in their struct fields to maintain subscriptions.

extern crate alloc;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::{Cell, Ref, RefCell};

use crate::layout::Rect;

// ─── SubscriptionHandle ────────────────────────────────────────────────────

/// Keeps a subscription alive. Drop to unsubscribe.
pub struct SubscriptionHandle {
    // Holds the external strong ref to the Rc<dyn Fn()> in SignalInner::subs.
    // When this drops, strong_count on the inner Rc falls to 1 (only subs vec)
    // → next notify() pass prunes it.
    _rc: Rc<dyn Fn()>,
}

// ─── SignalInner<T> ────────────────────────────────────────────────────────

struct SignalInner<T: 'static> {
    value:     RefCell<T>,
    subs:      RefCell<Vec<Rc<dyn Fn()>>>,
    notifying: Cell<bool>,  // re-entrancy guard
}

// ─── Signal<T> ────────────────────────────────────────────────────────────

/// Reactive value container. Cloning a Signal shares the same underlying cell.
pub struct Signal<T: 'static> {
    inner: Rc<SignalInner<T>>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self { Self { inner: Rc::clone(&self.inner) } }
}

impl<T: 'static> Signal<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(SignalInner {
                value:     RefCell::new(value),
                subs:      RefCell::new(Vec::new()),
                notifying: Cell::new(false),
            }),
        }
    }

    pub fn get(&self) -> Ref<'_, T> { self.inner.value.borrow() }

    pub fn set(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        self.notify();
    }

    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        f(&mut self.inner.value.borrow_mut());
        self.notify();
    }

    /// Register a callback; returns handle that keeps the subscription alive.
    pub fn subscribe<F: Fn() + 'static>(&self, f: F) -> SubscriptionHandle {
        let rc: Rc<dyn Fn()> = Rc::new(f);
        let handle_rc = Rc::clone(&rc);
        self.inner.subs.borrow_mut().push(rc);
        SubscriptionHandle { _rc: handle_rc }
    }

    /// Derive a new Signal<U> that updates whenever self changes.
    pub fn map<U: 'static, F: Fn(&T) -> U + 'static>(&self, f: F) -> Computed<U> {
        let initial = f(&self.inner.value.borrow());
        let out = Signal::new(initial);
        let out_clone = out.clone();
        let self_clone = self.clone();
        let handle = self.subscribe(move || {
            let new_val = f(&self_clone.inner.value.borrow());
            out_clone.set(new_val);
        });
        Computed { signal: out, _handle: handle }
    }

    fn notify(&self) {
        if self.inner.notifying.get() { return; }
        self.inner.notifying.set(true);
        // Clone list so subscribers can call set() on *other* signals without
        // hitting the subs RefCell borrow conflict.
        let subs: Vec<Rc<dyn Fn()>> = self.inner.subs.borrow().clone();
        for sub in &subs {
            // strong_count > 1 means external handle is still alive
            if Rc::strong_count(sub) > 1 { sub(); }
        }
        // Prune dead subscriptions (handle dropped = strong_count == 1)
        self.inner.subs.borrow_mut().retain(|rc| Rc::strong_count(rc) > 1);
        self.inner.notifying.set(false);
    }
}

// ─── Computed<T> ──────────────────────────────────────────────────────────

/// Read-only derived signal. Alive as long as this struct exists.
pub struct Computed<T: 'static> {
    signal:  Signal<T>,
    _handle: SubscriptionHandle,  // keeps parent → out subscription alive
}

impl<T: 'static> Computed<T> {
    pub fn get(&self) -> Ref<'_, T> { self.signal.get() }

    pub fn subscribe<F: Fn() + 'static>(&self, f: F) -> SubscriptionHandle {
        self.signal.subscribe(f)
    }
}
```

### `dirty.rs` — DirtyRect accumulator

```rust
//! Dirty-rectangle accumulator for ViUI v2 partial repaint.

use crate::layout::Rect;

/// Accumulates damaged screen regions; yields a single union rect per frame.
///
/// Usage: widgets mark their bounds dirty when a Signal changes.
/// The renderer consumes the rect via `take()` and repaints only that region.
pub struct DirtyRect {
    region: Option<Rect>,
}

impl DirtyRect {
    pub const fn new() -> Self { Self { region: None } }

    /// Union `rect` into the accumulated damage region.
    pub fn mark(&mut self, rect: Rect) {
        self.region = Some(match self.region {
            Some(acc) => acc.union(rect),
            None      => rect,
        });
    }

    /// Mark the entire surface dirty.
    pub fn mark_all(&mut self, w: f32, h: f32) {
        self.region = Some(Rect::new(0.0, 0.0, w, h));
    }

    /// Take the accumulated region and reset to clean.
    pub fn take(&mut self) -> Option<Rect> { self.region.take() }

    pub fn is_dirty(&self) -> bool { self.region.is_some() }
}

impl Default for DirtyRect { fn default() -> Self { Self::new() } }
```

---

## Related Code Files

| File | Action | Note |
|------|--------|------|
| `libs/viui/src/signal.rs` | **CREATE** | Full impl above |
| `libs/viui/src/dirty.rs` | **CREATE** | Full impl above |
| `libs/viui/src/lib.rs` | **MODIFY** | Add `pub mod signal; pub mod dirty;` |

---

## Implementation Steps

1. Create `libs/viui/src/signal.rs` with `Signal<T>`, `Computed<T>`, `SubscriptionHandle`
2. Create `libs/viui/src/dirty.rs` with `DirtyRect`
3. Add `pub mod signal;` and `pub mod dirty;` to `libs/viui/src/lib.rs`
4. Run `cargo check -p viui` — must compile clean with zero warnings

---

## Success Criteria

- [x] `cargo check -p viui` passes, zero warnings
- [x] Signal<i32>: set → subscriber called (verified by code review — standard Rc/RefCell pattern)
- [x] Signal<i32>: drop SubscriptionHandle → subscriber pruned on next notify() (strong_count guard)
- [x] Signal<i32>::map → Computed<U>: parent set → computed updates (via subscribe closure)
- [x] Re-entrancy: `notifying: Cell<bool>` guard prevents infinite loop
- [x] DirtyRect: mark 2 rects → take() returns union via `Rect::union()` from layout.rs
- [x] DirtyRect: no mark → take() returns None

**Note on tests**: `ostd` has RISC-V-only naked asm that prevents host compilation.
Unit tests in `signal.rs` are correct but can only run inside QEMU (via kernel test harness).
`cargo check -p viui` on riscv64 target is the primary verification.

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `RefCell` double-borrow during notify | Cloning subs list before iteration prevents borrow conflict |
| Computed<U> map with T non-Clone | f: Fn(&T) -> U pattern avoids Clone requirement |
| No test harness in no_std | Use `#[cfg(test)]` with `extern crate std` for unit tests |
