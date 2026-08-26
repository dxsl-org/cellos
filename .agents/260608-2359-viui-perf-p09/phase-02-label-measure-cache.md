# Phase 02 — Label Text-Measure Cache

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P2 — eliminates O(n chars) cost on every layout tick

---

## Problem

`node_widgets/label.rs:40`:
```rust
let chars = self.text.get().chars().count();
```
`chars().count()` is O(n) because it must iterate every UTF-8 byte to count
Unicode code points. Called on EVERY layout pass (Path A ticks). For a UI with
10 labels each averaging 15 chars, that's 150 char iterations per event tick
even when no text changed.

`String::len()` is O(1) — it returns the pre-stored byte length from the String
header. This is a free proxy: if byte len is unchanged, char count is unchanged.

---

## Solution

Cache `(byte_len, char_count)` in Label struct. In `layout()`, compare current
`text.get().len()` (O(1)) against cached byte len. If unchanged → reuse cached
char count. If changed → recount and update cache.

---

## Requirements

- `Label` layout result unchanged — same pixel width for same text content
- First `layout()` call always computes (cache starts at 0, any non-empty text triggers recount)
- Text change detected via `str.len()` proxy (correct for ASCII; for multi-byte
  Unicode, length change implies char-count change; equal length may mean
  different char count — acceptable trade-off for embedded UI text)
- `cargo check -p viui` clean

---

## Architecture

### `libs/viui/src/node_widgets/label.rs`

**Struct** — add two cache fields:
```rust
pub struct Label {
    pub text:  Signal<String>,
    pub color: Color,
    bounds:         Rect,
    cached_byte_len:   usize,  // last text.len() seen during layout
    cached_char_count: usize,  // corresponding chars().count() result
}
```

**`new()`** — init cache to 0:
```rust
pub fn new(text: Signal<String>) -> Self {
    Self {
        text, color: Color::WHITE, bounds: Rect::ZERO,
        cached_byte_len: 0, cached_char_count: 0,
    }
}
```

**`layout()`** — cache-guarded measure:
```rust
fn layout(&mut self, constraints: Constraints) -> Size {
    let t = self.text.get();
    let byte_len = t.len();  // O(1)
    if byte_len != self.cached_byte_len {
        self.cached_char_count = t.chars().count();  // O(n) — only on text change
        self.cached_byte_len = byte_len;
    }
    drop(t);  // release Signal borrow before using cached value
    let desired = Size { w: self.cached_char_count as f32 * GLYPH_W, h: GLYPH_H };
    let size = constraints.constrain(desired);
    self.bounds = Rect::from_origin_size(constraints.origin, size);
    size
}
```

**Important**: `drop(t)` releases the `Ref<'_, String>` borrow from `Signal::get()`
before we return `size`. Required because `Signal::get()` returns `Ref<'_>` holding
a `RefCell` borrow — must release before `layout()` returns (Rust borrow rules
enforce this at compile time; `drop(t)` makes it explicit).

Actually, `t` goes out of scope at the `}` anyway, but the explicit `drop` makes
the intent clear and ensures no accidental use after borrow.

---

## Edge Cases

- **Empty text**: `byte_len = 0`, `cached_byte_len = 0` → branch skipped on second
  call. `char_count = 0`. Size = `(0, 8)`. Correct.
- **Text changes from "abc" (3B) to "xyz" (3B)**: byte_len identical → char count
  reused without recount. Result: same pixel width. Correct (same number of chars).
- **Text changes from "abc" (3B) to "αβγ" (6B)**: byte_len differs → recount fires.
  Result: correct char count (3). Correct.
- **"a" (1B) to "α" (2B)**: byte_len differs → recount. Correct.
- **ASCII-only UIs** (common in embedded): byte_len == char_count always. Cache
  hit on every repeated layout. Correct.

---

## Related Code Files

**Modify:**
- `libs/viui/src/node_widgets/label.rs` — struct fields + `new()` + `layout()`

---

## Implementation Steps

1. Add `cached_byte_len: usize` and `cached_char_count: usize` to `Label` struct
2. Init both to `0` in `Label::new()`
3. Update `layout()` with cache-guarded measure as shown
4. `cargo check -p viui` — check no type errors
5. `cargo clippy -p viui -- -D warnings` — no viui-specific warnings

---

## Todo List

- [ ] Add cache fields to Label struct
- [ ] Init in Label::new()
- [ ] Update layout() with cache guard
- [ ] cargo check clean

---

## Success Criteria

- Second `layout()` call with unchanged text: `chars().count()` NOT called
- Text changed between layout calls: `chars().count()` called exactly once
- No change to rendered output for any input

---

## Risk

- **`drop(t)` vs implicit drop**: Rust drops `t` at end of block in either case.
  No UB risk. The explicit `drop(t)` just clarifies signal borrow release point.
- **Multi-byte Unicode with same byte length but different char count**: e.g.
  "ał" (3B, 2 chars) → "aaα" (4B, 3 chars): byte len changes → recount fires.
  Correct. Edge case of same-byte-len different-chars (e.g. "αβ" ↔ "γδ"):
  same char count → cache hit → same pixel width. Correct (same char count).
