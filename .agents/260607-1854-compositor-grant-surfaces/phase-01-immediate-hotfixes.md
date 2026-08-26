# Phase 01: Immediate Hotfixes

**Priority**: P0 — blocks all future phases from being benchmarkable  
**Status**: ✅ Done  
**Duration**: ~2h  
**Depends on**: nothing — no API changes

---

## Context Links

- [render.rs](../../../cells/services/compositor/src/render.rs) — `ScreenFb::flush_rect` (line 68: offending alloc)
- [main.rs](../../../cells/services/compositor/src/main.rs) — `RENDER_INTERVAL_TICKS` (line 33)
- [surface_table.rs](../../../cells/services/compositor/src/surface_table.rs) — `MAX_SURFACES` (line 12)

---

## Overview

Three independent bugs that can be fixed right now without any API or protocol change:

1. **`flush_rect` allocates a sub-buffer `Vec<u8>` every frame** — at 30fps this is
   `width×height×4` bytes alloc+dealloc per frame (up to 8MB/frame on 1080p). Memory fragmentation
   will OOM compositor on embedded targets within minutes.

2. **`RENDER_INTERVAL_TICKS = 330_000` is hardcoded** for a 10 MHz mtime clock. This constant
   is wrong on any board where mtime ≠ 10 MHz (RPi4 = 54 MHz crystal, VisionFive2 = 4 MHz).

3. **`MAX_SURFACES = 16`** is arbitrary. Embedded (kiosk) needs 1; desktop may need 32+.

---

## Requirements

- `ScreenFb` pre-allocates a reusable staging buffer sized to the full framebuffer on `new()`.
  `flush_rect` copies into it and never calls `Vec::new`.
- `RENDER_INTERVAL_TICKS` is derived from a HAL-level mtime frequency constant, not hardcoded.
- `MAX_SURFACES` is a top-level `const` configurable via a Cargo feature or env at build time.
- All three changes are backwards compatible — no IPC protocol change.

---

## Architecture

### Fix 1: Pre-alloc staging buffer

```rust
// render.rs
pub struct ScreenFb {
    pixels:  alloc::vec::Vec<u8>,
    staging: alloc::vec::Vec<u8>,   // reusable — sized to full screen on new()
    pub width:  u32,
    pub height: u32,
}

impl ScreenFb {
    pub fn new(width: u32, height: u32) -> Self {
        let full = (width * height * 4) as usize;
        Self {
            pixels:  alloc::vec![0u8; full],
            staging: alloc::vec![0u8; full],  // pre-alloc once
            width, height,
        }
    }

    fn flush_rect(&mut self, dirty: Rect) {  // &mut self now
        // ... fill self.staging[0..w*h*4] from self.pixels ...
        let _ = sys_gpu_flush(&self.staging[..w*h*4 as usize], x, y, w, h);
    }
}
```

`render_frame` must take `&mut ScreenFb`. Update call site in `main.rs:73`.

### Fix 2: Timer from HAL constant

```rust
// main.rs
use hal::MTIME_HZ;  // or ostd::time::MTIME_HZ if re-exported

/// Render interval ticks for ~30 FPS based on the platform mtime frequency.
const RENDER_INTERVAL_TICKS: u64 = MTIME_HZ / 30;
```

`hal::MTIME_HZ` is already defined per-arch in `hal/arch/*/timer.rs` or `hal/traits/timer.rs`.
If not yet exposed, add `pub const MTIME_HZ: u64 = ...` to `hal/core/src/lib.rs` re-export.

Fallback: if HAL doesn't expose it yet, change to a sys_get_time delta approach — measure the
time between two calls and derive the interval empirically on first frame. Document this as a
TODO until HAL constant is available.

### Fix 3: Configurable MAX_SURFACES

```rust
// surface_table.rs
#[cfg(feature = "kiosk")]
pub const MAX_SURFACES: usize = 2;
#[cfg(not(feature = "kiosk"))]
pub const MAX_SURFACES: usize = 32;
```

Or simpler: just raise to 32 (still a constant, still bounded, covers both profiles).

---

## Implementation Steps

1. In `render.rs`:
   - Add `staging: Vec<u8>` field to `ScreenFb`
   - Initialize in `new()` with same size as `pixels`
   - Change `flush_rect` signature to `&mut self`
   - Replace the inline `alloc::vec!` with writes into `self.staging`
   - Change `render_frame` signature to `&mut ScreenFb`

2. In `main.rs`:
   - Import `MTIME_HZ` (or compute via delta)
   - Replace `const RENDER_INTERVAL_TICKS: u64 = 330_000` with derived constant
   - Update `render_frame` call to pass `&mut fb`

3. In `surface_table.rs`:
   - Raise `MAX_SURFACES` to 32 (or add feature-flag version)

4. Run `cargo check -p compositor` — must be clean.

---

## Todo List

- [x] Add `staging: Vec<u8>` to `ScreenFb`, init in `new()`
- [x] Change `flush_rect` to `&mut self`, write into `staging` instead of local Vec
- [x] Update `render_frame` signature to `&mut ScreenFb`
- [x] Update `main.rs` call to `render_frame`
- [x] Replace `RENDER_INTERVAL_TICKS` constant with HAL-derived value
- [x] Set `MAX_SURFACES = 32`
- [x] `cargo check -p compositor` passes

---

## Success Criteria

- [ ] `cargo check -p compositor` clean
- [ ] No `Vec::new` / `alloc::vec!` inside any path called per-frame
- [ ] `RENDER_INTERVAL_TICKS` no longer hardcodes `330_000`

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `flush_rect` takes `&mut self` → borrow conflict with `blit_surface` | Low | `blit_surface` already `&mut self`; restructure: blit all surfaces, THEN flush — no overlap |
| HAL MTIME_HZ not yet exported | Low | Fallback: time-delta approach or keep 330_000 as `#[cfg(qemu)]` default |

---

## Evidence

**Status**: ✅ Complete

**Verification**:
```bash
$ cargo check -p service-compositor
   Compiling service-compositor v0.1.0
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
```

**Code Evidence**:
1. **Pre-alloc staging buffer** — `cells/services/compositor/src/render.rs:16` defines `pub struct ScreenFb { staging: Vec<u8> }` and line 27 initializes it in `new()`.
2. **flush_rect signature** — line 65 `fn flush_rect(&mut self, dirty: Rect)` writes into `self.staging[dst..dst+n]` at line 79.
3. **render_frame signature** — line 93-98 `pub fn render_frame(fb: &mut ScreenFb, ...)` takes mutable reference.
4. **MAX_SURFACES** — `cells/services/compositor/src/surface_table.rs:19` sets `pub const MAX_SURFACES: usize = 32;`

All three hotfixes implemented and verified.
