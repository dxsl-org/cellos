# Phase 03 — DrawText Short-String Dedup

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P3 — independent (can execute after Phase 01 or in parallel)

---

## Overview

`GpuCanvas::draw_text()` currently does `String::from(text)` on every paint call,
allocating a heap string even for a 3-character label. For embedded targets with
limited heap and no allocator caching, this is the dominant per-frame allocation.

Add `GpuCmd::DrawTextShort` — a stack-allocated variant using `[u8; 32]` for text
≤ 32 bytes. This covers 90%+ of typical UI strings (labels, button text, counters).
The existing `GpuCmd::DrawText { text: String }` remains for longer strings.

---

## Key Insights

- `GpuCanvas::draw_text()` is called once per widget per paint pass — the allocation
  path is hit on EVERY frame for EVERY text widget, even when text didn't change.
- 32 bytes fits: "Increment" (9), "Count: 999" (10), most button/label text.
- `[u8; 32]` + `len: u8` is 33 bytes on the stack vs. 24-byte `String` on heap.
  Total `GpuCmd::DrawTextShort` = `Point(8) + [u8;32](32) + u8(1) + TextStyle(8)` ≈ 56 bytes.
  Slightly larger on stack, but avoids `malloc` + cache miss for heap pointer.
- UTF-8 invariant: bytes are always written from `&str.as_bytes()` in `draw_text()`,
  so `from_utf8` in executor is infallible. Use `unwrap_or("")` defensively.

---

## Requirements

### Functional
- `GpuCmd::DrawTextShort { pos: Point, bytes: [u8; 32], len: u8, style: TextStyle }` added
- `GpuCanvas::draw_text()`: use `DrawTextShort` if `text.len() <= 32`; else `DrawText`
- `CpuExecutor::execute()`: handle `DrawTextShort` — reconstruct `&str`, call `canvas.draw_text()`
- `GpuCmd::bounding_rect()`: handle `DrawTextShort` same as `DrawText` (pos + len estimation)

### Non-functional
- `gpu_cmd.rs` stays ≤ 100 lines (currently ~74)
- `gpu_canvas.rs` stays ≤ 100 lines (currently ~88)
- `executor.rs` stays ≤ 130 lines (currently ~117)
- No unsafe code (from_utf8, not from_utf8_unchecked)

---

## Architecture

### `libs/viui/src/gpu_cmd.rs` — add DrawTextShort variant

```rust
pub enum GpuCmd {
    FillRect      { rect: Rect, color: Color },
    DrawLine      { a: Point, b: Point, color: Color },
    DrawText      { pos: Point, text: String, style: TextStyle },
    DrawImage     { dest: Rect, pixels: Vec<u8>, src_stride: u32 },
    /// Zero-alloc path for text ≤ 32 bytes. Written by GpuCanvas::draw_text.
    DrawTextShort { pos: Point, bytes: [u8; 32], len: u8, style: TextStyle },
}

impl GpuCmd {
    pub fn bounding_rect(&self) -> Option<Rect> {
        match self {
            GpuCmd::FillRect      { rect, .. }           => Some(*rect),
            GpuCmd::DrawLine      { a, b, .. }           => Some(Rect::bounding_points(*a, *b)),
            GpuCmd::DrawText      { pos, text, style: _ } => {
                Some(Rect::new(pos.x, pos.y, text.len() as f32 * 8.0, 8.0))
            }
            GpuCmd::DrawImage     { dest, .. }           => Some(*dest),
            GpuCmd::DrawTextShort { pos, len, .. }       => {
                Some(Rect::new(pos.x, pos.y, *len as f32 * 8.0, 8.0))
            }
        }
    }
}
```

### `libs/viui/src/gpu_canvas.rs` — fast path in draw_text

```rust
fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
    if text.len() <= 32 {
        let mut bytes = [0u8; 32];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        self.buf.push(GpuCmd::DrawTextShort {
            pos, bytes, len: text.len() as u8, style,
        });
    } else {
        self.buf.push(GpuCmd::DrawText {
            pos, text: String::from(text), style,
        });
    }
}
```

### `libs/viui/src/executor.rs` — handle DrawTextShort

Add arm to the `match cmd` in `CpuExecutor::execute()`:

```rust
GpuCmd::DrawTextShort { pos, bytes, len, style } => {
    let text = core::str::from_utf8(&bytes[..*len as usize]).unwrap_or("");
    canvas.draw_text(*pos, text, *style);
}
```

This is placed before or after the `DrawText` arm — order doesn't matter.

---

## Related Code Files

**Modify:**
- `libs/viui/src/gpu_cmd.rs` — add `DrawTextShort` variant + update `bounding_rect`
- `libs/viui/src/gpu_canvas.rs` — fast path in `draw_text()`
- `libs/viui/src/executor.rs` — handle new variant in `execute()`

---

## Implementation Steps

1. `gpu_cmd.rs`: add `DrawTextShort` variant; update `bounding_rect()` match arm
2. `gpu_canvas.rs`: update `draw_text()` with `text.len() <= 32` branch
3. `executor.rs`: add `DrawTextShort` arm in `CpuExecutor::execute()`
4. `cargo check -p viui`
5. `cargo clippy -p viui -- -D warnings`

---

## Todo List

- [ ] gpu_cmd.rs: add DrawTextShort + update bounding_rect
- [ ] gpu_canvas.rs: fast path in draw_text
- [ ] executor.rs: add DrawTextShort arm
- [ ] cargo check passes
- [ ] cargo clippy clean

---

## Success Criteria

- `draw_text("OK", ...)` → no heap alloc (uses `DrawTextShort`)
- `draw_text("A very long string more than 32 bytes here!!")` → uses `DrawText { String }`
- `CpuExecutor` renders `DrawTextShort` identically to `DrawText` of same content
- `executor.rs` exhaustive match: compiler error if new variant added without arm

---

## Risk

- **Text > 32 bytes**: silently falls back to `DrawText { String }`. Correct.
- **UTF-8 multi-byte**: `text.len()` is byte count, not char count. A 10-char
  Japanese string may be 30 bytes → still fits in `DrawTextShort`. Correct.
- **`TextStyle` must be Copy**: check `canvas.rs` — if `TextStyle` is not `Copy`,
  `DrawTextShort { style: TextStyle }` hits a move error. Fix: derive `Copy` on
  `TextStyle` if not already present. Current code already copies `style` in
  `GpuCmd::DrawText`, so `TextStyle` is likely already Copy.
