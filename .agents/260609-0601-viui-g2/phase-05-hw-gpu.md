# Phase 05 — HW GPU Backend (GLES2)

## Overview

| | |
|---|---|
| **Priority** | Low |
| **Status** | Planned |
| **Stage** | G2 Wave 2 |
| **Crate** | `libs/viui` — new `canvas/gles2.rs` behind feature flag |
| **Parallel** | After Wave 1 (P01-P04) stabilizes |
| **Blocker** | Requires EGL + GLES2 driver availability in ViOS (not yet wired) |

Add an optional `gles2` feature to `libs/viui` that provides a `Gles2Canvas` implementing
`ViCanvas`. Batches draw calls into vertex buffers; maintains a glyph atlas texture.
All existing CPU-path behavior unchanged when feature is off.

---

## Key Insights

- `ViCanvas` trait is the abstraction layer. Adding a new impl is additive — no widget changes.
- GLES2 context must be obtained from the compositor (ViOS doesn't surface this yet). Phase 05
  is therefore **spec + skeleton** until compositor grants an EGL surface.
- Target: `ARM Mali-G52` (RK3588), `Imagination PowerVR` (Pi 4B), `llvmpipe` (QEMU softpipe).
- `no_std`: GLES2 bindings must be manually wired (no `glutin`/`winit` — those need std+OS).
  Use `gl_generator` crate at build time (build.rs) or manual FFI to `libGLESv2.so` via `dlopen`.
- Glyph atlas: 512×512 R8 texture; `FontContext` provides bitmaps; GPU path uploads on demand.
- Feature flag: `viui = { features = ["gles2"] }` in Cargo.toml; guards all GLES2 code.

---

## Requirements

### Functional
1. `feature = "gles2"` in `libs/viui/Cargo.toml` — disabled by default.
2. `Gles2Canvas` struct implementing `ViCanvas` (fill_rect, draw_line, draw_image, draw_text, clip).
3. `fill_rect`: batched colored quads → VBO → single draw call per color.
4. `draw_text`: glyph atlas upload + textured quads.
5. `clip_push/pop`: scissor rect stack via `glScissor`.
6. `ViSurfaceRenderer` gains optional GPU path: `pub fn new_gles2(...) -> Self`.
7. Fallback to CPU when GLES2 context unavailable.

### Non-functional
- `cargo check --workspace --features viui/gles2` — no errors on x86_64 (EGL stubs OK).
- `cargo check --workspace` (no flag) — identical behavior as today.

---

## Architecture

### Feature-gated module

```
libs/viui/src/
├── canvas.rs          — ViCanvas trait (unchanged)
├── fb_canvas.rs       — CPU framebuffer (unchanged)
└── gles2_canvas.rs    — #[cfg(feature = "gles2")] Gles2Canvas
```

### Gles2Canvas

```rust
#[cfg(feature = "gles2")]
pub struct Gles2Canvas<'a> {
    // EGL context handle (opaque, provided by caller)
    gl: &'a Gl,
    // Quad batch: (rect, color) pairs
    quads: Vec<ColoredQuad>,
    // Glyph atlas: 512×512 R8 texture, CPU-side bitmap cache
    atlas: &'a mut GlyphAtlas,
    // Scissor stack
    clip_stack: Vec<Rect>,
    width: u32,
    height: u32,
}

impl ViCanvas for Gles2Canvas<'_> { ... }
```

### Render loop

```
1. glClear(COLOR_BUFFER_BIT)
2. root.paint(&mut cx)         ← Gles2Canvas batches quads
3. gles2_canvas.flush()        ← submit batched VBO draw calls
4. eglSwapBuffers(...)
```

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/Cargo.toml` | **Modify** — add `[features] gles2 = []` |
| `libs/viui/src/gles2_canvas.rs` | **Create** — `#[cfg(feature = "gles2")]` |
| `libs/viui/src/lib.rs` | **Modify** — `#[cfg(feature = "gles2")] pub mod gles2_canvas;` |
| `libs/viui/src/surface_renderer.rs` | **Modify** — add `#[cfg(feature = "gles2")] pub fn new_gles2(...)` |

---

## Implementation Steps

1. Add `[features] gles2 = []` to `libs/viui/Cargo.toml`.
2. Create skeleton `gles2_canvas.rs` with `Gles2Canvas` struct, all `ViCanvas` methods as stubs
   returning `()` / `todo!()` with `#[cfg(feature = "gles2")]` guard.
3. Implement `fill_rect` batching: collect quads, flush on demand.
4. Implement scissor clip_push/pop via `glScissor`.
5. Implement glyph atlas: `GlyphAtlas` struct, upload on miss, textured quad draw.
6. Wire `new_gles2()` constructor in `surface_renderer.rs`.
7. Stub EGL context type so it compiles on non-EGL targets (feature-gated `#[cfg]`).
8. `cargo check --workspace --features viui/gles2`.

---

## Todo

- [ ] Add `gles2` feature flag to `libs/viui/Cargo.toml`
- [ ] Create `gles2_canvas.rs` skeleton with all ViCanvas stubs
- [ ] Implement fill_rect quad batching + VBO flush
- [ ] Implement scissor clip stack
- [ ] Implement glyph atlas (CPU-side bitmap → R8 texture)
- [ ] Wire `new_gles2()` in surface_renderer
- [ ] `cargo check --features viui/gles2` passes

---

## Success Criteria

1. `cargo check --workspace --features viui/gles2` — no errors.
2. `cargo check --workspace` (no feature) — identical to before.
3. Gles2Canvas implements all ViCanvas methods (no unimplemented!() panics).
4. robot-dashboard compiles with `viui = { features = ["gles2"] }`.

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| EGL not available in ViOS yet | Phase is skeleton + compile-check only; runtime use deferred |
| GLES2 FFI needs libGLESv2.so link | Stub via `#[cfg(target_os = "...")]` guards |
| Glyph atlas texture limits (512px) | Overflow: evict LRU glyphs (Phase 05b follow-up) |
| VBO quad batching overflows | Cap batch at 4096 quads; auto-flush on overflow |

---

## Security Considerations

GPU shader source is static (no user input). EGL context acquired from trusted compositor.
No unsafe beyond the GL FFI calls — document each `// SAFETY: GL call is correct per spec`.

---

## Next Steps

After Phase 05 skeleton: wire EGL surface grant from compositor (depends on Phase 03 G2 roadmap
item — compositor Grant surfaces). Full GPU rendering deferred until compositor integration.
