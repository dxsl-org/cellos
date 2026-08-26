# Phase 02 — DrawTextShort 32→128 Bytes

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P2 — eliminates heap alloc for any UI string ≤128 bytes

---

## Problem

`gpu_canvas.rs:66`:
```rust
if text.len() <= 32 {
    // stack buffer — zero alloc
} else {
    self.buf.push(GpuCmd::DrawText { pos, text: String::from(text), style });
    // ↑ heap alloc for every label text > 32 bytes
}
```

Many real UI strings exceed 32 bytes:
- `"Error: device not found"` — 24 bytes ✅
- `"Temperature: 42.5°C"` — 20 bytes ✅
- `"CPU: 87% | Mem: 1.2 GB free"` — 28 bytes ✅
- `"Connection timeout after 30s"` — 28 bytes ✅
- `"Error: timeout reading sensor 0x3F"` — 35 bytes ❌ → heap alloc
- `"ViCell v0.2.0 — embedded OS"` — 28 bytes ✅

128 bytes covers virtually all single-line UI labels. Only multi-paragraph text
or dynamically constructed strings (file paths, error dumps) exceed this.

---

## Solution

Extend `DrawTextShort` stack buffer from `[u8; 32]` to `[u8; 128]`, raise
threshold from `<= 32` to `<= 127`, keep `len: u8` (unchanged — u8 holds 0-255).

Stack cost: `GpuCmd::DrawTextShort` grows by 96 bytes per enum variant instance.
`GpuCommandBuffer` is a `Vec<GpuCmd>` — each slot is `size_of::<GpuCmd>()`.
`GpuCmd` is already sized by its largest variant; if `DrawImage` (with `Vec<u8>`)
is larger, the enum size is unchanged. If not, it grows by 96 bytes.
Either way, the retained `buf` field in `GpuRenderer` means no extra alloc per frame.

---

## Requirements

- `GpuCmd::DrawTextShort.bytes` extended to `[u8; 128]`
- `GpuCanvas::draw_text` threshold: `text.len() <= 127`
  (using 127 not 128: `len: u8` holds 127, array index `[..127]` is valid for `[u8;128]`)
  Actually `<= 128` works since `bytes[..128].copy_from_slice` on a `[u8;128]` is valid.
  Using `<= 127` is more conservative and clearer intent.
- `executor.rs`: no change needed — already uses `bytes[..*len as usize]`
- `cargo check -p viui` + `cargo check -p viui-demo` clean
- No `libs/api/` changes — Law 1 not triggered

---

## Architecture

### `libs/viui/src/gpu_cmd.rs` — extend variant

**Before (`gpu_cmd.rs:29`):**
```rust
DrawTextShort { pos: Point, bytes: [u8; 32], len: u8, style: TextStyle },
```

**After:**
```rust
DrawTextShort { pos: Point, bytes: [u8; 128], len: u8, style: TextStyle },
```

Also update the doc comment to reflect new capacity:
```rust
/// Zero-alloc path for text ≤ 127 bytes (covers all typical single-line UI strings).
```

---

### `libs/viui/src/gpu_canvas.rs` — raise threshold

**Before (`gpu_canvas.rs:65-73`):**
```rust
fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
    if text.len() <= 32 {
        let mut bytes = [0u8; 32];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        self.buf.push(GpuCmd::DrawTextShort { pos, bytes, len: text.len() as u8, style });
    } else {
        self.buf.push(GpuCmd::DrawText { pos, text: String::from(text), style });
    }
}
```

**After:**
```rust
fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
    if text.len() <= 127 {
        let mut bytes = [0u8; 128];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        self.buf.push(GpuCmd::DrawTextShort { pos, bytes, len: text.len() as u8, style });
    } else {
        self.buf.push(GpuCmd::DrawText { pos, text: String::from(text), style });
    }
}
```

---

### `libs/viui/src/executor.rs` — no change needed

The executor arm is:
```rust
GpuCmd::DrawTextShort { pos, bytes, len, style } => {
    let text = core::str::from_utf8(&bytes[..*len as usize]).unwrap_or("");
    canvas.draw_text(*pos, text, *style);
}
```
`bytes[..*len as usize]` uses the runtime `len` — no hardcoded 32. Works for any array size.

---

## Related Code Files

**Modify:**
- `libs/viui/src/gpu_cmd.rs` — array size + doc comment
- `libs/viui/src/gpu_canvas.rs` — threshold + buffer size

**No change:**
- `libs/viui/src/executor.rs`
- All widget files

---

## Implementation Steps

1. `gpu_cmd.rs`: change `[u8; 32]` → `[u8; 128]`, update doc comment
2. `gpu_canvas.rs`: change `<= 32` → `<= 127`, change `[0u8; 32]` → `[0u8; 128]`
3. `cargo check -p viui` — verify no type errors
4. `cargo check -p viui-demo`

---

## Todo List

- [ ] gpu_cmd.rs: array [u8; 32] → [u8; 128] + update doc
- [ ] gpu_canvas.rs: threshold 32→127 + buffer [0u8; 32]→[0u8; 128]
- [ ] cargo check -p viui passes
- [ ] cargo check -p viui-demo passes

---

## Success Criteria

- `gpu_canvas.rs::draw_text`: `String::from()` not called for text ≤127 bytes (verifiable)
- `cargo check -p viui` clean
- Text ≤127 bytes recorded as `DrawTextShort` — zero heap alloc per draw call

---

## Risk

- **`GpuCmd` enum size increase**: if `DrawTextShort` is the largest variant, each slot
  in the `Vec<GpuCmd>` grows by 96 bytes. For a typical 20-command frame buffer,
  total growth = 20 × 96 = 1920 bytes. Acceptable for embedded (>4KB heap).
- **`DrawImage` variant is larger**: `DrawImage` holds `Vec<u8>` (3×pointer = 24 bytes),
  far less than `[u8;128]` (128 bytes). So `DrawTextShort` will become the largest
  variant after this change. Enum size = ~128 + overhead.
- **Threshold `<= 127` vs `<= 128`**: `bytes[..128]` on `[u8;128]` is valid (full slice).
  Using `<= 127` is a conservative choice to avoid any risk of off-by-one. Can safely
  use `<= 128` if we want maximum coverage; both compile and both are correct.
