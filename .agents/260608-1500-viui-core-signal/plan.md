# ViUI v2 P01 — viui-core: Signal Engine + ViRenderer

**Plan ID**: 260608-1500-viui-core-signal  
**Stage**: G2  
**Priority**: P0 — foundation of all ViUI v2 work  
**Created**: 2026-06-08  
**Design Brief**: [.agents/brainstorms/260608-viui-nextgen-architecture.md](../brainstorms/260608-viui-nextgen-architecture.md)

---

## Mục tiêu

Xây dựng foundation layer cho ViUI v2 Reactive Signal Tree:

1. `signal.rs` — `Signal<T>`, `Computed<T>`, `SubscriptionHandle`, reactive dependency tracking
2. `dirty.rs` — `DirtyRect` accumulator (mark → union → take)
3. `renderer.rs` — `ViRenderer` trait (object-safe, swappable CPU/GPU) + `FramebufferRenderer` impl

**Không xóa v1 code** — `elm.rs`, `widget.rs`, `widgets/` vẫn giữ nguyên để tham khảo.

---

## Phase Table

| Phase | File | Nội dung | Status |
|-------|------|----------|--------|
| P01 | [phase-01-signal-engine.md](phase-01-signal-engine.md) | Signal<T>, Computed<T>, SubscriptionHandle, DirtyRect | ✅ Done |
| P02 | [phase-02-virenderer-trait.md](phase-02-virenderer-trait.md) | ViRenderer trait, FramebufferRenderer, wire into lib.rs | ✅ Done |

P02 cần DirtyRect từ P01 cho tham số `damage: Option<Rect>`.

---

## Files Modified

| File | Action |
|------|--------|
| `libs/viui/src/signal.rs` | **CREATE** |
| `libs/viui/src/dirty.rs` | **CREATE** |
| `libs/viui/src/renderer.rs` | **CREATE** |
| `libs/viui/src/lib.rs` | **MODIFY** — add 3 pub mod entries |

Không chạm `libs/api/` — không cần Law 1 confirmation.

---

## Key Design Decisions

### Signal<T>

- `Rc<SignalInner<T>>` — single-threaded (no Send/Sync), phù hợp UI loop
- `SignalInner`: `value: RefCell<T>`, `subs: RefCell<Vec<Rc<dyn Fn()>>>`, `notifying: Cell<bool>`
- `subscribe()` → `SubscriptionHandle { _rc: Rc<dyn Fn()> }` — drop handle = drop subscription
- `notify()`: clone subs list → call each → cleanup dead (strong_count == 1)
- Re-entrancy guard: `notifying` flag blocks nested `set()` calls from subscribers

### DirtyRect

- `Option<Rect>` wrapping `Rect::union()` (already in `layout.rs`)
- `mark(rect)` / `mark_all(w, h)` / `take() -> Option<Rect>` / `is_dirty()`

### ViRenderer

- Object-safe: `render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas))`
- Closure pattern solves `FramebufferCanvas<'fb>` borrow lifetime (canvas created inside closure)
- G1: `damage_all()` always; G2: `damage_rect(r)` when ViSurface API extends
