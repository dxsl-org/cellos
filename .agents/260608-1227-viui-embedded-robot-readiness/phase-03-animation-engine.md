# Phase 03 — Animation Engine

**Status:** Planned  
**Priority:** High — robot UX feedback (sensor update blinks, value transitions)  
**Estimate:** 2-3 ngày  
**Depends on:** Phase 01 (app_runner.rs structure), Phase 02 (ProgressBar/Slider use animations)

---

## Context Links

- [`libs/viui/src/app_runner.rs`](../../../libs/viui/src/app_runner.rs):64 — `tick()` cần ms_elapsed
- [`libs/viui/src/signal.rs`](../../../libs/viui/src/signal.rs) — Signal<T> là target của animation output

---

## Overview

ViOS không có wall-clock RTC cần thiết. Animation engine dùng **frame-elapsed-ms** được
tính từ kernel timer ticks, truyền vào `ViApp::tick(ms_elapsed: u64)`.

Target: smooth 200ms transition cho value changes — sensor level, battery indicator, loading bar.

---

## Requirements

- Easing functions: `linear`, `ease_in`, `ease_out`, `ease_in_out` (pure f32 math, no alloc)
- `Tween<f32>`: animates một f32 giá trị từ start đến end trong `duration_ms`
- `AnimTimer`: global tick counter, driven by `ViApp::tick()`
- Animate `Signal<f32>` mượt: `signal.animate_to(target, 200)` — trigger animation, update signal mỗi frame
- Widgets không cần biết về animation — chúng chỉ observe Signal<f32> như bình thường

---

## Architecture

### Easing (pure functions, no alloc)

```rust
// libs/viui/src/animation.rs

/// Normalized easing functions. `t` ∈ [0.0, 1.0] → output ∈ [0.0, 1.0].
pub mod easing {
    pub fn linear(t: f32) -> f32 { t }
    pub fn ease_in(t: f32) -> f32 { t * t }
    pub fn ease_out(t: f32) -> f32 { t * (2.0 - t) }
    pub fn ease_in_out(t: f32) -> f32 {
        if t < 0.5 { 2.0 * t * t } else { -1.0 + (4.0 - 2.0 * t) * t }
    }
}
```

### Tween<f32>

```rust
pub struct Tween {
    start:       f32,
    end:         f32,
    duration_ms: u32,
    elapsed_ms:  u32,
    easing:      fn(f32) -> f32,
    done:        bool,
}

impl Tween {
    pub fn new(start: f32, end: f32, duration_ms: u32) -> Self { ... }
    pub fn with_easing(mut self, f: fn(f32) -> f32) -> Self { ... }

    /// Advance by `dt` ms. Returns current value.
    pub fn tick(&mut self, dt: u32) -> f32 {
        self.elapsed_ms = (self.elapsed_ms + dt).min(self.duration_ms);
        self.done = self.elapsed_ms >= self.duration_ms;
        let t = self.elapsed_ms as f32 / self.duration_ms as f32;
        let t_eased = (self.easing)(t.clamp(0.0, 1.0));
        self.start + (self.end - self.start) * t_eased
    }

    pub fn is_done(&self) -> bool { self.done }
}
```

### AnimatedSignal<f32>

Combines Signal<f32> (target) với một active Tween:

```rust
pub struct AnimatedSignal {
    signal:      Signal<f32>,
    tween:       Option<Tween>,
}

impl AnimatedSignal {
    pub fn new(initial: f32) -> Self {
        Self { signal: Signal::new(initial), tween: None }
    }

    /// Signal handle để subscribe hoặc pass to widgets.
    pub fn signal(&self) -> Signal<f32> { self.signal.clone() }

    /// Jump immediately (no animation).
    pub fn set(&self, value: f32) { self.signal.set(value); }

    /// Animate from current value to `target` in `duration_ms`.
    pub fn animate_to(&mut self, target: f32, duration_ms: u32) {
        let start = *self.signal.get();
        self.tween = Some(Tween::new(start, target, duration_ms));
    }

    /// Called by ViApp each tick. Returns true if animation is active.
    pub fn tick(&mut self, dt_ms: u32) -> bool {
        if let Some(tween) = &mut self.tween {
            let v = tween.tick(dt_ms);
            self.signal.set(v);
            if tween.is_done() { self.tween = None; }
            true
        } else {
            false
        }
    }
}
```

### ViApp animation integration

`ViApp::tick()` nhận `dt_ms: u32` (milliseconds since last tick):

```rust
// Before: pub fn tick(&mut self, events: &[Event]) -> bool
// After:  pub fn tick(&mut self, events: &[Event], dt_ms: u32) -> bool
```

User app giữ danh sách `AnimatedSignal` bên ngoài ViApp và call `.tick(dt_ms)` mỗi frame
trước khi call `app.tick()`.

**Why bên ngoài ViApp?** — ViApp không quản lý animated signals để tránh borrow cycles
(AnimatedSignal owns Signal; widget borrows Signal; ViApp borrows widget).

Mẫu sử dụng:

```rust
// Trong cell main loop:
let mut battery = AnimatedSignal::new(0.0_f32);
let progress_bar = ProgressBar::new(battery.signal());
let mut app = ViApp::new(Box::new(progress_bar), renderer);

loop {
    let dt = kernel_ticks_to_ms(read_timer());
    battery.tick(dt);            // advance animation
    app.tick(&input_events, dt); // process events + render
}
```

**Alternative (không break existing API):** Thêm `AnimationList` vào `ViApp`:

```rust
pub struct ViApp {
    // ...existing
    animations: Vec<Box<dyn Animatable>>,  // NEW
}
impl ViApp {
    pub fn add_animation(&mut self, anim: Box<dyn Animatable>) { ... }
    pub fn tick(&mut self, events: &[Event], dt_ms: u32) -> bool {
        let mut needs_render = false;
        for a in &mut self.animations {
            if a.tick(dt_ms) { needs_render = true; }
        }
        // ...existing tick logic
    }
}
```

**Chọn approach 2** (ViApp-owned AnimationList) — cleaner API, nhất quán.

### Animatable trait

```rust
pub trait Animatable {
    /// Advance by dt_ms. Returns true if animation caused a redraw (signal changed).
    fn tick(&mut self, dt_ms: u32) -> bool;
}

impl Animatable for AnimatedSignal { ... }
```

---

## ViOS Timer Integration

Cell lấy elapsed time từ kernel via syscall:

```rust
// libs/ostd/src/io.rs hoặc task.rs — đã có mtime read?
// Nếu chưa, thêm:
pub fn elapsed_ms_since(last: u64) -> u64 {
    let now = sys_get_time(); // syscall GetTime (đã có từ RTC plan)
    now.saturating_sub(last) / 1_000_000  // nanoseconds → ms
}
```

Nếu GetTime syscall chưa available trong G1, dùng RISC-V `rdtime` CSR qua ostd::io.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/animation.rs` | CREATE — Tween, AnimatedSignal, Animatable, easing |
| `libs/viui/src/app_runner.rs` | MODIFY — add animations: Vec<Box<dyn Animatable>>, tick(dt_ms) |
| `libs/viui/src/lib.rs` | MODIFY — pub mod animation, re-export AnimatedSignal |
| `libs/ostd/src/io.rs` | VERIFY/MODIFY — elapsed time read |

---

## Implementation Steps

1. Tạo `libs/viui/src/animation.rs` với `easing` module, `Tween`, `AnimatedSignal`, `Animatable`
2. Update `ViApp::tick()` signature: thêm `dt_ms: u32`
3. Thêm `animations: Vec<Box<dyn Animatable>>` vào `ViApp`
4. `ViApp::add_animation()` + tick animations trước event processing
5. Verify `ostd::io` có way để get elapsed ms (rdtime hoặc GetTime syscall)
6. `cargo check` full workspace
7. Test: `AnimatedSignal::animate_to(1.0, 500)`, tick 100ms/frame × 5 frames → value reaches 1.0

---

## Todo

- [ ] Tạo animation.rs (easing + Tween + AnimatedSignal + Animatable)
- [ ] Update ViApp::tick() nhận dt_ms, advance animations trước event
- [ ] Thêm ViApp::add_animation()
- [ ] Verify ostd timer API
- [ ] cargo check pass
- [ ] Unit test: Tween 0→1 trong 200ms, tick 4 × 50ms → value=1.0 at end
- [ ] Integration test: ProgressBar với AnimatedSignal — smooth fill

---

## Success Criteria

- `Tween::new(0.0, 1.0, 200).tick(200)` = 1.0 với bất kỳ easing
- `AnimatedSignal::animate_to(1.0, 200)` + 4 × `tick(50)` → Signal<f32> = 1.0
- ProgressBar animated từ 0→1 trong 500ms không skip frames (mỗi tick render một lần)
- Không có alloc trong easing/Tween hot path

---

## Risk

**dt_ms accuracy**: QEMU TCG timer không realtime — jitter cao. Animation vẫn correct về
giá trị cuối (clamp to duration), chỉ mượt hơn trên real hardware. Acceptable for G1.

**Backward compat**: `ViApp::tick(&events, dt_ms)` breaks existing viui-demo. Fix bằng cách
update viui-demo cùng lúc, hoặc giữ `tick(&events)` với `dt_ms=0` (no animation advance)
qua default parameter workaround (không có trong Rust, dùng separate method thay thế):
```rust
pub fn tick(&mut self, events: &[Event]) -> bool { self.tick_with_dt(events, 0) }
pub fn tick_with_dt(&mut self, events: &[Event], dt_ms: u32) -> bool { ... }
```
